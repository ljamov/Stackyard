//! Whole-site backups, and sites packed into one file to move them to
//! another computer. Both are the same zip file:
//!
//! - `stackyard-site.toml`: the site's settings (not its folder, which
//!   differs between computers) and which database goes with it;
//! - `files/…`: the project folder, without `node_modules` (`npm install`
//!   makes it again, and it is often most of a project's size);
//! - `database/<file>.sql.gz`: a dump of the site's database, when it has
//!   one, in the format of the Databases screen's backups.
//!
//! Backups live in `backups/sites/<domain>/` inside Stackyard's data folder.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use crate::databases::{self, Client};
use crate::settings::Settings;
use crate::sites::{Site, SiteDatabase, SiteRuntime, SiteSettings, SiteStore};
use crate::{domain, Error, Result};

pub const MANIFEST: &str = "stackyard-site.toml";
const FILES: &str = "files/";
const DATABASE: &str = "database/";
/// Folders never packed: made again by the project's tools.
const SKIP: [&str; 1] = ["node_modules"];
/// The manifest format this version writes and reads.
const FORMAT: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    /// Milliseconds since 1970.
    pub created: u64,
    /// The Stackyard version that made the file.
    pub made_by: String,
    pub site: PackedSite,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<PackedDatabase>,
}

/// A site without its folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackedSite {
    pub name: String,
    pub tld: String,
    pub runtime: SiteRuntime,
    pub https: bool,
    #[serde(default)]
    pub dev_server: bool,
    #[serde(default, skip_serializing_if = "SiteSettings::is_empty")]
    pub settings: SiteSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackedDatabase {
    /// `mysql` (MySQL or MariaDB) or `postgres`.
    pub server: String,
    pub name: String,
    /// The dump's name inside `database/`.
    pub file: String,
}

/// A backup in the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SiteBackup {
    /// The site's domain, which names the folder the backup is in.
    pub site: String,
    pub file: String,
    pub size: u64,
    /// Milliseconds since 1970.
    pub created: u64,
    /// The database inside, e.g. `blog`.
    pub database: Option<String>,
}

/// What packing did, with anything the person should know.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Packed {
    pub path: PathBuf,
    pub size: u64,
    pub files: u64,
    pub database: Option<String>,
    /// Why the database was left out, when one was found but not packed.
    pub warning: Option<String>,
}

/// What a backup, export, restore or import is doing, for progress bars.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "stage", rename_all = "lowercase")]
pub enum PackStep {
    /// Saving the site's database to a file.
    Database,
    /// Adding the site's files to the zip.
    Files { done: u64, total: u64 },
    /// Writing the files in a zip into the site's folder.
    Unpacking { done: u64, total: u64 },
    /// Loading the database in a zip into the database server.
    Loading,
}

pub fn backups_dir(home: &Path) -> PathBuf {
    databases::backups_dir(home).join("sites")
}

/// The database a site uses: the one in its settings, else one named in
/// the project's `.env` (Laravel, Symfony and most PHP and Node apps) or
/// `wp-config.php` (WordPress).
pub fn site_database(site: &Site) -> Option<SiteDatabase> {
    if let Some(db) = &site.settings.database {
        return Some(db.clone());
    }
    from_env(&site.path).or_else(|| from_wp_config(&site.path))
}

fn from_env(dir: &Path) -> Option<SiteDatabase> {
    let text = std::fs::read_to_string(dir.join(".env")).ok()?;
    let mut name = None;
    let mut connection = None;
    for line in text.lines() {
        let Some((key, value)) = line.trim().split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches(['"', '\'']).to_string();
        match key.trim() {
            "DB_DATABASE" => name = Some(value),
            "DB_CONNECTION" => connection = Some(value),
            "DATABASE_URL" if name.is_none() => {
                // e.g. mysql://root@127.0.0.1:3306/shop?serverVersion=8.4
                let (scheme, rest) = value.split_once("://")?;
                let db = rest.split_once('/')?.1.split(['?', '#']).next()?;
                connection = Some(scheme.to_string());
                name = Some(db.to_string());
            }
            _ => {}
        }
    }
    let server = match connection.as_deref().unwrap_or("mysql") {
        "pgsql" | "postgres" | "postgresql" => "postgres",
        "mysql" | "mariadb" | "mysql2" => "mysql",
        _ => return None,
    };
    let name = name.filter(|n| databases::check_name(n).is_ok())?;
    Some(SiteDatabase {
        server: server.into(),
        name,
    })
}

