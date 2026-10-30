//! Tools for a WordPress theme or plugin project: read and set its
//! version, and pack it into the `.zip` WordPress installs from.
//!
//! The version lives in the theme's `style.css` header or the plugin's
//! main file header (the `.php` file in its folder with `Plugin Name:`),
//! and in `readme.txt`'s `Stable tag:` when there is one. The zip holds one
//! folder named after the theme or plugin, as WordPress expects, and
//! leaves out what is only there for building it: Git, `node_modules`,
//! editor settings, other zips, and whatever `.distignore` lists (the file
//! `wp dist-archive` reads too). Zips are written to `dist/` in the
//! project's own folder; they never end up in the next zip, and a
//! `dist/.gitignore` keeps them out of Git.

use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::projects::ProjectKind;
use crate::{Error, Result};

fn fail(message: impl Into<String>) -> Error {
    Error::Project(message.into())
}

/// Never packed, wherever they are in the folder.
const SKIP: [&str; 14] = [
    ".git",
    ".github",
    ".svn",
    ".idea",
    ".vscode",
    "node_modules",
    ".DS_Store",
    "Thumbs.db",
    ".distignore",
    ".gitignore",
    ".gitattributes",
    ".editorconfig",
    ".stackyard",
    ".env",
];

/// What the theme or plugin says about itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Package {
    /// Its folder's name, which WordPress knows it by.
    pub slug: String,
    /// The file with its header, inside its folder, e.g. `style.css`.
    pub main_file: Option<String>,
    /// `Theme Name:` or `Plugin Name:`.
    pub name: Option<String>,
    /// `Version:`; `None` when the header has none.
    pub version: Option<String>,
}

/// A packed theme or plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Built {
    pub path: PathBuf,
    pub files: u64,
    pub bytes: u64,
}

fn slug(folder: &Path) -> Result<String> {
    folder
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .filter(|n| !n.is_empty())
        .ok_or_else(|| fail(format!("{} has no folder name", folder.display())))
}

/// The start of a file, where WordPress reads headers from (8 KB).
fn head(path: &Path) -> Option<String> {
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(8192)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Where `Field:` starts on `line`, as WordPress reads headers: after
/// nothing but spaces and comment marks (`*`, `/`, `#`, `@`).
fn field_at(line: &str, field: &str) -> Option<usize> {
    let lead = line.len()
        - line
            .trim_start_matches([' ', '\t', '/', '*', '#', '@'])
            .len();
    let rest = &line.as_bytes()[lead..];
    let wanted = field.len() + 1;
    (rest.len() >= wanted
        && rest[..field.len()].eq_ignore_ascii_case(field.as_bytes())
        && rest[field.len()] == b':')
        .then_some(lead)
}

/// The value of the header `field` in `text`, up to the end of its line
/// or a closing `*/`.
fn header(text: &str, field: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let at = field_at(line, field)?;
        let value = &line[at + field.len() + 1..];
        let value = value.split("*/").next().unwrap_or(value).trim();
        (!value.is_empty()).then(|| value.to_string())
    })
}

/// The file holding the header: `style.css`, or the plugin's main `.php`.
fn main_file(kind: ProjectKind, folder: &Path) -> Result<Option<PathBuf>> {
    match kind {
        ProjectKind::WordpressTheme => {
            let style = folder.join("style.css");
            Ok(style.is_file().then_some(style))
        }
        ProjectKind::WordpressPlugin => {
            let slug = slug(folder)?;
            let mut php: Vec<PathBuf> = std::fs::read_dir(folder)
                .map_err(|source| Error::Io {
                    path: folder.to_owned(),
                    source,
                })?
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "php"))
                .collect();
            // The one named after the folder first, as WordPress finds it.
            php.sort_by_key(|p| {
                (
                    p.file_stem().and_then(|s| s.to_str()) != Some(&slug),
                    p.clone(),
                )
            });
            Ok(php
                .into_iter()
                .find(|p| head(p).is_some_and(|t| header(&t, "Plugin Name").is_some())))
        }
        _ => Err(fail(format!(
            "a {} is not a WordPress theme or plugin",
            kind.name()
        ))),
    }
}

