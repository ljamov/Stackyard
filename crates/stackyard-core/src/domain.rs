//! Local domain names such as `myapp.stack`.

use crate::{Error, Result};

/// The domain ending new sites get unless the user picks another one.
pub const DEFAULT_TLD: &str = ".stack";

/// Used if `.stack` ever becomes a real public domain ending.
/// `.test` is reserved for testing by RFC 2606, so it can never clash.
pub const FALLBACK_TLD: &str = ".test";

/// Turns user input like `Stack`, `.stack` or ` .Test ` into `.stack` / `.test`.
pub fn normalize_tld(input: &str) -> Result<String> {
    let trimmed = input.trim().trim_start_matches('.').to_ascii_lowercase();
    let valid = !trimmed.is_empty()
        && trimmed.len() <= 63
        && trimmed
            .split('.')
            .all(|label| is_valid_label(label) && !label.is_empty());
    if !valid {
        return Err(Error::InvalidTld(input.to_string()));
    }
    Ok(format!(".{trimmed}"))
}

/// A warning to show when the chosen ending is known to cause trouble.
pub fn tld_warning(tld: &str) -> Option<&'static str> {
    match tld {
        ".local" => Some(
            ".local is used by Bonjour/mDNS for devices on your network; \
             lookups can be slow on macOS and fail on some Linux setups",
        ),
        ".dev" | ".app" | ".page" => Some(
            "this is a real public domain ending that browsers force to HTTPS; \
             sites may clash with real websites",
        ),
        ".localhost" => Some("some tools treat every .localhost name as 127.0.0.1 already"),
        _ => None,
    }
}

/// Checks a site name such as `myapp` or `my-shop`.
pub fn validate_site_name(name: &str) -> Result<()> {
    if is_valid_label(name) && !name.is_empty() {
        Ok(())
    } else {
        Err(Error::InvalidSiteName(name.to_string()))
    }
}

/// `myapp` + `.stack` -> `myapp.stack`.
pub fn site_domain(name: &str, tld: &str) -> String {
    format!("{name}{tld}")
}

fn is_valid_label(label: &str) -> bool {
    label.len() <= 63
        && label
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !label.starts_with('-')
        && !label.ends_with('-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_endings() {
        assert_eq!(normalize_tld("stack").unwrap(), ".stack");
        assert_eq!(normalize_tld(" .Test ").unwrap(), ".test");
        assert_eq!(normalize_tld("dev.local").unwrap(), ".dev.local");
    }

    #[test]
    fn rejects_bad_endings() {
        for bad in ["", ".", "my site", "-x", "a..b", "ü"] {
            assert!(normalize_tld(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn warns_about_local() {
        assert!(tld_warning(".local").is_some());
        assert!(tld_warning(DEFAULT_TLD).is_none());
        assert!(tld_warning(FALLBACK_TLD).is_none());
    }

    #[test]
    fn validates_site_names() {
        assert!(validate_site_name("my-app2").is_ok());
        for bad in ["", "My", "my app", "-app", "app-", "a.b"] {
            assert!(
                validate_site_name(bad).is_err(),
                "{bad:?} should be rejected"
            );
        }
    }
}