fn from_wp_config(dir: &Path) -> Option<SiteDatabase> {
    // Bedrock keeps its settings in `.env`, which is read first.
    let text = std::fs::read_to_string(dir.join("wp-config.php")).ok()?;
    let at = text
        .find("'DB_NAME'")
        .or_else(|| text.find("\"DB_NAME\""))?;
    let rest = &text[at + "'DB_NAME'".len()..];
    let rest = rest.trim_start().strip_prefix(',')?.trim_start();
    let quote = rest.chars().next().filter(|c| *c == '\'' || *c == '"')?;
    let name = rest[1..].split(quote).next()?;
    databases::check_name(name).ok()?;
    Some(SiteDatabase {
        server: "mysql".into(),
        name: name.into(),
    })
}

/// Packs `site` into a new zip file at `dest`. A database that was found
/// by looking at the project but cannot be dumped (server stopped, no such
/// database) is left out with a warning; one picked in the site's settings
/// must work.
pub fn pack(
    home: &Path,
    site: &Site,
    dest: &Path,
    progress: &mut dyn FnMut(PackStep),
) -> Result<Packed> {
    if !site.path.is_dir() {
        return Err(Error::NotAFolder(site.path.clone()));
    }
    let io = |path: &Path| {
        let path = path.to_owned();
        move |source| Error::Io { path, source }
    };
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(io(parent))?;
    }
    let partial = dest.with_extension("zip.partial");
    let result = write_pack(home, site, &partial, dest, progress);
    if result.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    let packed = result?;
    std::fs::rename(&partial, dest).map_err(io(dest))?;
    Ok(packed)
}

fn write_pack(
    home: &Path,
    site: &Site,
    partial: &Path,
    dest: &Path,
    progress: &mut dyn FnMut(PackStep),
) -> Result<Packed> {
    let io = |path: &Path| {
        let path = path.to_owned();
        move |source| Error::Io { path, source }
    };
    let (dump, warning) = dump_database(home, site, progress)?;
    let file = File::create(partial).map_err(io(partial))?;
    let mut zip = zip::ZipWriter::new(BufWriter::new(file));
    let zip_err = |e: zip::result::ZipError| Error::Archive {
        path: dest.to_owned(),
        message: e.to_string(),
    };
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(6));

    let manifest = Manifest {
        format: FORMAT,
        created: now_ms(),
        made_by: env!("CARGO_PKG_VERSION").into(),
        site: PackedSite {
            name: site.name.clone(),
            tld: site.tld.clone(),
            runtime: site.runtime.clone(),
            https: site.https,
            dev_server: site.dev_server,
            settings: site.settings.clone(),
        },
        database: dump.as_ref().map(|(db, path)| PackedDatabase {
            server: db.server.clone(),
            name: db.name.clone(),
            file: file_name(path),
        }),
    };
    zip.start_file(MANIFEST, options).map_err(zip_err)?;
    zip.write_all(toml::to_string_pretty(&manifest)?.as_bytes())
        .map_err(io(partial))?;

    let skip = [dest.to_path_buf(), partial.to_path_buf()];
    let mut entries = Vec::new();
    let count = list_folder(&site.path, Path::new(""), &skip, &mut entries)
        .and_then(|()| add_entries(&mut zip, &site.path, &entries, options, progress))
        .map_err(|e| Error::Archive {
            path: dest.to_owned(),
            message: e,
        })?;

    if let Some((_, path)) = &dump {
        zip.start_file(
            format!("{DATABASE}{}", file_name(path)),
            options.compression_method(zip::CompressionMethod::Stored),
        )
        .map_err(zip_err)?;
        let mut from = File::open(path).map_err(io(path))?;
        std::io::copy(&mut from, &mut zip).map_err(io(path))?;
        // The site backup holds it now.
        let _ = std::fs::remove_file(path);
    }
    let mut out = zip.finish().map_err(zip_err)?;
    out.flush().map_err(io(partial))?;
    drop(out);
    let size = std::fs::metadata(partial).map_err(io(partial))?.len();
    Ok(Packed {
        path: dest.to_owned(),
        size,
        files: count,
        database: manifest.database.map(|d| d.name),
        warning,
    })
}

