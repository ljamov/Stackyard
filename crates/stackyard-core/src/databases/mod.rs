//! The Databases screen and `stackyard db`: list, create and drop
//! databases, browse tables, run queries, and back up and restore. It
//! drives the `mysql` and `mysqldump` programs that come with MySQL and
//! MariaDB, and `psql` and `pg_dump` for PostgreSQL ([`postgres`]), so no
//! database driver is built in.
//!
//! Backups are gzipped SQL dumps in `backups/<database>/` inside
//! Stackyard's data folder.

mod html;
pub mod postgres;
mod xml;

use std::ffi::OsString;
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::UNIX_EPOCH;

use serde::Serialize;

use crate::shell::{Context, Launch, Tool};
use crate::{Error, Result};

pub use xml::ResultSet;

/// Databases the server keeps for itself; never listed, dropped or replaced.
const SYSTEM: [&str; 4] = ["mysql", "information_schema", "performance_schema", "sys"];
/// Rows a query or a page of a table shows at most.
pub const MAX_ROWS: usize = 1000;
/// Output read from one query at most; the rest is cut off.
const MAX_OUTPUT: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Server {
    /// `mysql`, `mariadb` or `postgres`.
    pub id: String,
    /// e.g. `MariaDB 11.8`.
    pub name: String,
    /// What the server says, e.g. `11.8.3-MariaDB`; `None` when it is not running.
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Database {
    pub name: String,
    pub tables: u64,
    /// Data and indexes, in bytes.
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Table {
    pub name: String,
    /// The server's estimate for InnoDB tables.
    pub rows: u64,
    pub size: u64,
    /// `BASE TABLE` or `VIEW`.
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Backup {
    /// `postgres` for PostgreSQL backups, else `mysql` (MySQL or MariaDB).
    pub server: &'static str,
    pub database: String,
    /// The file name inside `backups/<database>/`.
    pub file: String,
    pub size: u64,
    /// Milliseconds since 1970.
    pub created: u64,
}

pub struct Client {
    home: PathBuf,
    mysql: Launch,
    server: String,
    pg: Option<postgres::Pg>,
}

/// The servers the screen can show: `mysql` when MySQL or MariaDB is
/// installed and switched on, `postgres` when PostgreSQL is.
pub fn servers(home: &Path) -> Vec<&'static str> {
    let Ok(settings) = crate::settings::Settings::load_from(&home.join("settings.toml")) else {
        return Vec::new();
    };
    let services = crate::services::ServiceManager::new(home);
    let usable = |id: &str| settings.module_enabled(id) && services.get(id).is_some();
    let mut found = Vec::new();
    if usable("mysql") || usable("mariadb") {
        found.push("mysql");
    }
    if usable("postgres") {
        found.push("postgres");
    }
    found
}

impl Client {
    /// The client for `server`: `postgres`, or MySQL and MariaDB for
    /// anything else (see [`Client::open`]).
    pub fn open_server(home: &Path, server: Option<&str>) -> Result<Self> {
        if server != Some("postgres") {
            return Self::open(home);
        }
        let pg = postgres::Pg::open(home)?;
        Ok(Self {
            home: home.to_owned(),
            mysql: Launch {
                program: PathBuf::new(),
                args: Vec::new(),
                path_first: PathBuf::new(),
                env: Vec::new(),
                summary: pg.summary().to_string(),
            },
            server: "postgres".into(),
            pg: Some(pg),
        })
    }

    /// The client for whichever of MySQL and MariaDB is installed and
    /// switched on (the running one when both are).
    pub fn open(home: &Path) -> Result<Self> {
        let context = Context::load(home)?;
        let mysql = context.launch(Tool::Mysql, home, Vec::new())?;
        // `MySQL 8.4` or `MariaDB 11.8`.
        let server = if mysql.summary.starts_with("MariaDB") {
            "mariadb"
        } else {
            "mysql"
        };
        Ok(Self {
            home: home.to_owned(),
            mysql,
            server: server.into(),
            pg: None,
        })
    }

    /// Which server this is, and its version if it is running.
    pub fn server(&self) -> Server {
        if let Some(pg) = &self.pg {
            return Server {
                id: self.server.clone(),
                name: pg.summary().to_string(),
                version: pg.version(),
            };
        }
        let version = self
            .query(None, "SELECT VERSION() AS v")
            .ok()
            .and_then(|sets| sets.into_iter().next())
            .and_then(|set| set.rows.into_iter().next())
            .and_then(|row| row.into_iter().next().flatten());
        Server {
            id: self.server.clone(),
            name: self.mysql.summary.clone(),
            version,
        }
    }

    pub fn databases(&self) -> Result<Vec<Database>> {
        if let Some(pg) = &self.pg {
            return pg.databases();
        }
        let sql = format!(
            "SELECT s.SCHEMA_NAME AS name, COUNT(t.TABLE_NAME) AS tables, \
             COALESCE(SUM(t.DATA_LENGTH + t.INDEX_LENGTH), 0) AS size \
             FROM information_schema.SCHEMATA s \
             LEFT JOIN information_schema.TABLES t ON t.TABLE_SCHEMA = s.SCHEMA_NAME \
             WHERE s.SCHEMA_NAME NOT IN ({}) \
             GROUP BY s.SCHEMA_NAME ORDER BY s.SCHEMA_NAME",
            SYSTEM.map(literal).join(", ")
        );
        let set = self.single(None, &sql)?;
        Ok(set
            .rows
            .into_iter()
            .map(|row| Database {
                name: text(&row, 0),
                tables: number(&row, 1),
                size: number(&row, 2),
            })
            .collect())
    }

    pub fn create(&self, name: &str) -> Result<()> {
        if let Some(pg) = &self.pg {
            return pg.create(name);
        }
        check_name(name)?;
        self.query(
            None,
            &format!("CREATE DATABASE {} CHARACTER SET utf8mb4", ident(name)),
        )
        .map(|_| ())
    }

    /// Drops a database and everything in it.
    pub fn drop(&self, name: &str) -> Result<()> {
        if let Some(pg) = &self.pg {
            return pg.drop(name);
        }
        check_user_database(name)?;
        self.query(None, &format!("DROP DATABASE {}", ident(name)))
            .map(|_| ())
    }

    pub fn tables(&self, database: &str) -> Result<Vec<Table>> {
        if let Some(pg) = &self.pg {
            return pg.tables(database);
        }
        let sql = format!(
            "SELECT TABLE_NAME AS name, COALESCE(TABLE_ROWS, 0) AS `rows`, \
             COALESCE(DATA_LENGTH + INDEX_LENGTH, 0) AS size, TABLE_TYPE AS kind \
             FROM information_schema.TABLES WHERE TABLE_SCHEMA = {} ORDER BY TABLE_NAME",
            literal(database)
        );
        let set = self.single(None, &sql)?;
        Ok(set
            .rows
            .into_iter()
            .map(|row| Table {
                name: text(&row, 0),
                rows: number(&row, 1),
                size: number(&row, 2),
                kind: text(&row, 3),
            })
            .collect())
    }

    /// A page of a table's rows.
    pub fn browse(
        &self,
        database: &str,
        table: &str,
        offset: u64,
        limit: usize,
    ) -> Result<ResultSet> {
        if let Some(pg) = &self.pg {
            return pg.browse(database, table, offset, limit);
        }
        let limit = limit.clamp(1, MAX_ROWS);
        let mut set = self.single(
            Some(database),
            &format!("SELECT * FROM {} LIMIT {offset}, {limit}", ident(table)),
        )?;
        if set.columns.is_empty() {
            // No rows, so the output named no columns; ask for them.
            let columns = self.single(
                None,
                &format!(
                    "SELECT COLUMN_NAME FROM information_schema.COLUMNS \
                     WHERE TABLE_SCHEMA = {} AND TABLE_NAME = {} ORDER BY ORDINAL_POSITION",
                    literal(database),
                    literal(table)
                ),
            )?;
            set.columns = columns.rows.iter().map(|row| text(row, 0)).collect();
        }
        Ok(set)
    }

    /// Runs SQL as typed, in `database` when given. Each statement that
    /// returns rows gives one result, with at most [`MAX_ROWS`] rows.
    pub fn query(&self, database: Option<&str>, sql: &str) -> Result<Vec<ResultSet>> {
        if let Some(pg) = &self.pg {
            return pg.query(database, sql);
        }
        let mut args: Vec<OsString> = vec!["--xml".into()];
        if let Some(db) = database {
            args.push(format!("--database={db}").into());
        }
        let output = self.run(&self.mysql, args, sql.as_bytes())?;
        let mut sets = xml::parse(&output.stdout);
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

    /// Dumps a database to a new gzipped file in its backups folder.
    pub fn backup(&self, database: &str) -> Result<Backup> {
        if let Some(pg) = &self.pg {
            return pg.backup(database);
        }
        check_user_database(database)?;
        let context = Context::load(&self.home)?;
        let dump = context.launch(Tool::Mysqldump, &self.home, Vec::new())?;
        let mut command = client_command(&dump);
        command
            .args([
                "--single-transaction",
                "--routines",
                "--triggers",
                "--events",
                "--hex-blob",
            ])
            .arg(database);
        write_backup(
            &self.home,
            database,
            &format!("{database}-{}.sql.gz", stamp()),
            command,
            &dump.program,
            |errors| client_error(errors, &self.server),
        )
    }

    /// Loads a backup into `into` (its own database when `None`), creating
    /// that database if needed. Tables in the backup replace tables of the
    /// same name; others are left alone.
    pub fn restore(&self, database: &str, file: &str, into: Option<&str>) -> Result<()> {
        if let Some(pg) = &self.pg {
            return pg.restore(database, file, into);
        }
        let path = backup_path(&self.home, database, file)?;
        if postgres::is_backup(file) {
            return Err(Error::Database(format!(
                "{file} is a PostgreSQL backup; restore it there"
            )));
        }
        let target = into.unwrap_or(database);
        check_user_database(target)?;
        check_name(target)?;
        self.query(
            None,
            &format!(
                "CREATE DATABASE IF NOT EXISTS {} CHARACTER SET utf8mb4",
                ident(target)
            ),
        )?;
        let source = File::open(&path).map_err(|source| Error::Io {
            path: path.clone(),
            source,
        })?;
        let mut sql = flate2::read::GzDecoder::new(BufReader::new(source));
        let mut child = client_command(&self.mysql)
            .arg("--connect-timeout=5")
            .arg(format!("--database={target}"))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| Error::Io {
                path: self.mysql.program.clone(),
                source,
            })?;
        let stderr = child.stderr.take().expect("piped");
        let errors = std::thread::spawn(move || read_all(stderr));
        let mut stdin = child.stdin.take().expect("piped");
        let fed = std::io::copy(&mut sql, &mut stdin);
        drop(stdin);
        let status = child.wait().map_err(|source| Error::Io {
            path: self.mysql.program.clone(),
            source,
        })?;
        let errors = errors.join().unwrap_or_default();
        if !status.success() {
            return Err(client_error(&errors, &self.server));
        }
        fed.map_err(|source| Error::Io { path, source })?;
        Ok(())
    }

    /// Runs a client program with `input` on stdin and reads its output,
    /// up to [`MAX_OUTPUT`].
    fn run(&self, launch: &Launch, args: Vec<OsString>, input: &[u8]) -> Result<Output> {
        let mut child = client_command(launch)
            .arg("--connect-timeout=5")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| Error::Io {
                path: launch.program.clone(),
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
            path: launch.program.clone(),
            source,
        })?;
        let _ = feeder.join();
        let errors = errors.join().unwrap_or_default();
        if !status.success() && !cut_off {
            return Err(client_error(&errors, &self.server));
        }
        Ok(Output {
            stdout: out,
            cut_off,
        })
    }
}

struct Output {
    stdout: Vec<u8>,
    cut_off: bool,
}

/// Runs a dump program and gzips what it prints into `backups/<database>/<file>`,
/// which appears only once the dump is complete.
fn write_backup(
    home: &Path,
    database: &str,
    file: &str,
    mut command: Command,
    program: &Path,
    explain: impl FnOnce(&str) -> Error,
) -> Result<Backup> {
    let dir = backups_dir(home).join(database);
    let io = |path: &Path| {
        let path = path.to_owned();
        move |source| Error::Io { path, source }
    };
    std::fs::create_dir_all(&dir).map_err(io(&dir))?;
    let path = dir.join(file);
    let partial = dir.join(format!(".{file}.partial"));

    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(io(program))?;
    let mut stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");
    let errors = std::thread::spawn(move || read_all(stderr));
    let written = (|| -> std::io::Result<()> {
        let out = File::create(&partial)?;
        let mut gz = flate2::write::GzEncoder::new(out, flate2::Compression::default());
        std::io::copy(&mut stdout, &mut gz)?;
        gz.finish()?.sync_all()
    })();
    let status = child.wait().map_err(io(program))?;
    let errors = errors.join().unwrap_or_default();
    if let Err(source) = written {
        let _ = std::fs::remove_file(&partial);
        return Err(Error::Io {
            path: partial,
            source,
        });
    }
    if !status.success() {
        let _ = std::fs::remove_file(&partial);
        return Err(explain(&errors));
    }
    std::fs::rename(&partial, &path).map_err(io(&path))?;
    backup_info(database, &path).ok_or_else(|| Error::Database("the backup file vanished".into()))
}

/// The client with its connection settings: root with no password, this
/// installation's server only, and no option files from elsewhere.
fn client_command(launch: &Launch) -> Command {
    let mut command = Command::new(&launch.program);
    command
        .arg("--no-defaults")
        .args(["--user=root", "--default-character-set=utf8mb4"]);
    if let Some((_, port)) = launch.env.iter().find(|(k, _)| k == "MYSQL_TCP_PORT") {
        if cfg!(windows) {
            let mut arg = OsString::from("--port=");
            arg.push(port);
            command
                .args(["--protocol=TCP", "--host=127.0.0.1"])
                .arg(arg);
        }
    }
    command.envs(launch.env.iter().map(|(k, v)| (k, v)));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// What the client said went wrong, in plain words where we know them.
fn client_error(stderr: &str, server: &str) -> Error {
    let name = if server == "mariadb" {
        "MariaDB"
    } else {
        "MySQL"
    };
    let line = stderr
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("ERROR") || l.contains("error:") || l.contains("Got error"))
        .or_else(|| stderr.lines().map(str::trim).find(|l| !l.is_empty()))
        .unwrap_or("the database client failed");
    // ERROR 2002 / 2003: nothing listening.
    if line.contains("2002") || line.contains("2003") || line.contains("Can't connect") {
        return Error::Database(format!(
            "{name} is not running. Start it on the Services screen or with `stackyard start {server}`."
        ));
    }
    // `ERROR 1049 (42000) at line 1: Unknown database 'x'` → `Unknown database 'x'`.
    let message = line
        .strip_prefix("ERROR")
        .and_then(|rest| rest.find(')').map(|i| &rest[i + 1..]))
        .and_then(|rest| rest.split_once(": ").map(|(_, message)| message))
        .unwrap_or(line);
    Error::Database(message.to_string())
}

fn read_all(mut from: impl Read) -> String {
    let mut bytes = Vec::new();
    let _ = from.read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Every backup, newest first.
pub fn backups(home: &Path) -> Vec<Backup> {
    let mut all = Vec::new();
    let Ok(dirs) = std::fs::read_dir(backups_dir(home)) else {
        return all;
    };
    for dir in dirs.flatten() {
        let database = dir.file_name().to_string_lossy().into_owned();
        let Ok(files) = std::fs::read_dir(dir.path()) else {
            continue;
        };
        for file in files.flatten() {
            let name = file.file_name().to_string_lossy().into_owned();
            if name.ends_with(".sql.gz") && !name.starts_with('.') {
                all.extend(backup_info(&database, &file.path()));
            }
        }
    }
    all.sort_by(|a, b| b.created.cmp(&a.created).then(a.file.cmp(&b.file)));
    all
}

pub fn remove_backup(home: &Path, database: &str, file: &str) -> Result<()> {
    let path = backup_path(home, database, file)?;
    std::fs::remove_file(&path).map_err(|source| Error::Io { path, source })
}

pub fn backups_dir(home: &Path) -> PathBuf {
    home.join("backups")
}

/// A backup's path, refusing anything that would leave its folder.
fn backup_path(home: &Path, database: &str, file: &str) -> Result<PathBuf> {
    let plain = |s: &str| {
        !s.is_empty() && !s.starts_with('.') && !s.contains(['/', '\\', ':']) && s != ".."
    };
    if !plain(database) || !plain(file) || !file.ends_with(".sql.gz") {
        return Err(Error::Database(format!(
            "there is no backup {database}/{file}"
        )));
    }
    let path = backups_dir(home).join(database).join(file);
    if !path.is_file() {
        return Err(Error::Database(format!(
            "there is no backup {database}/{file}"
        )));
    }
    Ok(path)
}

fn backup_info(database: &str, path: &Path) -> Option<Backup> {
    let meta = std::fs::metadata(path).ok()?;
    let created = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as u64);
    let file = path.file_name()?.to_string_lossy().into_owned();
    Some(Backup {
        server: if postgres::is_backup(&file) {
            "postgres"
        } else {
            "mysql"
        },
        database: database.to_string(),
        file,
        size: meta.len(),
        created,
    })
}

/// `20261014-235959` in local time, for backup file names: sortable and unique enough.
pub(crate) fn stamp() -> String {
    let at = crate::scheduler::clock::now();
    let m = crate::scheduler::clock::local(at);
    format!(
        "{}{:02}{:02}-{:02}{:02}{:02}",
        m.year,
        m.month,
        m.day,
        m.hour,
        m.minute,
        at.rem_euclid(60)
    )
}

/// Names for new databases: letters, digits, `_` and `-`, up to 64.
pub(crate) fn check_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if !ok {
        return Err(Error::Database(
            "use letters, digits, _ and - for database names (up to 64)".into(),
        ));
    }
    Ok(())
}

fn check_user_database(name: &str) -> Result<()> {
    if SYSTEM.iter().any(|s| s.eq_ignore_ascii_case(name)) {
        return Err(Error::Database(format!(
            "{name} belongs to the database server itself"
        )));
    }
    if name.is_empty() {
        return Err(Error::Database("choose a database".into()));
    }
    Ok(())
}

/// A name quoted for SQL: `my``table`.
pub fn ident(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

/// A string quoted for SQL in the servers' default mode, where a
/// backslash escapes.
fn literal(text: &str) -> String {
    let mut out = String::from("'");
    for c in text.chars() {
        match c {
            '\'' => out.push_str("''"),
            '\\' => out.push_str("\\\\"),
            '\0' => out.push_str("\\0"),
            c => out.push(c),
        }
    }
    out.push('\'');
    out
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

#[cfg(test)]
pub(crate) mod tests;
