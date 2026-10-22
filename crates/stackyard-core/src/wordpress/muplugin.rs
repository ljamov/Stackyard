//! Stackyard's must-use plugin, `wp-content/mu-plugins/stackyard.php`.
//! WordPress loads must-use plugins on every request, before other
//! plugins, and they can't be switched off in wp-admin.
//!
//! It is written fresh from [`Options`] each time, so it always matches
//! the copy it is in. It does only what its options turn on:
//!
//! - one-time login links made by the WordPress tab;
//! - the copy's name in the admin bar, so a staging is never taken for
//!   the live site.

use std::path::{Path, PathBuf};

use super::WpSite;
use crate::{Error, Result};

pub const FILE: &str = "stackyard.php";
/// The option a login link is stored in until it is used.
pub const LOGIN_OPTION: &str = "stackyard_login";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// Shown in the admin bar, e.g. `Local` or `Staging: checkout`.
    pub label: String,
    /// A staging copy: the label is shown in a warning color.
    pub staging: bool,
}

impl Options {
    pub fn for_site(_home: &Path, wp: &WpSite) -> Self {
        let staging = wp.config.get("WP_ENVIRONMENT_TYPE") == Some("staging");
        Options {
            label: if staging {
                "Staging".into()
            } else {
                "Local".into()
            },
            staging,
        }
    }
}

pub fn path(wp: &WpSite) -> PathBuf {
    wp.wp_content().join("mu-plugins").join(FILE)
}

/// Writes the plugin into the site.
pub fn install(wp: &WpSite, options: &Options) -> Result<()> {
    let file = path(wp);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|source| Error::Io {
            path: dir.to_owned(),
            source,
        })?;
    }
    std::fs::write(&file, source(options)).map_err(|source| Error::Io { path: file, source })
}

/// A PHP single-quoted string.
fn php_string(text: &str) -> String {
    super::config::quote(text)
}

pub fn source(options: &Options) -> String {
    let mut out = String::from(
        "<?php\n\
         /**\n\
         \x20* Plugin Name: Stackyard\n\
         \x20* Description: Written by Stackyard for this copy of the site. Stackyard writes it again when it changes the copy, so edits here are lost.\n\
         \x20*/\n\
         \n\
         if ( ! defined( 'ABSPATH' ) ) {\n\
         \texit;\n\
         }\n\n",
    );
    out.push_str(&format!(
        "define( 'STACKYARD_COPY', {} );\n\n",
        php_string(&options.label)
    ));
    out.push_str(LOGIN);
    out.push_str(&ADMIN_BAR.replace(
        "{color}",
        if options.staging {
            "#b45309"
        } else {
            "#2563eb"
        },
    ));
    out
}

/// A login link: `?stackyard-login=<token>` logs in as the user Stackyard
/// stored with the token's hash, once, within its time.
const LOGIN: &str = r#"add_action( 'init', function () {
	if ( empty( $_GET['stackyard-login'] ) ) {
		return;
	}
	$saved = get_option( 'stackyard_login' );
	$token = (string) wp_unslash( $_GET['stackyard-login'] );
	$valid = is_array( $saved ) && ! empty( $saved['hash'] )
		&& hash_equals( (string) $saved['hash'], hash( 'sha256', $token ) );
	if ( ! $valid ) {
		wp_die( 'This login link was already used or has expired. Make a new one in Stackyard.', 'Stackyard', array( 'response' => 403 ) );
	}
	delete_option( 'stackyard_login' );
	if ( time() > (int) $saved['expires'] || ! get_userdata( (int) $saved['user'] ) ) {
		wp_die( 'This login link has expired. Make a new one in Stackyard.', 'Stackyard', array( 'response' => 403 ) );
	}
	wp_set_current_user( (int) $saved['user'] );
	wp_set_auth_cookie( (int) $saved['user'], false );
	wp_safe_redirect( admin_url() );
	exit;
}, 1 );

"#;

const ADMIN_BAR: &str = r#"add_action( 'admin_bar_menu', function ( $bar ) {
	$bar->add_node( array(
		'id'    => 'stackyard-copy',
		'title' => esc_html( STACKYARD_COPY ),
		'meta'  => array( 'title' => 'This is a copy of the site made by Stackyard' ),
	) );
}, 1 );

$stackyard_bar_style = function () {
	echo '<style>#wpadminbar #wp-admin-bar-stackyard-copy > .ab-item{background:{color};color:#fff;font-weight:600}</style>';
};
add_action( 'wp_head', $stackyard_bar_style );
add_action( 'admin_head', $stackyard_bar_style );
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_plugin_with_its_label() {
        let php = source(&Options {
            label: "Staging: it's new".into(),
            staging: true,
        });
        assert!(php.starts_with("<?php\n"));
        assert!(php.contains("define( 'STACKYARD_COPY', 'Staging: it\\'s new' );"));
        assert!(php.contains("#b45309"));
        assert!(php.contains("hash_equals"));
        // Balanced braces, as a quick check the PHP holds together.
        assert_eq!(php.matches('{').count(), php.matches('}').count());
    }
}