/// A database dump in the Databases screen's backups folder, and where it is.
type Dump = (SiteDatabase, PathBuf);

/// Dumps the site's database into the Databases screen's backups folder;
/// the caller moves it into the zip. The second part says why a database
/// that was found was left out.
fn dump_database(
    home: &Path,
    site: &Site,
    progress: &mut dyn FnMut(PackStep),
) -> Result<(Option<Dump>, Option<String>)> {
    let Some(db) = site_database(site) else {
        return Ok((None, None));
    };
    progress(PackStep::Database);
    let picked = site.settings.database.is_some();
    let dumped = Client::open_server(home, Some(&db.server)).and_then(|client| {
        let exists = client.databases()?.iter().any(|d| d.name == db.name);
        if !exists {
            return Err(Error::Database(format!("there is no database {}", db.name)));
        }
        client.backup(&db.name)
    });
    match dumped {
        Ok(backup) => {
            let path = databases::backups_dir(home)
                .join(&backup.database)
                .join(&backup.file);
            Ok((Some((db, path)), None))
        }
        Err(e) if picked => Err(e),
        Err(e) => Ok((
            None,
            Some(format!(
                "the database {} was left out ({e}); start the database server and back up again to include it",
                db.name
            )),
        )),
    }
}

/// A folder or file to pack, by its path inside the project.
struct Entry {
    inner: PathBuf,
    dir: bool,
}

/// Lists what to pack under `root/relative`, folders before what is in them.
fn list_folder(
    root: &Path,
    relative: &Path,
    skip: &[PathBuf],
    out: &mut Vec<Entry>,
) -> std::result::Result<(), String> {
    let dir = root.join(relative);
    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name();
        let path = entry.path();
        let inner = relative.join(&name);
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        // Links can point anywhere, even outside the project; leave them out.
        if kind.is_symlink() || skip.contains(&path) {
            continue;
        }
        if kind.is_dir() {
            if SKIP.iter().any(|s| name == *s) {
                continue;
            }
            out.push(Entry {
                inner: inner.clone(),
                dir: true,
            });
            list_folder(root, &inner, skip, out)?;
        } else if kind.is_file() {
            out.push(Entry { inner, dir: false });
        }
    }
    Ok(())
}

/// Adds the listed folders and files to the zip. Returns how many files.
fn add_entries<W: Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    root: &Path,
    entries: &[Entry],
    options: zip::write::SimpleFileOptions,
    progress: &mut dyn FnMut(PackStep),
) -> std::result::Result<u64, String> {
    let total = entries.iter().filter(|e| !e.dir).count() as u64;
    let mut done = 0;
    progress(PackStep::Files { done, total });
    for entry in entries {
        let zip_name = format!("{FILES}{}", slash(&entry.inner));
        if entry.dir {
            zip.add_directory(format!("{zip_name}/"), options)
                .map_err(|e| e.to_string())?;
            continue;
        }
        let path = root.join(&entry.inner);
        let meta = std::fs::metadata(&path).ok();
        let size = meta.as_ref().map_or(0, |m| m.len());
        let mut file_options = options.large_file(size >= u32::MAX as u64);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(meta) = &meta {
                file_options = file_options.unix_permissions(meta.permissions().mode() & 0o777);
            }
        }
        zip.start_file(zip_name, file_options)
            .map_err(|e| e.to_string())?;
        let mut from = File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        std::io::copy(&mut from, zip).map_err(|e| format!("{}: {e}", path.display()))?;
        done += 1;
        progress(PackStep::Files { done, total });
    }
    Ok(done)
}

