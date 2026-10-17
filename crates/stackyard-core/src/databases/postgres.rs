//! The Databases screen for PostgreSQL. Like MySQL's side it drives the
//! server's own programs, `psql` and `pg_dump`, so no driver is built in.
//! `psql` prints results as HTML tables (see [`html`]), the one format of
//! its own that marks where each result starts and ends.
//!
//! Backups are gzipped plain SQL from `pg_dump`, named `….pg.sql.gz` so
//! they are never loaded into the wrong server.

use std::ffi::OsString;
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::{html, read_all, Backup, Database, Output, ResultSet, Table, MAX_OUTPUT, MAX_ROWS};
use crate::shell::{Context, Launch, Tool};
use crate::{Error, Result};

/// The database every server has, which tools connect to first.
const MAINTENANCE: &str = "postgres";
/// Never listed, dropped or replaced.
const SYSTEM: [&str; 3] = [MAINTENANCE, "template0", "template1"];

pub(super) struct Pg {
    home: PathBuf,
    psql: Launch,
    /// Marks NULL in the output; random, so no real value can look like it.
    null: String,
}

impl Pg {
    pub fn open(home: &Path) -> Result<Self> {
        let context = Context::load(home)?;
        let psql = context.launch(Tool::Psql, home, Vec::new())?;
        let null = format!("\u{1}null-{:x}", rand_u64());
        Ok(Self {
            home: home.to_owned(),
            psql,
            null,
        })
    }

    pub fn summary(&self) -> &str {
        &self.psql.summary
    }

    pub fn version(&self) -> Option<String> {
        let sets = self.query(None, "SHOW server_version").ok()?;
        let row = sets.into_iter().next()?.rows.into_iter().next()?;
        row.into_iter().next().flatten()
    }

    pub fn databases(&self) -> Result<Vec<Database>> {
        let list = self.single(
            None,
            &format!(
                "SELECT datname, pg_database_size(datname) FROM pg_database \
                 WHERE datallowconn AND datname NOT IN ({}) ORDER BY datname",
                SYSTEM.map(literal).join(", ")
            ),
        )?;
        let mut databases: Vec<Database> = list
            .rows
            .iter()
            .map(|row| Database {
                name: text(row, 0),
                tables: 0,
                size: number(row, 1),
            })
            .collect();
        if databases.is_empty() {
            return Ok(databases);
        }
        // Tables live in each database, so visit each in one psql run.
        let mut script = String::new();
        for db in &databases {
            script.push_str(&format!(
                "\\connect {}\nSELECT current_database(), count(*) FROM pg_class c \
                 JOIN pg_namespace n ON n.oid = c.relnamespace WHERE {};\n",
                meta_quote(&db.name),
                USER_RELATIONS
            ));
        }
        // A database that can't be visited keeps a count of 0.
        if let Ok(output) = self.run(None, script.as_bytes(), false) {
            for set in html::parse(&output.stdout, &self.null) {
                for row in &set.rows {
                    let name = text(row, 0);
                    if let Some(db) = databases.iter_mut().find(|d| d.name == name) {
                        db.tables = number(row, 1);
                    }
                }
            }
        }
        Ok(databases)
    }

    pub fn create(&self, name: &str) -> Result<()> {
        check_name(name)?;
        self.query(None, &format!("CREATE DATABASE {}", ident(name)))
            .map(|_| ())
    }

    pub fn drop(&self, name: &str) -> Result<()> {
        check_user_database(name)?;
        // FORCE closes the connections an app still holds.
        self.query(None, &format!("DROP DATABASE {} WITH (FORCE)", ident(name)))
            .map(|_| ())
    }

    pub fn tables(&self, database: &str) -> Result<Vec<Table>> {
        let set = self.single(
            Some(database),
            &format!(
                "SELECT CASE WHEN n.nspname = 'public' THEN c.relname \
                 ELSE n.nspname || '.' || c.relname END, \
                 GREATEST(c.reltuples, 0)::bigint, pg_total_relation_size(c.oid), \
                 CASE WHEN c.relkind IN ('v', 'm') THEN 'VIEW' ELSE 'BASE TABLE' END \
                 FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
                 WHERE {USER_RELATIONS} ORDER BY 1"
            ),
        )?;
        Ok(set
            .rows
            .iter()
            .map(|row| Table {
                name: text(row, 0),
                rows: number(row, 1),
                size: number(row, 2),
                kind: text(row, 3),
            })
            .collect())
    }

    /// `table` is `name` in the public schema, else `schema.name`, as
    /// [`Pg::tables`] lists them.
    pub fn browse(
        &self,
        database: &str,
        table: &str,
        offset: u64,
        limit: usize,
    ) -> Result<ResultSet> {
        let limit = limit.clamp(1, MAX_ROWS);
        let name = match table.split_once('.') {
            Some((schema, name)) => format!("{}.{}", ident(schema), ident(name)),
            None => ident(table),
        };
        self.single(
            Some(database),
            &format!("SELECT * FROM {name} LIMIT {limit} OFFSET {offset}"),
        )
    }

