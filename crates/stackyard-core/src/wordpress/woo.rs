//! WooCommerce helpers: the store's state, scheduled actions, payment
//! gateways switched between test and live mode, and test customers and
//! orders.
//!
//! Stores keep orders in one of two ways, and both are read: in their own
//! tables (High-Performance Order Storage, the default since WooCommerce
//! 8.2) or as posts of the type `shop_order`.

use std::path::Path;

use serde::Serialize;

use super::php::{self, Value};
use super::{fail, Load, WpSite};
use crate::databases::{ident, literal};
use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Status {
    pub version: Option<String>,
    /// Orders live in WooCommerce's own tables (HPOS).
    pub hpos: bool,
    pub orders: u64,
    pub products: u64,
    pub customers: u64,
    /// Scheduled actions that are due and have not run.
    pub due_actions: u64,
    pub failed_actions: u64,
    pub currency: Option<String>,
}

/// Whether the store keeps orders in their own tables.
pub fn uses_hpos(home: &Path, wp: &WpSite) -> Result<bool> {
    Ok(wp
        .option(home, "woocommerce_custom_orders_table_enabled")?
        .as_deref()
        == Some("yes"))
}

fn count(home: &Path, wp: &WpSite, sql: &str) -> u64 {
    wp.sql(home, sql)
        .ok()
        .and_then(|rows| rows.into_iter().next())
        .and_then(|r| r.into_iter().next().flatten())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

fn table_exists(home: &Path, wp: &WpSite, table: &str) -> bool {
    count(
        home,
        wp,
        &format!(
            "SELECT COUNT(*) FROM information_schema.TABLES WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = {}",
            literal(table)
        ),
    ) > 0
}

pub fn status(home: &Path, wp: &WpSite) -> Result<Status> {
    let hpos = uses_hpos(home, wp)?;
    let posts = ident(&wp.table("posts"));
    let orders = if hpos {
        count(
            home,
            wp,
            &format!(
                "SELECT COUNT(*) FROM {} WHERE type = 'shop_order'",
                ident(&wp.table("wc_orders"))
            ),
        )
    } else {
        count(
            home,
            wp,
            &format!("SELECT COUNT(*) FROM {posts} WHERE post_type = 'shop_order'"),
        )
    };
    let actions = wp.table("actionscheduler_actions");
    let (due, failed) = if table_exists(home, wp, &actions) {
        (
            count(
                home,
                wp,
                &format!(
                    "SELECT COUNT(*) FROM {} WHERE status = 'pending' AND scheduled_date_gmt <= UTC_TIMESTAMP()",
                    ident(&actions)
                ),
            ),
            count(
                home,
                wp,
                &format!("SELECT COUNT(*) FROM {} WHERE status = 'failed'", ident(&actions)),
            ),
        )
    } else {
        (0, 0)
    };
    let customers_table = wp.table("wc_customer_lookup");
    let customers = if table_exists(home, wp, &customers_table) {
        count(
            home,
            wp,
            &format!("SELECT COUNT(*) FROM {}", ident(&customers_table)),
        )
    } else {
        0
    };
    Ok(Status {
        version: wp.plugin_version("woocommerce"),
        hpos,
        orders,
        products: count(
            home,
            wp,
            &format!("SELECT COUNT(*) FROM {posts} WHERE post_type = 'product' AND post_status <> 'trash'"),
        ),
        customers,
        due_actions: due,
        failed_actions: failed,
        currency: wp.option(home, "woocommerce_currency")?,
    })
}

/// Runs the scheduled actions that are due now (WooCommerce's emails,
/// webhooks, subscription renewals…), as the site's cron would.
pub fn run_due_actions(home: &Path, wp: &WpSite) -> Result<String> {
    let out = wp.wp(
        home,
        Load::Everything,
        &["action-scheduler", "run", "--batches=10"],
    )?;
    Ok(out
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim()
        .to_string())
}

/// A payment gateway and whether it is in test mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Gateway {
    /// e.g. `stripe`, from the option `woocommerce_stripe_settings`.
    pub id: String,
    pub title: String,
    pub enabled: bool,
    /// `None` when the gateway has no setting Stackyard knows how to switch.
    pub test_mode: Option<bool>,
}

/// Settings keys gateways use for test mode, with the values for on and off.
const TEST_KEYS: &[(&str, &str, &str)] = &[
    ("testmode", "yes", "no"),
    ("test_mode", "yes", "no"),
    ("sandbox", "yes", "no"),
    ("sandbox_mode", "yes", "no"),
    ("sandbox_on", "yes", "no"),
    ("environment", "sandbox", "production"),
];

/// Gateway settings that are not gateways.
const NOT_GATEWAYS: &[&str] = &[
    "email",
    "new_order",
    "cancelled_order",
    "customer",
    "admin",
    "tracker",
    "onboarding",
    "task",
];