/// Backs a site up into its backups folder.
pub fn backup(home: &Path, site: &Site, progress: &mut dyn FnMut(PackStep)) -> Result<Packed> {
    let domain = site.domain();
    let dir = backups_dir(home).join(&domain);
    let stamp = databases::stamp();
    // Two backups in one second (restore makes one first) get their own files.
    let dest = (1..)
        .map(|n| match n {
            1 => dir.join(format!("{domain}-{stamp}.zip")),
            n => dir.join(format!("{domain}-{stamp}-{n}.zip")),
        })
        .find(|p| !p.exists())
        .expect("a free name");
    pack(home, site, &dest, progress)
}

/// A site's backups, newest first.
pub fn backups(home: &Path, domain: &str) -> Vec<SiteBackup> {
    let Ok(files) = std::fs::read_dir(backups_dir(home).join(domain)) else {
        return Vec::new();
    };
    let mut all: Vec<SiteBackup> = files
        .flatten()
        .filter_map(|entry| {
            let file = entry.file_name().to_string_lossy().into_owned();
            if !file.ends_with(".zip") || file.starts_with('.') {
                return None;
            }
            let meta = entry.metadata().ok()?;
            let manifest = read_manifest(&entry.path()).ok();
            Some(SiteBackup {
                site: domain.to_string(),
                created: manifest.as_ref().map_or_else(
                    || {
                        meta.modified()
                            .ok()
                            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                            .map_or(0, |d| d.as_millis() as u64)
                    },
                    |m| m.created,
                ),
                database: manifest.and_then(|m| m.database.map(|d| d.name)),
                size: meta.len(),
                file,
            })
        })
        .collect();
    all.sort_by(|a, b| b.created.cmp(&a.created).then(b.file.cmp(&a.file)));
    all
}

/// A backup's path, refusing anything that would leave its folder.
pub fn backup_path(home: &Path, domain: &str, file: &str) -> Result<PathBuf> {
    let plain = |s: &str| !s.is_empty() && !s.starts_with('.') && !s.contains(['/', '\\', ':']);
    let path = backups_dir(home).join(domain).join(file);
    if !plain(domain) || !plain(file) || !file.ends_with(".zip") || !path.is_file() {
        return Err(Error::Shell(format!(
            "there is no backup {file} of {domain}"
        )));
    }
    Ok(path)
}

pub fn remove_backup(home: &Path, domain: &str, file: &str) -> Result<()> {
    let path = backup_path(home, domain, file)?;
    std::fs::remove_file(&path).map_err(|source| Error::Io { path, source })
}

pub fn read_manifest(pack: &Path) -> Result<Manifest> {
    let mut zip = open_zip(pack)?;
    let mut entry = zip.by_name(MANIFEST).map_err(|_| not_a_pack(pack))?;
    let mut text = String::new();
    entry
        .read_to_string(&mut text)
        .map_err(|_| not_a_pack(pack))?;
    let manifest: Manifest = toml::from_str(&text).map_err(|source| Error::Parse {
        path: pack.join(MANIFEST),
        source,
    })?;
    if manifest.format > FORMAT {
        return Err(Error::Shell(format!(
            "{} was made by a newer Stackyard ({}); update Stackyard to open it",
            pack.display(),
            manifest.made_by
        )));
    }
    Ok(manifest)
}

fn open_zip(pack: &Path) -> Result<zip::ZipArchive<BufReader<File>>> {
    let file = File::open(pack).map_err(|source| Error::Io {
        path: pack.to_owned(),
        source,
    })?;
    zip::ZipArchive::new(BufReader::new(file)).map_err(|_| not_a_pack(pack))
}

fn not_a_pack(pack: &Path) -> Error {
    Error::Shell(format!(
        "{} is not a Stackyard site file or backup",
        pack.display()
    ))
}