    pub fn query(&self, database: Option<&str>, sql: &str) -> Result<Vec<ResultSet>> {
        let output = self.run(database, sql.as_bytes(), true)?;
        let mut sets = html::parse(&output.stdout, &self.null);
        for set in &mut sets {
            if set.rows.len() > MAX_ROWS {
                set.rows.truncate(MAX_ROWS);
                set.truncated = true;
            }
        }
        if output.cut_off {
            if let Some(last) = sets.last_mut() {
                last.truncated = true;
            }
        }
        Ok(sets)
    }

    fn single(&self, database: Option<&str>, sql: &str) -> Result<ResultSet> {
        Ok(self
            .query(database, sql)?
            .into_iter()
            .next()
            .unwrap_or_default())
    }

    pub fn backup(&self, database: &str) -> Result<Backup> {
        check_user_database(database)?;
        let context = Context::load(&self.home)?;
        let dump = context.launch(Tool::PgDump, &self.home, Vec::new())?;
        let mut command = command(&dump);
        command.args([
            "--no-password",
            // Restoring replaces what the backup has and keeps the rest.
            "--clean",
            "--if-exists",
            // The backup works in any database and on any server.
            "--no-owner",
            "--no-privileges",
            "--dbname",
            database,
        ]);
        super::write_backup(
            &self.home,
            database,
            &format!("{database}-{}.pg.sql.gz", super::stamp()),
            command,
            &dump.program,
            pg_error,
        )
    }

    pub fn restore(&self, database: &str, file: &str, into: Option<&str>) -> Result<()> {
        let path = super::backup_path(&self.home, database, file)?;
        if !is_backup(file) {
            return Err(Error::Database(format!(
                "{file} is a MySQL or MariaDB backup; restore it there"
            )));
        }
        let target = into.unwrap_or(database);
        check_user_database(target)?;
        check_name(target)?;
        let exists = self.single(
            None,
            &format!(
                "SELECT 1 FROM pg_database WHERE datname = {}",
                literal(target)
            ),
        )?;
        if exists.rows.is_empty() {
            self.create(target)?;
        }
        let source = File::open(&path).map_err(|source| Error::Io {
            path: path.clone(),
            source,
        })?;
        let mut sql = flate2::read::GzDecoder::new(BufReader::new(source));
        let mut child = self
            .psql_command(Some(target))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| Error::Io {
                path: self.psql.program.clone(),
                source,
            })?;
        let stderr = child.stderr.take().expect("piped");
        let errors = std::thread::spawn(move || read_all(stderr));
        let mut stdin = child.stdin.take().expect("piped");
        let fed = std::io::copy(&mut sql, &mut stdin);
        drop(stdin);
        let status = child.wait().map_err(|source| Error::Io {
            path: self.psql.program.clone(),
            source,
        })?;
        let errors = errors.join().unwrap_or_default();
        if !status.success() {
            return Err(pg_error(&errors));
        }
        fed.map_err(|source| Error::Io { path, source })?;
        Ok(())
    }

    fn psql_command(&self, database: Option<&str>) -> Command {
        let mut command = command(&self.psql);
        command.args([
            "--no-psqlrc",
            "--quiet",
            "--no-password",
            "--set=ON_ERROR_STOP=1",
            "--dbname",
            database.unwrap_or(MAINTENANCE),
        ]);
        command
    }

    /// Runs `input` through psql and reads its output, up to [`MAX_OUTPUT`].
    /// `stop` ends the run at the first error, which is then reported.
    fn run(&self, database: Option<&str>, input: &[u8], stop: bool) -> Result<Output> {
        let mut command = self.psql_command(database);
        if !stop {
            command.arg("--set=ON_ERROR_STOP=0");
        }
        let mut null = OsString::from("--pset=null=");
        null.push(&self.null);
        let mut child = command
            .args(["--html", "--pset=footer=off"])
            .arg(null)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| Error::Io {
                path: self.psql.program.clone(),
                source,
            })?;
        let stderr = child.stderr.take().expect("piped");
        let errors = std::thread::spawn(move || read_all(stderr));
        let mut stdin = child.stdin.take().expect("piped");
        let input = input.to_vec();
        let feeder = std::thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
        let mut stdout = child.stdout.take().expect("piped");
        let mut out = Vec::new();
        let mut buf = [0u8; 64 * 1024];
        let mut cut_off = false;
        loop {
            match stdout.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    out.extend_from_slice(&buf[..n]);
                    if out.len() > MAX_OUTPUT {
                        cut_off = true;
                        let _ = child.kill();
                        break;
                    }
                }
            }
        }
        let status = child.wait().map_err(|source| Error::Io {
            path: self.psql.program.clone(),
            source,
        })?;
        let _ = feeder.join();
        let errors = errors.join().unwrap_or_default();
        if !status.success() && !cut_off {
            return Err(pg_error(&errors));
        }
        Ok(Output {
            stdout: out,
            cut_off,
        })
    }
}

