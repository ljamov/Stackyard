//! The one-time PHP helper that reads a live site's database when the
//! server has no `mysqldump` Stackyard can run, e.g. on hosting with FTP only.
//!
//! Stackyard writes it for one clone or push, with the database settings
//! from the live `wp-config.php` and the hash of a random key in it, and
//! uploads it into the site's folder. It works from the command line (over
//! SSH) and over HTTPS, where it answers only POST requests carrying the
//! key, and it deletes itself when Stackyard is done or, at the latest,
//! when its time is up, even if the transfer stopped halfway.
//!
//! It only reads: it lists the tables and exports them in chunks, so a
//! large table goes over in many small requests and a broken connection
//! costs one chunk, not the whole table.

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::config::quote;

/// How long a helper works before it deletes itself.
pub const LIFETIME_SECS: u64 = 30 * 60;

/// The database settings the helper connects with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbSettings {
    pub host: String,
    pub user: String,
    pub password: String,
    pub name: String,
    pub charset: String,
    pub prefix: String,
}

/// A helper made for one transfer.
#[derive(Debug, Clone)]
pub struct Helper {
    /// The file name it is uploaded as, e.g. `stackyard-k3j9x2.php`.
    pub file: String,
    /// The key Stackyard sends; only its hash is in the file.
    pub key: String,
    pub source: String,
}

/// Writes a helper that works until `until` (seconds since 1970).
pub fn make(db: &DbSettings, until: u64) -> crate::Result<Helper> {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes)
        .map_err(|e| crate::Error::WordPress(format!("no randomness: {e}")))?;
    let key: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let file = format!("stackyard-{}.php", &key[..12]);
    let hash: String = Sha256::digest(key.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let source = SOURCE
        .replace("{KEY_HASH}", &hash)
        .replace("{UNTIL}", &until.to_string())
        .replace("{HOST}", &quote(&db.host))
        .replace("{USER}", &quote(&db.user))
        .replace("{PASSWORD}", &quote(&db.password))
        .replace("{NAME}", &quote(&db.name))
        .replace("{CHARSET}", &quote(&db.charset))
        .replace("{PREFIX}", &quote(&db.prefix));
    Ok(Helper { file, key, source })
}

/// What `do=info` answers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Info {
    pub php: String,
    pub mysql: String,
    #[serde(default)]
    pub wordpress: String,
    #[serde(default)]
    pub siteurl: String,
    #[serde(default)]
    pub home: String,
    /// The folder the helper is in: WordPress's own folder on the server.
    #[serde(default)]
    pub abspath: String,
    pub tables: Vec<Table>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Table {
    pub name: String,
    /// An estimate, as MySQL keeps it.
    pub rows: u64,
    pub bytes: u64,
}

/// What a `do=table` chunk says about itself.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Chunk {
    pub rows: u64,
    /// Where the next chunk starts; pass it back as `from`.
    pub next: String,
    /// The table is complete with this chunk.
    pub done: bool,
    /// The chunk is gzip-compressed.
    pub gz: bool,
}

/// The line before the chunk's metadata in the command line's error output.
pub const META_MARK: &str = "STACKYARD-META ";

/// Finds the metadata in what the helper wrote to its error output.
pub fn meta_from_errors(errors: &str) -> Option<Chunk> {
    errors
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix(META_MARK))
        .and_then(|json| serde_json::from_str(json).ok())
}

const SOURCE: &str = r#"<?php
/*
 * Stackyard helper. Stackyard put this file here to copy this site's
 * database to a computer. It answers only requests that carry its key,
 * only reads, and deletes itself when Stackyard is done or when its time
 * is up. It is safe to delete by hand.
 */
define( 'SY_KEY_HASH', '{KEY_HASH}' );
define( 'SY_UNTIL', {UNTIL} );
define( 'SY_PREFIX', {PREFIX} );
define( 'SY_CHUNK_BYTES', 8 * 1024 * 1024 );
define( 'SY_CHUNK_ROWS', 20000 );
define( 'SY_STATEMENT_BYTES', 1024 * 1024 );
$sy_db = array(
	'host'     => {HOST},
	'user'     => {USER},
	'password' => {PASSWORD},
	'name'     => {NAME},
	'charset'  => {CHARSET},
);

error_reporting( 0 );
@ini_set( 'display_errors', '0' );
@set_time_limit( 300 );

function sy_cli() {
	return PHP_SAPI === 'cli';
}