/// Puts back a site's files and database from `pack`. Files in the backup
/// replace the site's files of the same name; files added since are kept.
/// A backup of the site as it is now is made first, and returned.
pub fn restore(
    home: &Path,
    site: &Site,
    pack: &Path,
    progress: &mut dyn FnMut(PackStep),
) -> Result<Packed> {
    let manifest = read_manifest(pack)?;
    let safety = backup(home, site, progress)?;
    unpack_files(pack, &site.path, progress)?;
    if let Some(db) = &manifest.database {
        progress(PackStep::Loading);
        restore_database(home, pack, db, None)?;
    }
    Ok(safety)
}

/// Writes the files in `pack` into `dest`.
fn unpack_files(pack: &Path, dest: &Path, progress: &mut dyn FnMut(PackStep)) -> Result<u64> {
    let fail = |message: String| Error::Archive {
        path: pack.to_owned(),
        message,
    };
    let mut zip = open_zip(pack)?;
    let total = zip
        .file_names()
        .filter(|n| n.len() > FILES.len() && n.starts_with(FILES) && !n.ends_with('/'))
        .count() as u64;
    let mut count = 0;
    progress(PackStep::Unpacking { done: 0, total });
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| fail(e.to_string()))?;
        let Some(name) = entry.name().strip_prefix(FILES).map(str::to_string) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        let safe = entry.enclosed_name().is_some()
            && Path::new(&name)
                .components()
                .all(|c| matches!(c, Component::Normal(_)));
        if !safe || entry.is_symlink() {
            return Err(fail(format!("refusing unsafe path {}", entry.name())));
        }
        let target = dest.join(&name);
        let io = |source| Error::Io {
            path: target.clone(),
            source,
        };
        if entry.is_dir() {
            std::fs::create_dir_all(&target).map_err(io)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        let mut out = File::create(&target).map_err(io)?;
        std::io::copy(&mut entry, &mut out).map_err(io)?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ =
                std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode & 0o777));
        }
        count += 1;
        progress(PackStep::Unpacking { done: count, total });
    }
    Ok(count)
}

/// Loads the dump in `pack` into `into` (the original name when `None`).
fn restore_database(
    home: &Path,
    pack: &Path,
    db: &PackedDatabase,
    into: Option<&str>,
) -> Result<()> {
    databases::check_name(&db.name)?;
    let plain =
        !db.file.is_empty() && !db.file.contains(['/', '\\', ':']) && !db.file.starts_with('.');
    if !plain {
        return Err(not_a_pack(pack));
    }
    let client = Client::open_server(home, Some(&db.server))?;
    // Put the dump where the Databases screen keeps backups and restore it
    // from there, the same way that screen does.
    let dir = databases::backups_dir(home).join(&db.name);
    std::fs::create_dir_all(&dir).map_err(|source| Error::Io {
        path: dir.clone(),
        source,
    })?;
    let path = dir.join(&db.file);
    {
        let mut zip = open_zip(pack)?;
        let mut entry = zip
            .by_name(&format!("{DATABASE}{}", db.file))
            .map_err(|_| not_a_pack(pack))?;
        let mut out = File::create(&path).map_err(|source| Error::Io {
            path: path.clone(),
            source,
        })?;
        std::io::copy(&mut entry, &mut out).map_err(|source| Error::Io {
            path: path.clone(),
            source,
        })?;
    }
    let result = client.restore(&db.name, &db.file, into);
    let _ = std::fs::remove_file(&path);
    result
}

/// Where and as what to bring in a site file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct ImportOptions {
    /// The new site's name; the one in the file when `None`.
    pub name: Option<String>,
    /// The project folder, which must be new or empty; a new folder in the
    /// sites folder when `None`.
    pub path: Option<PathBuf>,
}