/// Reads the theme's or plugin's name and version from its header.
pub fn read(kind: ProjectKind, folder: &Path) -> Result<Package> {
    let slug = slug(folder)?;
    let Some(main) = main_file(kind, folder)? else {
        return Ok(Package {
            slug,
            main_file: None,
            name: None,
            version: None,
        });
    };
    let text = head(&main).unwrap_or_default();
    let name_field = match kind {
        ProjectKind::WordpressTheme => "Theme Name",
        _ => "Plugin Name",
    };
    Ok(Package {
        slug,
        main_file: main.file_name().map(|n| n.to_string_lossy().into_owned()),
        name: header(&text, name_field),
        version: header(&text, "Version"),
    })
}

/// A version WordPress and a zip's name can carry: `1.2.0`, `2.0-beta.1`.
fn check_version(version: &str) -> Result<()> {
    let ok = !version.is_empty()
        && version.len() <= 32
        && version.starts_with(|c: char| c.is_ascii_alphanumeric())
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | '_'));
    if ok {
        Ok(())
    } else {
        Err(fail(format!(
            "{version:?} is not a version; use numbers and dots, like 1.2.0"
        )))
    }
}

/// Puts `value` after `Field:` on the line that has it, keeping the rest of
/// the line (spacing, a closing `*/`). `None` when no line has the field.
fn replace_header(text: &str, field: &str, value: &str) -> Option<String> {
    let mut done = false;
    let mut out = String::with_capacity(text.len() + value.len());
    for line in text.split_inclusive('\n') {
        let at = (!done).then(|| field_at(line, field)).flatten();
        let Some(at) = at else {
            out.push_str(line);
            continue;
        };
        let start = at + field.len() + 1;
        let rest = &line[start..];
        let gap = &rest[..rest.len() - rest.trim_start_matches([' ', '\t']).len()];
        let old_end = rest
            .find("*/")
            .unwrap_or_else(|| rest.trim_end_matches(['\r', '\n']).len());
        let old = &rest[..old_end];
        let tail = &rest[old.trim_end().len()..];
        out.push_str(&line[..start]);
        out.push_str(if gap.is_empty() { " " } else { gap });
        out.push_str(value);
        out.push_str(tail);
        done = true;
    }
    done.then_some(out)
}

/// Adds a `Version:` line after the name line, in the same style.
fn add_version(text: &str, name_field: &str, value: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len() + value.len() + 16);
    let mut done = false;
    for line in text.split_inclusive('\n') {
        out.push_str(line);
        if done {
            continue;
        }
        if let Some(at) = field_at(line, name_field) {
            let lead = &line[..at];
            let ending = if line.ends_with("\r\n") { "\r\n" } else { "\n" };
            if !line.ends_with('\n') {
                out.push_str(ending);
            }
            out.push_str(&format!("{lead}Version: {value}{ending}"));
            done = true;
        }
    }
    done.then_some(out)
}

fn write(path: &Path, text: &str) -> Result<()> {
    std::fs::write(path, text).map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })
}

/// Sets the version in the header, and in `readme.txt`'s `Stable tag:`
/// when there is one. Returns the theme or plugin as it is now.
pub fn set_version(kind: ProjectKind, folder: &Path, version: &str) -> Result<Package> {
    let version = version.trim();
    check_version(version)?;
    let main = main_file(kind, folder)?.ok_or_else(|| {
        fail(match kind {
            ProjectKind::WordpressTheme => "there is no style.css with a theme header".to_string(),
            _ => "there is no .php file with a Plugin Name header".to_string(),
        })
    })?;
    let text = std::fs::read_to_string(&main).map_err(|source| Error::Io {
        path: main.clone(),
        source,
    })?;
    let name_field = match kind {
        ProjectKind::WordpressTheme => "Theme Name",
        _ => "Plugin Name",
    };
    let changed = replace_header(&text, "Version", version)
        .or_else(|| add_version(&text, name_field, version))
        .ok_or_else(|| fail(format!("{} has no {name_field} header", main.display())))?;
    write(&main, &changed)?;
    let readme = folder.join("readme.txt");
    if let Ok(text) = std::fs::read_to_string(&readme) {
        if let Some(changed) = replace_header(&text, "Stable tag", version) {
            write(&readme, &changed)?;
        }
    }
    read(kind, folder)
}