function sy_fail( $status, $message ) {
	if ( sy_cli() ) {
		fwrite( STDERR, $message . "\n" );
		exit( 1 );
	}
	http_response_code( $status );
	header( 'Content-Type: application/json' );
	header( 'Cache-Control: no-store' );
	echo json_encode( array( 'error' => $message ) );
	exit;
}

function sy_meta( $meta ) {
	if ( sy_cli() ) {
		fwrite( STDERR, "\nSTACKYARD-META " . json_encode( $meta ) . "\n" );
	} else {
		header( 'X-Stackyard-Meta: ' . json_encode( $meta ) );
	}
}

function sy_json( $data ) {
	if ( ! sy_cli() ) {
		header( 'Content-Type: application/json' );
		header( 'Cache-Control: no-store' );
	}
	echo json_encode( $data );
	exit;
}

if ( time() > SY_UNTIL ) {
	@unlink( __FILE__ );
	sy_fail( 410, 'This Stackyard helper has expired and removed itself.' );
}

if ( sy_cli() ) {
	$sy_args = array();
	foreach ( array_slice( $argv, 1 ) as $sy_arg ) {
		$sy_at = strpos( $sy_arg, '=' );
		if ( false !== $sy_at ) {
			$sy_args[ substr( $sy_arg, 0, $sy_at ) ] = substr( $sy_arg, $sy_at + 1 );
		}
	}
} else {
	$sy_key = '';
	if ( isset( $_SERVER['HTTP_X_STACKYARD_KEY'] ) ) {
		$sy_key = (string) $_SERVER['HTTP_X_STACKYARD_KEY'];
	} elseif ( isset( $_POST['key'] ) ) {
		$sy_key = (string) $_POST['key'];
	}
	if ( 'POST' !== $_SERVER['REQUEST_METHOD'] || '' === $sy_key || ! hash_equals( SY_KEY_HASH, hash( 'sha256', $sy_key ) ) ) {
		sy_fail( 404, 'Not found' );
	}
	$sy_args = $_POST;
}

$sy_do = isset( $sy_args['do'] ) ? (string) $sy_args['do'] : '';

if ( 'finish' === $sy_do ) {
	sy_json( array( 'removed' => @unlink( __FILE__ ) ) );
}
if ( 'ping' === $sy_do ) {
	sy_json( array( 'php' => PHP_VERSION, 'abspath' => __DIR__ ) );
}

if ( ! class_exists( 'mysqli' ) ) {
	sy_fail( 500, 'PHP on this server has no mysqli extension.' );
}

function sy_connect( $db ) {
	$host   = $db['host'];
	$port   = null;
	$socket = null;
	if ( 0 === strpos( $host, 'p:' ) ) {
		$host = substr( $host, 2 );
	}
	if ( preg_match( '/^(.*):(\d+)$/', $host, $m ) ) {
		$host = $m[1];
		$port = (int) $m[2];
	} elseif ( false !== strpos( $host, ':/' ) ) {
		$socket = substr( $host, strpos( $host, ':' ) + 1 );
		$host   = substr( $host, 0, strpos( $host, ':' ) );
	}
	if ( '' === $host ) {
		$host = 'localhost';
	}
	mysqli_report( MYSQLI_REPORT_OFF );
	$link = mysqli_init();
	if ( ! @$link->real_connect( $host, $db['user'], $db['password'], $db['name'], $port, $socket ) ) {
		sy_fail( 500, 'Could not connect to the database: ' . mysqli_connect_error() );
	}
	$charset = '' !== $db['charset'] ? $db['charset'] : 'utf8mb4';
	if ( ! $link->set_charset( $charset ) ) {
		$link->set_charset( 'utf8' );
	}
	return $link;
}

function sy_tables( $link ) {
	$tables = array();
	$result = $link->query( 'SHOW TABLE STATUS' );
	if ( ! $result ) {
		sy_fail( 500, 'Could not list the tables: ' . $link->error );
	}
	while ( $row = $result->fetch_assoc() ) {
		// Views have no engine; only the site's own tables are copied.
		if ( null === $row['Engine'] || 0 !== strpos( $row['Name'], SY_PREFIX ) ) {
			continue;
		}
		$tables[ $row['Name'] ] = array(
			'name'  => $row['Name'],
			'rows'  => (int) $row['Rows'],
			'bytes' => (int) $row['Data_length'] + (int) $row['Index_length'],
		);
	}
	return $tables;
}

function sy_name( $name ) {
	return '`' . str_replace( '`', '``', $name ) . '`';
}