/// Brings in a site file made by [`pack`] (or a backup): unpacks its
/// files, adds the site and loads its database.
pub fn import(
    home: &Path,
    pack: &Path,
    options: ImportOptions,
    progress: &mut dyn FnMut(PackStep),
) -> Result<(Site, Option<String>)> {
    let manifest = read_manifest(pack)?;
    let settings = Settings::load_from(&home.join("settings.toml"))?;
    let name = options
        .name
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| manifest.site.name.clone());
    domain::validate_site_name(&name)?;
    let path = options
        .path
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| crate::paths::expand_home(&p))
        .unwrap_or_else(|| settings.sites_folder(home).join(&name));
    let has_files = path
        .read_dir()
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false);
    if has_files {
        return Err(Error::Shell(format!(
            "{} already has files in it; pick a new or empty folder",
            path.display()
        )));
    }
    let mut store = SiteStore::load_from(&home.join("sites.toml"))?;
    let site = Site {
        name,
        tld: manifest.site.tld.clone(),
        path: path.clone(),
        runtime: manifest.site.runtime.clone(),
        https: manifest.site.https,
        dev_server: manifest.site.dev_server,
        settings: manifest.site.settings.clone(),
    };
    if store.list().iter().any(|s| s.domain() == site.domain()) {
        return Err(Error::SiteExists(site.domain()));
    }
    std::fs::create_dir_all(&path).map_err(|source| Error::Io {
        path: path.clone(),
        source,
    })?;
    unpack_files(pack, &path, progress)?;
    let site = store.add(site)?.clone();
    store.save()?;
    let warning = manifest.database.as_ref().and_then(|db| {
        progress(PackStep::Loading);
        restore_database(home, pack, db, None).err().map(|e| {
            format!(
                "the files are in place, but the database {} could not be loaded ({e}); start the database server and restore it from the site's backups",
                db.name
            )
        })
    });
    // Keep the file, so the database can be loaded later from the site's backups.
    if warning.is_some() {
        let dir = backups_dir(home).join(site.domain());
        if std::fs::create_dir_all(&dir).is_ok() {
            let _ = std::fs::copy(pack, dir.join(format!("{}-imported.zip", site.domain())));
        }
    }
    Ok((site, warning))
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn slash(path: &Path) -> String {
    path.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(home: &Path) -> Site {
        let path = home.join("Code").join("blog");
        std::fs::create_dir_all(path.join("public")).unwrap();
        std::fs::create_dir_all(path.join("node_modules/left-pad")).unwrap();
        std::fs::write(path.join("public/index.php"), "<?php echo 'hi';").unwrap();
        std::fs::write(path.join("node_modules/left-pad/index.js"), "x").unwrap();
        std::fs::write(path.join("README.md"), "old").unwrap();
        let mut settings = SiteSettings::default();
        settings.env.insert("APP_ENV".into(), "local".into());
        Site {
            name: "blog".into(),
            tld: ".stack".into(),
            path,
            runtime: SiteRuntime::Php {
                version: "8.4".into(),
            },
            https: true,
            dev_server: false,
            settings,
        }
    }

    #[test]
    fn finds_the_database_in_the_project() {
        let dir = crate::testutil::tempdir();
        let site = Site {
            path: dir.clone(),
            ..site(&crate::testutil::tempdir())
        };
        assert_eq!(site_database(&site), None);
        std::fs::write(
            dir.join("wp-config.php"),
            "<?php\ndefine( 'DB_NAME', 'wp_blog' );\ndefine( 'DB_USER', 'root' );",
        )
        .unwrap();
        assert_eq!(site_database(&site).unwrap().name, "wp_blog");
        std::fs::write(
            dir.join(".env"),
            "APP_NAME=Shop\nDB_CONNECTION=pgsql\nDB_DATABASE=\"shop\"\n",
        )
        .unwrap();
        let found = site_database(&site).unwrap();
        assert_eq!(
            (found.server.as_str(), found.name.as_str()),
            ("postgres", "shop")
        );
        std::fs::write(
            dir.join(".env"),
            "DATABASE_URL=\"mysql://root@127.0.0.1:3306/symfony_app?serverVersion=8.4\"\n",
        )
        .unwrap();
        let found = site_database(&site).unwrap();
        assert_eq!(
            (found.server.as_str(), found.name.as_str()),
            ("mysql", "symfony_app")
        );
        std::fs::write(
            dir.join(".env"),
            "DB_CONNECTION=sqlite\nDB_DATABASE=/x/db.sqlite\n",
        )
        .unwrap();
        // SQLite lives in the project's files, which are packed anyway.
        assert_eq!(site_database(&site).map(|d| d.name), Some("wp_blog".into()));

        let picked = Site {
            settings: SiteSettings {
                database: Some(SiteDatabase {
                    server: "mysql".into(),
                    name: "mine".into(),
                }),
                ..SiteSettings::default()
            },
            ..site
        };
        assert_eq!(site_database(&picked).unwrap().name, "mine");
    }

    #[test]
    fn backs_up_restores_and_imports() {
        let home = crate::testutil::tempdir();
        let site = site(&home);
        let mut steps = Vec::new();
        let packed = backup(&home, &site, &mut |step| steps.push(step)).unwrap();
        assert_eq!(packed.files, 2, "node_modules is left out");
        assert_eq!(steps.first(), Some(&PackStep::Files { done: 0, total: 2 }));
        assert_eq!(steps.last(), Some(&PackStep::Files { done: 2, total: 2 }));
        assert_eq!(packed.database, None);
        let list = backups(&home, "blog.stack");
        assert_eq!(list.len(), 1);
        assert!(list[0].file.starts_with("blog.stack-"));

        let manifest = read_manifest(&packed.path).unwrap();
        assert_eq!(manifest.site.name, "blog");
        assert_eq!(manifest.site.settings.env["APP_ENV"], "local");

        // Restore puts changed files back and keeps new ones.
        std::fs::write(site.path.join("README.md"), "changed").unwrap();
        std::fs::write(site.path.join("new.txt"), "new").unwrap();
        std::fs::remove_file(site.path.join("public/index.php")).unwrap();
        let mut steps = Vec::new();
        let safety = restore(&home, &site, &packed.path, &mut |step| steps.push(step)).unwrap();
        assert_eq!(
            steps.last(),
            Some(&PackStep::Unpacking { done: 2, total: 2 })
        );
        assert_eq!(
            std::fs::read_to_string(site.path.join("README.md")).unwrap(),
            "old"
        );
        assert!(site.path.join("public/index.php").is_file());
        assert!(site.path.join("new.txt").is_file());
        assert!(safety.path.is_file());

        // Import under another name into a new folder.
        let (imported, warning) = import(
            &home,
            &packed.path,
            ImportOptions {
                name: Some("blog-copy".into()),
                path: Some(home.join("Code/blog-copy")),
            },
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(warning, None);
        assert_eq!(imported.domain(), "blog-copy.stack");
        assert_eq!(imported.settings.env["APP_ENV"], "local");
        assert!(home.join("Code/blog-copy/public/index.php").is_file());
        assert!(!home.join("Code/blog-copy/node_modules").exists());
        let store = SiteStore::load_from(&home.join("sites.toml")).unwrap();
        assert_eq!(store.list().len(), 1);
        // A folder with files in it, or a name in use, is refused.
        let again = |name: &str| {
            import(
                &home,
                &packed.path,
                ImportOptions {
                    name: Some(name.into()),
                    path: Some(home.join("Code/blog-copy")),
                },
                &mut |_| {},
            )
        };
        assert!(again("third").is_err());
        assert!(matches!(
            import(
                &home,
                &packed.path,
                ImportOptions {
                    name: Some("blog-copy".into()),
                    path: Some(home.join("Code/empty")),
                },
                &mut |_| {},
            ),
            Err(Error::SiteExists(_))
        ));

        remove_backup(&home, "blog.stack", &list[0].file).unwrap();
        assert!(remove_backup(&home, "blog.stack", "../../sites.toml").is_err());
    }

    #[test]
    fn refuses_files_that_are_not_site_files() {
        let home = crate::testutil::tempdir();
        let other = home.join("other.zip");
        let mut zip = zip::ZipWriter::new(File::create(&other).unwrap());
        zip.start_file("hello.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.finish().unwrap();
        assert!(read_manifest(&other).is_err());
        std::fs::write(home.join("text.zip"), "not a zip").unwrap();
        assert!(read_manifest(&home.join("text.zip")).is_err());
    }
}