/// One line of `.distignore`: a name (`*` matches any run of characters)
/// matched against every file and folder name, or, with a `/` inside, a
/// path from the project's folder.
struct Pattern {
    glob: String,
    anchored: bool,
    dir_only: bool,
}

impl Pattern {
    fn parse(line: &str) -> Option<Pattern> {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let dir_only = line.ends_with('/');
        let line = line.trim_end_matches('/');
        let anchored = line.contains('/');
        let glob = line.trim_start_matches('/').to_string();
        (!glob.is_empty()).then_some(Pattern {
            glob,
            anchored,
            dir_only,
        })
    }

    fn matches(&self, inner: &str, name: &str, dir: bool) -> bool {
        if self.dir_only && !dir {
            return false;
        }
        wildcard(&self.glob, if self.anchored { inner } else { name })
    }
}

/// `*` matches any run of characters but `/`; `?` one of them.
fn wildcard(glob: &str, text: &str) -> bool {
    fn go(g: &[u8], t: &[u8]) -> bool {
        match g.split_first() {
            None => t.is_empty(),
            Some((b'*', rest)) => (0..=t.len())
                .take_while(|&i| i == 0 || t[i - 1] != b'/')
                .any(|i| go(rest, &t[i..])),
            Some((b'?', rest)) => t.first().is_some_and(|&c| c != b'/') && go(rest, &t[1..]),
            Some((c, rest)) => t.first() == Some(c) && go(rest, &t[1..]),
        }
    }
    go(glob.as_bytes(), text.as_bytes())
}

fn ignored(folder: &Path) -> Vec<Pattern> {
    std::fs::read_to_string(folder.join(".distignore"))
        .map(|text| text.lines().filter_map(Pattern::parse).collect())
        .unwrap_or_default()
}

/// Lists what goes in the zip, by path inside the folder, `/`-separated.
fn list(
    root: &Path,
    inner: &str,
    patterns: &[Pattern],
    out: &mut Vec<(String, PathBuf)>,
) -> Result<()> {
    let dir = if inner.is_empty() {
        root.to_owned()
    } else {
        root.join(inner)
    };
    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .map_err(|source| Error::Io {
            path: dir.clone(),
            source,
        })?
        .flatten()
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = if inner.is_empty() {
            name.clone()
        } else {
            format!("{inner}/{name}")
        };
        // Links can point anywhere, even outside the project; leave them out.
        if kind.is_symlink() || SKIP.contains(&name.as_str()) {
            continue;
        }
        let lower = name.to_ascii_lowercase();
        if kind.is_file() && (lower.ends_with(".zip") || lower.ends_with(".zip.partial")) {
            continue;
        }
        if patterns
            .iter()
            .any(|p| p.matches(&path, &name, kind.is_dir()))
        {
            continue;
        }
        if kind.is_dir() {
            list(root, &path, patterns, out)?;
        } else if kind.is_file() {
            out.push((path, entry.path()));
        }
    }
    Ok(())
}

/// Where built zips go: `dist/` in the project's own folder.
pub fn builds_dir(folder: &Path) -> PathBuf {
    folder.join("dist")
}

/// Makes `out` and, when it has none, a `.gitignore` there that keeps the
/// zips out of Git. Other files a build tool puts in `dist/` stay tracked.
fn prepare_out(out: &Path) -> Result<()> {
    std::fs::create_dir_all(out).map_err(|source| Error::Io {
        path: out.to_owned(),
        source,
    })?;
    let ignore = out.join(".gitignore");
    if !ignore.exists() {
        write(&ignore, "# Zips built by Stackyard\n*.zip\n*.zip.partial\n")?;
    }
    Ok(())
}