$sy_link = sy_connect( $sy_db );

if ( 'info' === $sy_do ) {
	$sy_options = array();
	$sy_result  = $sy_link->query( 'SELECT option_name, option_value FROM ' . sy_name( SY_PREFIX . 'options' ) . " WHERE option_name IN ('siteurl', 'home')" );
	if ( $sy_result ) {
		while ( $sy_row = $sy_result->fetch_row() ) {
			$sy_options[ $sy_row[0] ] = $sy_row[1];
		}
	}
	$wp_version = '';
	if ( is_file( __DIR__ . '/wp-includes/version.php' ) ) {
		include __DIR__ . '/wp-includes/version.php';
	}
	sy_json(
		array(
			'php'       => PHP_VERSION,
			'mysql'     => $sy_link->server_info,
			'wordpress' => $wp_version,
			'siteurl'   => isset( $sy_options['siteurl'] ) ? $sy_options['siteurl'] : '',
			'home'      => isset( $sy_options['home'] ) ? $sy_options['home'] : '',
			'abspath'   => __DIR__,
			'tables'    => array_values( sy_tables( $sy_link ) ),
		)
	);
}

if ( 'table' !== $sy_do ) {
	sy_fail( 400, 'Unknown request.' );
}

$sy_table = isset( $sy_args['table'] ) ? (string) $sy_args['table'] : '';
if ( ! array_key_exists( $sy_table, sy_tables( $sy_link ) ) ) {
	sy_fail( 404, 'No such table.' );
}
$sy_from = isset( $sy_args['from'] ) ? (string) $sy_args['from'] : '';
$sy_q    = sy_name( $sy_table );
$sy_out  = '';

if ( '' === $sy_from ) {
	$sy_create = $sy_link->query( "SHOW CREATE TABLE $sy_q" );
	$sy_create = $sy_create ? $sy_create->fetch_row() : null;
	if ( ! $sy_create ) {
		sy_fail( 500, 'Could not read the table: ' . $sy_link->error );
	}
	$sy_out .= "DROP TABLE IF EXISTS $sy_q;\n" . $sy_create[1] . ";\n";
}

$sy_columns = array();
$sy_binary  = array();
$sy_keys    = array();
$sy_key_int = false;
$sy_result  = $sy_link->query( "SHOW COLUMNS FROM $sy_q" );
while ( $sy_result && $sy_column = $sy_result->fetch_assoc() ) {
	$sy_type      = strtolower( $sy_column['Type'] );
	$sy_columns[] = sy_name( $sy_column['Field'] );
	$sy_binary[]  = (bool) preg_match( '/blob|binary|^bit|geometry|point|linestring|polygon/', $sy_type );
	if ( 'PRI' === $sy_column['Key'] ) {
		$sy_keys[]  = $sy_column['Field'];
		$sy_key_int = (bool) preg_match( '/int/', $sy_type );
	}
}
// One whole-number key: continue after the last one sent, which stays fast
// on big tables. Otherwise count rows in, in key order where there is one.
$sy_keyset = 1 === count( $sy_keys ) && $sy_key_int;
$sy_key_at = $sy_keyset ? array_search( sy_name( $sy_keys[0] ), $sy_columns, true ) : -1;
$sy_offset = 0;
if ( $sy_keyset ) {
	$sy_sql = "SELECT * FROM $sy_q";
	if ( 0 === strpos( $sy_from, 'k:' ) && preg_match( '/^-?\d+$/', substr( $sy_from, 2 ) ) ) {
		$sy_sql .= ' WHERE ' . sy_name( $sy_keys[0] ) . ' > ' . substr( $sy_from, 2 );
	}
	$sy_sql .= ' ORDER BY ' . sy_name( $sy_keys[0] ) . ' LIMIT ' . SY_CHUNK_ROWS;
} else {
	if ( 0 === strpos( $sy_from, 'o:' ) ) {
		$sy_offset = max( 0, (int) substr( $sy_from, 2 ) );
	}
	$sy_order = $sy_keys ? ' ORDER BY ' . implode( ', ', array_map( 'sy_name', $sy_keys ) ) : '';
	$sy_sql   = "SELECT * FROM $sy_q$sy_order LIMIT $sy_offset, " . SY_CHUNK_ROWS;
}