pub fn gateways(home: &Path, wp: &WpSite) -> Result<Vec<Gateway>> {
    let rows = wp.sql(
        home,
        &format!(
            "SELECT option_name, option_value FROM {} WHERE option_name LIKE 'woocommerce\\_%\\_settings' ORDER BY option_name",
            ident(&wp.table("options"))
        ),
    )?;
    let mut found = Vec::new();
    for row in rows {
        let (Some(Some(name)), Some(Some(value))) = (row.first(), row.get(1)) else {
            continue;
        };
        let id = name
            .trim_start_matches("woocommerce_")
            .trim_end_matches("_settings")
            .to_string();
        if NOT_GATEWAYS.iter().any(|n| id.contains(n)) {
            continue;
        }
        let Some(settings) = php::unserialize(value.as_bytes()) else {
            continue;
        };
        // Only settings with an on/off switch are payment methods.
        let Some(enabled) = settings.get("enabled").and_then(Value::as_str) else {
            continue;
        };
        let test_mode = TEST_KEYS
            .iter()
            .find_map(|(key, on, _)| settings.get(key).and_then(Value::as_str).map(|v| v == *on));
        let title = settings
            .get("title")
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
            .unwrap_or(&id)
            .to_string();
        found.push(Gateway {
            id,
            title,
            enabled: enabled == "yes",
            test_mode,
        });
    }
    Ok(found)
}

/// Switches a gateway's test mode on or off.
pub fn set_test_mode(home: &Path, wp: &WpSite, id: &str, on: bool) -> Result<()> {
    if !id
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(fail(format!("no payment gateway {id:?}")));
    }
    let option = format!("woocommerce_{id}_settings");
    let raw = wp
        .option(home, &option)?
        .ok_or_else(|| fail(format!("no payment gateway {id:?}")))?;
    let mut settings = php::unserialize(raw.as_bytes())
        .ok_or_else(|| fail(format!("the settings of {id} can't be read")))?;
    let (key, yes, no) = TEST_KEYS
        .iter()
        .find(|(key, _, _)| settings.get(key).is_some())
        .ok_or_else(|| {
            fail(format!(
                "{id} has no test mode Stackyard knows how to switch; use its own settings page"
            ))
        })?;
    settings.set(key, Value::str(if on { yes } else { no }));
    wp.set_option(home, &option, &settings.serialize(), true)
}

/// The first administrator's ID, which WooCommerce's commands run as.
fn admin_id(home: &Path, wp: &WpSite) -> Result<String> {
    let out = wp.wp(
        home,
        Load::Core,
        &[
            "user",
            "list",
            "--role=administrator",
            "--field=ID",
            "--number=1",
        ],
    )?;
    out.lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && l.bytes().all(|b| b.is_ascii_digit()))
        .map(str::to_string)
        .ok_or_else(|| fail("the site has no administrator to run WooCommerce's commands as"))
}

fn random_word() -> String {
    let mut bytes = [0u8; 4];
    let _ = getrandom::fill(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Makes `count` test customers with `@example.test` addresses (a domain
/// that never receives mail); returns their e-mail addresses.
pub fn create_customers(home: &Path, wp: &WpSite, count: u32) -> Result<Vec<String>> {
    let admin = admin_id(home, wp)?;
    let user = format!("--user={admin}");
    let mut made = Vec::new();
    for _ in 0..count.clamp(1, 50) {
        let word = random_word();
        let email = format!("test-{word}@example.test");
        wp.wp(
            home,
            Load::Everything,
            &[
                "wc",
                "customer",
                "create",
                &format!("--email={email}"),
                &format!("--username=test-{word}"),
                "--first_name=Test",
                &format!("--last_name=Customer {word}"),
                &user,
                "--porcelain",
            ],
        )?;
        made.push(email);
    }
    Ok(made)
}

/// Makes `count` test orders, each for one of the store's published
/// products, marked as processing; returns their numbers.
pub fn create_orders(home: &Path, wp: &WpSite, count: u32) -> Result<Vec<String>> {
    let admin = admin_id(home, wp)?;
    let products: Vec<String> = wp
        .sql(
            home,
            &format!(
                "SELECT ID FROM {} WHERE post_type = 'product' AND post_status = 'publish' ORDER BY ID DESC LIMIT 20",
                ident(&wp.table("posts"))
            ),
        )?
        .into_iter()
        .filter_map(|r| r.into_iter().next().flatten())
        .collect();
    if products.is_empty() {
        return Err(fail("the store has no published products to order"));
    }
    let user = format!("--user={admin}");
    let mut made = Vec::new();
    for i in 0..count.clamp(1, 50) as usize {
        let product = &products[i % products.len()];
        let items = format!("--line_items=[{{\"product_id\":{product},\"quantity\":1}}]");
        let billing = "--billing={\"first_name\":\"Test\",\"last_name\":\"Order\",\"email\":\"orders@example.test\"}";
        let id = wp.wp(
            home,
            Load::Everything,
            &[
                "wc",
                "shop_order",
                "create",
                &items,
                billing,
                "--status=processing",
                "--set_paid=true",
                &user,
                "--porcelain",
            ],
        )?;
        made.push(id.trim().to_string());
    }
    Ok(made)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keys_cover_common_gateways() {
        let stripe = php::unserialize(br#"a:3:{s:7:"enabled";s:3:"yes";s:5:"title";s:11:"Credit card";s:8:"testmode";s:2:"no";}"#).unwrap();
        let key = TEST_KEYS
            .iter()
            .find(|(k, _, _)| stripe.get(k).is_some())
            .unwrap();
        assert_eq!(key.0, "testmode");
        assert!(NOT_GATEWAYS.iter().any(|n| "email_new_order".contains(n)));
    }
}