/// Tables and views the user made, not the server's own.
const USER_RELATIONS: &str = "c.relkind IN ('r', 'p', 'v', 'm') \
    AND n.nspname NOT IN ('pg_catalog', 'information_schema') \
    AND n.nspname NOT LIKE 'pg\\_toast%' AND n.nspname NOT LIKE 'pg\\_temp%'";

/// Whether a backup file was made by [`Pg::backup`].
pub fn is_backup(file: &str) -> bool {
    file.ends_with(".pg.sql.gz")
}

fn command(launch: &Launch) -> Command {
    let mut command = Command::new(&launch.program);
    command.envs(launch.env.iter().map(|(k, v)| (k, v)));
    // The server's own messages in English, so they can be explained.
    command
        .env("PGCLIENTENCODING", "UTF8")
        .env("LC_MESSAGES", "C");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// What psql or pg_dump said went wrong, in plain words where we know them.
pub(super) fn pg_error(stderr: &str) -> Error {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let all = lines.join(" ");
    if all.contains("Connection refused")
        || all.contains("could not connect")
        || all.contains("No connection could be made")
        || all.contains("the database system is starting up")
    {
        return Error::Database(
            "PostgreSQL is not running. Start it on the Services screen or with `stackyard start postgres`."
                .into(),
        );
    }
    // `psql:<stdin>:1: ERROR:  syntax error at or near "SELEC"` → `syntax error at or near "SELEC"`.
    let line = lines
        .iter()
        .find(|l| l.contains("ERROR:") || l.contains("FATAL:"))
        .or(lines.first())
        .copied()
        .unwrap_or("the database client failed");
    let message = ["ERROR:", "FATAL:", "error:"]
        .iter()
        .find_map(|tag| line.split_once(tag).map(|(_, rest)| rest.trim()))
        .unwrap_or(line);
    Error::Database(message.to_string())
}

/// Names for new databases: letters, digits, `_` and `-`, up to 63
/// (PostgreSQL cuts longer names short).
fn check_name(name: &str) -> Result<()> {
    if name.len() > 63 {
        return Err(Error::Database(
            "use letters, digits, _ and - for database names (up to 63)".into(),
        ));
    }
    super::check_name(name)
}

fn check_user_database(name: &str) -> Result<()> {
    if SYSTEM.contains(&name) {
        return Err(Error::Database(format!(
            "{name} belongs to the database server itself"
        )));
    }
    if name.is_empty() {
        return Err(Error::Database("choose a database".into()));
    }
    Ok(())
}

/// A name quoted for SQL: `"my""table"`.
pub fn ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// A string quoted for SQL; PostgreSQL's strings take no backslash escapes.
fn literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// A name quoted for a psql command such as `\connect`.
fn meta_quote(text: &str) -> String {
    format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn text(row: &[Option<String>], i: usize) -> String {
    row.get(i).cloned().flatten().unwrap_or_default()
}

fn number(row: &[Option<String>], i: usize) -> u64 {
    row.get(i)
        .and_then(|v| v.as_deref())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// Enough randomness for a marker: the clock mixed with this process.
fn rand_u64() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    );
    hasher.write_u32(std::process::id());
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_names_and_strings() {
        assert_eq!(ident("shop"), "\"shop\"");
        assert_eq!(ident("we\"ird"), "\"we\"\"ird\"");
        assert_eq!(literal(r"it's a\b"), r"'it''s a\b'");
        assert_eq!(meta_quote("it's"), r"'it\'s'");
        assert!(check_name(&"x".repeat(63)).is_ok());
        assert!(check_name(&"x".repeat(64)).is_err());
        assert!(check_user_database("postgres").is_err());
        assert!(check_user_database("template1").is_err());
        assert!(check_user_database("shop").is_ok());
        assert!(is_backup("shop-20261017-101010.pg.sql.gz"));
        assert!(!is_backup("shop-20261017-101010.sql.gz"));
    }

    #[test]
    fn explains_client_errors() {
        let e = |s: &str| pg_error(s).to_string();
        assert_eq!(
            e("psql:<stdin>:1: ERROR:  syntax error at or near \"SELEC\"\nLINE 1: SELEC 1\n        ^\n"),
            "syntax error at or near \"SELEC\""
        );
        assert!(e("psql: error: connection to server at \"127.0.0.1\", port 5432 failed: Connection refused\n\tIs the server running on that host and accepting TCP/IP connections?")
            .starts_with("PostgreSQL is not running"));
        assert_eq!(
            e("psql: error: connection to server at \"127.0.0.1\", port 5432 failed: FATAL:  database \"nope\" does not exist"),
            "database \"nope\" does not exist"
        );
    }
}
