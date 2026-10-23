//! Stackyard's must-use plugin, `wp-content/mu-plugins/stackyard.php`.
//! WordPress loads must-use plugins on every request, before other
//! plugins, and they can't be switched off in wp-admin.
//!
//! It is written fresh from [`Options`] each time, so it always matches
//! the copy it is in. It does only what its options turn on:
//!
//! - one-time login links made by the WordPress tab;
//! - the copy's name in the admin bar, so a staging is never taken for
//!   the live site;
//! - on copies of a live site ([`Safety`]): mail goes to Mailpit (or
//!   nowhere), search engines are told to stay away, scheduled tasks can't
//!   call outside services, WooCommerce webhooks are held back, and media
//!   that was left on the live site is fetched from it when a page asks.

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
    pub safety: Option<Safety>,
    /// Another copy's uploads folder: media missing here is copied from it
    /// when a page asks (a staging made without its own copy of the media).
    pub media_folder: Option<String>,
}

/// What keeps a copy of a live site from acting as the live site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Safety {
    /// Mailpit's SMTP port; `None` when Mailpit is off, and mail is dropped.
    pub mailpit: Option<u16>,
    /// The live site's address, when media was left there.
    pub media_from: Option<String>,
}

impl Options {
    pub fn for_site(home: &Path, wp: &WpSite) -> Self {
        let env = super::env::load(home, &wp.site.domain()).ok().flatten();
        let staging = match &env {
            Some(env) => env.kind == super::env::Kind::Staging,
            None => wp.config.get("WP_ENVIRONMENT_TYPE") == Some("staging"),
        };
        let media_folder = env
            .as_ref()
            .and_then(|e| e.shared_media.as_ref())
            .map(|p| p.display().to_string());
        let safety = env.filter(|e| e.safe).map(|env| Safety {
            mailpit: mailpit_port(home),
            media_from: env
                .live
                .filter(|l| l.media == super::env::Media::FromLive)
                .map(|l| l.url),
        });
        Options {
            label: if staging {
                format!("Staging: {}", wp.site.name)
            } else {
                "Local".into()
            },
            staging,
            safety,
            media_folder,
        }
    }
}