/// Packs the theme or plugin into `<out>/<slug>-<version>.zip` (or
/// `<slug>.zip` without a version), replacing one of the same name. Every
/// file is inside a `<slug>/` folder, as WordPress's Upload expects.
pub fn build_zip(kind: ProjectKind, folder: &Path, out: &Path) -> Result<Built> {
    let package = read(kind, folder)?;
    if package.main_file.is_none() {
        return Err(fail(match kind {
            ProjectKind::WordpressTheme => {
                "WordPress needs a style.css with a Theme Name header to install a theme"
            }
            _ => "WordPress needs a .php file with a Plugin Name header to install a plugin",
        }));
    }
    let mut files = Vec::new();
    list(folder, "", &ignored(folder), &mut files)?;
    prepare_out(out)?;
    let file_name = match &package.version {
        Some(v) if check_version(v).is_ok() => format!("{}-{v}.zip", package.slug),
        _ => format!("{}.zip", package.slug),
    };
    let dest = out.join(&file_name);
    let partial = out.join(format!("{file_name}.partial"));
    let zip_err = |e: zip::result::ZipError| Error::Archive {
        path: dest.clone(),
        message: e.to_string(),
    };
    let io = |path: &Path| {
        let path = path.to_owned();
        move |source| Error::Io { path, source }
    };
    let written = (|| -> Result<()> {
        let file = File::create(&partial).map_err(io(&partial))?;
        let mut zip = zip::ZipWriter::new(BufWriter::new(file));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .compression_level(Some(9));
        zip.add_directory(format!("{}/", package.slug), options)
            .map_err(zip_err)?;
        let mut made = std::collections::HashSet::new();
        for (inner, path) in &files {
            // Folders first, so every unzip tool makes them.
            let mut at = 0;
            while let Some(i) = inner[at..].find('/') {
                let dir = &inner[..at + i];
                if made.insert(dir.to_string()) {
                    zip.add_directory(format!("{}/{dir}/", package.slug), options)
                        .map_err(zip_err)?;
                }
                at += i + 1;
            }
            zip.start_file(format!("{}/{inner}", package.slug), options)
                .map_err(zip_err)?;
            let mut from = File::open(path).map_err(io(path))?;
            std::io::copy(&mut from, &mut zip).map_err(io(path))?;
        }
        zip.finish()
            .map_err(zip_err)?
            .flush()
            .map_err(io(&partial))?;
        Ok(())
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&partial);
        return Err(e);
    }
    let _ = std::fs::remove_file(&dest);
    std::fs::rename(&partial, &dest).map_err(io(&dest))?;
    let bytes = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
    Ok(Built {
        path: dest,
        files: files.len() as u64,
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects;

    fn names(zip: &Path) -> Vec<String> {
        let mut archive = zip::ZipArchive::new(File::open(zip).unwrap()).unwrap();
        (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect()
    }

    #[test]
    fn reads_and_sets_a_plugins_version() {
        let home = crate::testutil::tempdir();
        let plugin = home.join("shop-tools");
        projects::scaffold(ProjectKind::WordpressPlugin, &plugin, "Shop tools").unwrap();
        std::fs::write(plugin.join("helpers.php"), "<?php // no header").unwrap();
        std::fs::write(
            plugin.join("readme.txt"),
            "=== Shop tools ===\r\nStable tag: 0.1.0\r\nLicense: GPLv2\r\n",
        )
        .unwrap();
        let found = read(ProjectKind::WordpressPlugin, &plugin).unwrap();
        assert_eq!(found.main_file.as_deref(), Some("shop-tools.php"));
        assert_eq!(found.name.as_deref(), Some("Shop tools"));
        assert_eq!(found.version.as_deref(), Some("0.1.0"));

        let now = set_version(ProjectKind::WordpressPlugin, &plugin, " 1.2.0 ").unwrap();
        assert_eq!(now.version.as_deref(), Some("1.2.0"));
        let readme = std::fs::read_to_string(plugin.join("readme.txt")).unwrap();
        assert_eq!(
            readme,
            "=== Shop tools ===\r\nStable tag: 1.2.0\r\nLicense: GPLv2\r\n"
        );
        let main = std::fs::read_to_string(plugin.join("shop-tools.php")).unwrap();
        assert!(main.contains(" * Version: 1.2.0\n"), "{main}");
        assert!(main.contains(" * Plugin Name: Shop tools\n"), "{main}");

        assert!(set_version(ProjectKind::WordpressPlugin, &plugin, "1.0; rm").is_err());
        assert!(set_version(ProjectKind::WordpressPlugin, &plugin, "").is_err());
    }

    #[test]
    fn adds_a_version_line_when_the_header_has_none() {
        let home = crate::testutil::tempdir();
        let theme = home.join("calm");
        std::fs::create_dir_all(&theme).unwrap();
        std::fs::write(
            theme.join("style.css"),
            "/*\nTheme Name: Calm\nAuthor: Me\n*/\n",
        )
        .unwrap();
        assert_eq!(
            read(ProjectKind::WordpressTheme, &theme).unwrap().version,
            None
        );
        set_version(ProjectKind::WordpressTheme, &theme, "2.0").unwrap();
        let style = std::fs::read_to_string(theme.join("style.css")).unwrap();
        assert_eq!(
            style,
            "/*\nTheme Name: Calm\nVersion: 2.0\nAuthor: Me\n*/\n"
        );

        // A header on the comment's own line keeps its closing */.
        assert_eq!(
            replace_header("/* Version:1.0 */\n", "Version", "1.1").unwrap(),
            "/* Version: 1.1 */\n"
        );
    }

    #[test]
    fn packs_one_folder_and_leaves_out_build_files() {
        let home = crate::testutil::tempdir();
        let theme = home.join("calm");
        projects::scaffold(ProjectKind::WordpressTheme, &theme, "Calm").unwrap();
        for (path, text) in [
            ("assets/js/app.js", "1"),
            ("node_modules/x/index.js", "1"),
            (".git/HEAD", "1"),
            ("src/app.ts", "1"),
            ("notes.md", "1"),
            ("old.zip", "1"),
            ("tests/a_test.php", "1"),
            ("languages/tests/kept.po", "1"),
            (
                ".distignore",
                "/src\nnotes.md\n/tests/\n# a comment\n*.map\n",
            ),
            ("assets/js/app.js.map", "1"),
        ] {
            let path = theme.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let out = builds_dir(&theme);
        assert_eq!(out, theme.join("dist"));
        let built = build_zip(ProjectKind::WordpressTheme, &theme, &out).unwrap();
        assert_eq!(built.path, out.join("calm-0.1.0.zip"));
        let names = names(&built.path);
        for kept in [
            "calm/",
            "calm/style.css",
            "calm/index.php",
            "calm/assets/",
            "calm/assets/js/",
            "calm/assets/js/app.js",
            "calm/languages/tests/kept.po",
        ] {
            assert!(
                names.contains(&kept.to_string()),
                "{kept} missing from {names:?}"
            );
        }
        for left in [
            "node_modules",
            ".git",
            "src/",
            "notes.md",
            "old.zip",
            "calm/tests/",
            ".map",
            ".distignore",
        ] {
            assert!(
                !names.iter().any(|n| n.contains(left)),
                "{left} packed: {names:?}"
            );
        }
        assert!(!out.join("calm-0.1.0.zip.partial").exists());
        assert_eq!(built.files, 7, "{names:?}");

        assert!(std::fs::read_to_string(out.join(".gitignore"))
            .unwrap()
            .contains("*.zip"));

        // Building again replaces the zip, and the zips and the .gitignore
        // in dist/ never go in it; a stale .partial neither.
        std::fs::write(out.join("calm-0.0.9.zip.partial"), "x").unwrap();
        let again = build_zip(ProjectKind::WordpressTheme, &theme, &out).unwrap();
        assert_eq!(again.files, 7);
        assert!(!super::tests::names(&again.path)
            .iter()
            .any(|n| n.contains("dist")));

        // Assets a build tool puts in dist/ are still packed.
        std::fs::write(out.join("app.css"), "a{}").unwrap();
        let with_assets = build_zip(ProjectKind::WordpressTheme, &theme, &out).unwrap();
        let packed = super::tests::names(&with_assets.path);
        assert!(
            packed.contains(&"calm/dist/app.css".to_string()),
            "{packed:?}"
        );
        assert!(!packed
            .iter()
            .any(|n| n.ends_with(".zip") || n.ends_with("/.gitignore")));
    }

    #[test]
    fn will_not_pack_without_a_header() {
        let home = crate::testutil::tempdir();
        let plugin = home.join("empty");
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(plugin.join("a.php"), "<?php").unwrap();
        assert!(build_zip(ProjectKind::WordpressPlugin, &plugin, &home.join("b")).is_err());
        assert!(read(ProjectKind::Website, &plugin).is_err());
    }

    #[test]
    fn wildcards() {
        assert!(wildcard("*.map", "app.js.map"));
        assert!(!wildcard("*.map", "a/b.map"));
        assert!(wildcard("src/*.ts", "src/a.ts"));
        assert!(!wildcard("src/*.ts", "src/x/a.ts"));
        assert!(wildcard("?.md", "a.md"));
    }
}