$sy_result = $sy_link->query( $sy_sql, MYSQLI_USE_RESULT );
if ( ! $sy_result ) {
	sy_fail( 500, 'Could not read the table: ' . $sy_link->error );
}
$sy_head   = "INSERT INTO $sy_q (" . implode( ', ', $sy_columns ) . ") VALUES\n";
$sy_values = array();
$sy_size   = 0;
$sy_rows   = 0;
$sy_last   = null;
$sy_full   = false;
while ( $sy_row = $sy_result->fetch_row() ) {
	$sy_cells = array();
	foreach ( $sy_row as $sy_i => $sy_value ) {
		if ( null === $sy_value ) {
			$sy_cells[] = 'NULL';
		} elseif ( $sy_binary[ $sy_i ] ) {
			$sy_cells[] = '' === $sy_value ? "''" : '0x' . bin2hex( $sy_value );
		} else {
			$sy_cells[] = "'" . $sy_link->real_escape_string( $sy_value ) . "'";
		}
	}
	$sy_line     = '(' . implode( ',', $sy_cells ) . ')';
	$sy_values[] = $sy_line;
	$sy_size    += strlen( $sy_line );
	++$sy_rows;
	if ( $sy_keyset ) {
		$sy_last = $sy_row[ $sy_key_at ];
	}
	if ( $sy_size >= SY_STATEMENT_BYTES ) {
		$sy_out   .= $sy_head . implode( ",\n", $sy_values ) . ";\n";
		$sy_values = array();
		$sy_size   = 0;
		if ( strlen( $sy_out ) >= SY_CHUNK_BYTES ) {
			$sy_full = true;
			break;
		}
	}
}
$sy_result->free();
if ( $sy_values ) {
	$sy_out .= $sy_head . implode( ",\n", $sy_values ) . ";\n";
}

$sy_done = ! $sy_full && $sy_rows < SY_CHUNK_ROWS;
if ( $sy_keyset ) {
	$sy_next = null === $sy_last ? $sy_from : 'k:' . $sy_last;
} else {
	$sy_next = 'o:' . ( $sy_offset + $sy_rows );
}
$sy_gz = function_exists( 'gzencode' );
sy_meta(
	array(
		'rows' => $sy_rows,
		'next' => $sy_next,
		'done' => $sy_done,
		'gz'   => $sy_gz,
	)
);
if ( ! sy_cli() ) {
	header( 'Content-Type: application/octet-stream' );
	header( 'Cache-Control: no-store' );
}
echo $sy_gz ? gzencode( $sy_out, 6 ) : $sy_out;
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> DbSettings {
        DbSettings {
            host: "localhost:/var/run/mysqld/mysqld.sock".into(),
            user: "shop".into(),
            password: "it's secret".into(),
            name: "shop_db".into(),
            charset: "utf8mb4".into(),
            prefix: "wp_".into(),
        }
    }

    #[test]
    fn writes_settings_and_only_the_key_hash() {
        let helper = make(&db(), 1_800_000_000).unwrap();
        assert!(helper.file.starts_with("stackyard-") && helper.file.ends_with(".php"));
        assert_eq!(helper.key.len(), 48);
        assert!(!helper.source.contains(&helper.key));
        assert!(helper.source.contains("define( 'SY_UNTIL', 1800000000 );"));
        assert!(helper.source.contains(r"'password' => 'it\'s secret',"));
        assert!(helper.source.contains("define( 'SY_PREFIX', 'wp_' );"));
        assert!(!helper.source.contains("{HOST}"));
        let other = make(&db(), 1_800_000_000).unwrap();
        assert_ne!(helper.key, other.key);
    }

    #[test]
    fn reads_chunk_metadata_from_error_output() {
        let errors = "PHP Warning: something\n\nSTACKYARD-META {\"rows\":3,\"next\":\"k:9\",\"done\":true,\"gz\":true}\n";
        let chunk = meta_from_errors(errors).unwrap();
        assert_eq!(chunk.rows, 3);
        assert_eq!(chunk.next, "k:9");
        assert!(chunk.done && chunk.gz);
        assert_eq!(meta_from_errors("nothing"), None);
    }

    /// Runs the helper with the PHP on this computer, when there is one,
    /// to catch syntax errors.
    #[test]
    fn is_valid_php() {
        let helper = make(&db(), 1_800_000_000).unwrap();
        let dir = crate::testutil::tempdir();
        let file = dir.join(&helper.file);
        std::fs::write(&file, &helper.source).unwrap();
        let Ok(out) = std::process::Command::new("php")
            .arg("-l")
            .arg(&file)
            .output()
        else {
            return;
        };
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
    }
}