/// Mailpit's SMTP port, when its module is on.
fn mailpit_port(home: &Path) -> Option<u16> {
    let settings = crate::settings::Settings::load_from(&home.join("settings.toml")).ok()?;
    settings.require_module("mailpit").ok()?;
    let def = crate::catalog::service("mailpit")?;
    Some(crate::services::port(home, def))
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
    if let Some(safety) = &options.safety {
        out.push_str(&match safety.mailpit {
            Some(port) => MAIL_TO_MAILPIT.replace("{port}", &port.to_string()),
            None => MAIL_DROPPED.to_string(),
        });
        out.push_str(SAFETY);
        if let Some(live) = &safety.media_from {
            out.push_str(&format!(
                "define( 'STACKYARD_MEDIA_FROM', {} );\n",
                php_string(live.trim_end_matches('/'))
            ));
        }
    }
    if let Some(folder) = &options.media_folder {
        out.push_str(&format!(
            "define( 'STACKYARD_MEDIA_FOLDER', {} );\n",
            php_string(folder.trim_end_matches(['/', '\\']))
        ));
    }
    if options.media_folder.is_some()
        || options
            .safety
            .as_ref()
            .is_some_and(|s| s.media_from.is_some())
    {
        out.push_str(MEDIA_MISSING);
    }
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

const MAIL_TO_MAILPIT: &str = r#"// Mail goes to Mailpit, never to real people.
add_action( 'phpmailer_init', function ( $mailer ) {
	$mailer->isSMTP();
	$mailer->Host        = '127.0.0.1';
	$mailer->Port        = {port};
	$mailer->SMTPAuth    = false;
	$mailer->SMTPSecure  = '';
	$mailer->SMTPAutoTLS = false;
}, PHP_INT_MAX );

"#;

const MAIL_DROPPED: &str = r#"// Mailpit is off, so mail is dropped rather than sent to real people.
add_filter( 'pre_wp_mail', function () {
	return true;
}, PHP_INT_MAX );

"#;

const SAFETY: &str = r#"// Search engines stay away from this copy.
add_filter( 'pre_option_blog_public', function () {
	return '0';
} );

// Scheduled tasks can't call outside services (payment, CRM, mail lists);
// the copy itself and this computer stay reachable.
add_filter( 'pre_http_request', function ( $pre, $args, $url ) {
	if ( false !== $pre || ! wp_doing_cron() ) {
		return $pre;
	}
	$host = strtolower( (string) wp_parse_url( $url, PHP_URL_HOST ) );
	$own  = strtolower( (string) wp_parse_url( home_url(), PHP_URL_HOST ) );
	if ( in_array( $host, array( $own, 'localhost', '127.0.0.1', '::1', '[::1]' ), true ) ) {
		return $pre;
	}
	return new WP_Error( 'stackyard_blocked', 'Stackyard keeps scheduled tasks on this copy from calling ' . $host . '.' );
}, 1, 3 );

// WooCommerce webhooks would tell outside services about test orders.
add_filter( 'woocommerce_webhook_should_deliver', '__return_false', PHP_INT_MAX );

"#;

/// Media that is not in the copy: a missing file under the uploads folder
/// reaches WordPress (the web server sends missing files to index.php), so
/// it is copied from the folder the copy shares media with, or fetched from
/// the live site, once, kept, and sent.
const MEDIA_MISSING: &str = r#"( function () {
	$path = isset( $_SERVER['REQUEST_URI'] ) ? (string) wp_parse_url( $_SERVER['REQUEST_URI'], PHP_URL_PATH ) : '';
	if ( '' === $path || false === strpos( $path, '/' ) ) {
		return;
	}
	$uploads = wp_upload_dir( null, false );
	$base    = rtrim( (string) wp_parse_url( $uploads['baseurl'], PHP_URL_PATH ), '/' );
	if ( '' === $base || 0 !== strpos( $path, $base . '/' ) ) {
		return;
	}
	$relative = rawurldecode( substr( $path, strlen( $base ) + 1 ) );
	if ( '' === $relative || false !== strpos( $relative, '..' ) || false !== strpos( $relative, "\0" ) ) {
		return;
	}
	$file = $uploads['basedir'] . '/' . $relative;
	if ( is_file( $file ) ) {
		return;
	}
	wp_mkdir_p( dirname( $file ) );
	$part  = $file . '.stackyard-part';
	$found = false;
	if ( defined( 'STACKYARD_MEDIA_FOLDER' ) && is_file( STACKYARD_MEDIA_FOLDER . '/' . $relative ) ) {
		$found = copy( STACKYARD_MEDIA_FOLDER . '/' . $relative, $part );
	}
	if ( ! $found && defined( 'STACKYARD_MEDIA_FROM' ) ) {
		$home     = rtrim( (string) wp_parse_url( home_url(), PHP_URL_PATH ), '/' );
		$url      = STACKYARD_MEDIA_FROM . ( '' !== $home && 0 === strpos( $path, $home . '/' ) ? substr( $path, strlen( $home ) ) : $path );
		$response = wp_remote_get( $url, array( 'timeout' => 60, 'stream' => true, 'filename' => $part ) );
		$found    = ! is_wp_error( $response ) && 200 === wp_remote_retrieve_response_code( $response );
	}
	if ( ! $found ) {
		@unlink( $part );
		status_header( 404 );
		nocache_headers();
		exit;
	}
	rename( $part, $file );
	// wp_check_filetype() needs the user functions, which are not loaded yet.
	$type = 'application/octet-stream';
	$ext  = strtolower( pathinfo( $file, PATHINFO_EXTENSION ) );
	foreach ( wp_get_mime_types() as $exts => $mime ) {
		if ( in_array( $ext, explode( '|', $exts ), true ) ) {
			$type = $mime;
			break;
		}
	}
	header( 'Content-Type: ' . $type );
	header( 'Content-Length: ' . filesize( $file ) );
	status_header( 200 );
	readfile( $file );
	exit;
} )();

"#;

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
            safety: None,
            media_folder: None,
        });
        assert!(php.starts_with("<?php\n"));
        assert!(php.contains("define( 'STACKYARD_COPY', 'Staging: it\\'s new' );"));
        assert!(php.contains("#b45309"));
        assert!(php.contains("hash_equals"));
        // Balanced braces, as a quick check the PHP holds together.
        assert_eq!(php.matches('{').count(), php.matches('}').count());
        assert!(!php.contains("phpmailer_init"));
    }

    #[test]
    fn makes_copies_of_live_sites_safe() {
        let options = Options {
            label: "Local".into(),
            staging: false,
            safety: Some(Safety {
                mailpit: Some(1026),
                media_from: Some("https://shop.example/".into()),
            }),
            media_folder: None,
        };
        let php = source(&options);
        assert!(php.contains("$mailer->Port        = 1026;"));
        assert!(php.contains("pre_option_blog_public"));
        assert!(php.contains("define( 'STACKYARD_MEDIA_FROM', 'https://shop.example' );"));
        assert_eq!(php.matches('{').count(), php.matches('}').count());
        let dropped = source(&Options {
            safety: Some(Safety {
                mailpit: None,
                media_from: None,
            }),
            ..options
        });
        assert!(dropped.contains("pre_wp_mail"));
        assert!(!dropped.contains("STACKYARD_MEDIA_FROM"));
        assert!(!dropped.contains("wp_upload_dir"));
        let shared = source(&Options {
            label: "Staging: checkout".into(),
            staging: true,
            safety: None,
            media_folder: Some("/sites/shop/wp-content/uploads/".into()),
        });
        assert!(shared
            .contains("define( 'STACKYARD_MEDIA_FOLDER', '/sites/shop/wp-content/uploads' );"));
        assert!(shared.contains("wp_upload_dir"));

        let dir = crate::testutil::tempdir();
        let file = dir.join("stackyard.php");
        std::fs::write(&file, php).unwrap();
        if let Ok(out) = std::process::Command::new("php")
            .arg("-l")
            .arg(&file)
            .output()
        {
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stdout)
            );
        }
    }
}
