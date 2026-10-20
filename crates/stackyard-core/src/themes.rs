//! Color themes people add to the app's window.
//!
//! A theme is `themes/<id>.json` in Stackyard's home: a name, whether it is
//! dark or light, and a `colors` table. The table uses the same keys as a VS
//! Code color theme (`editor.background`, `sideBar.background`, ...), so a VS
//! Code theme file works as it is; the window maps those keys onto its own
//! colors. [`import`] copies a theme in from a `.json` file or from a VS Code
//! extension (`.vsix`), which may hold several.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::paths;

/// A theme file is a few kilobytes; anything far larger is not one.
const MAX_THEME: u64 = 2 * 1024 * 1024;
/// Extensions carry images and code beside their themes.
const MAX_EXTENSION: u64 = 64 * 1024 * 1024;
/// How many `include`s deep a theme may build on another.
const MAX_INCLUDES: usize = 8;
/// Names the window uses for its own themes.
const BUILT_IN: [&str; 4] = ["dark", "light", "high-contrast", "system"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ThemeFile {
    /// The file name without `.json`; what `theme` in settings.toml names.
    pub id: String,
    pub name: String,
    /// `dark`, `light` or `high-contrast`: which built-in theme fills in the
    /// colors this one leaves out.
    pub kind: String,
    pub colors: BTreeMap<String, String>,
}

/// A file that could not be used, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ThemeProblem {
    pub file: String,
    pub problem: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ThemeFiles {
    pub folder: PathBuf,
    pub themes: Vec<ThemeFile>,
    pub problems: Vec<ThemeProblem>,
}

pub fn folder() -> PathBuf {
    paths::home().join("themes")
}

/// Reads every theme in [`folder`]; a missing folder is no themes.
pub fn load() -> ThemeFiles {
    load_from(&folder())
}

pub fn load_from(dir: &Path) -> ThemeFiles {
    let mut out = ThemeFiles {
        folder: dir.to_path_buf(),
        ..Default::default()
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("json"))
        })
        .collect();
    files.sort();
    for path in files {
        let file = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let id = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let result = if BUILT_IN.contains(&id.as_str()) {
            Err(format!(
                "{id} is the name of a built-in theme; rename the file"
            ))
        } else {
            read_theme(&mut Disk, &path).map(|t| ThemeFile {
                id,
                name: t.name.unwrap_or_else(|| title(&path)),
                kind: t.kind,
                colors: t.colors,
            })
        };
        match result {
            Ok(theme) => out.themes.push(theme),
            Err(problem) => out.problems.push(ThemeProblem { file, problem }),
        }
    }
    out
}

/// Copies the theme in `path` (a `.json` theme or a `.vsix` extension) into
/// [`folder`] and returns the ids of the themes added. Importing a theme with
/// the same name again replaces it.
pub fn import(path: &Path) -> Result<Vec<String>, String> {
    import_into(path, &folder())
}

pub fn import_into(path: &Path, dir: &Path) -> Result<Vec<String>, String> {
    let is_vsix = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("vsix"));
    let found = if is_vsix {
        read_extension(path)?
    } else {
        let theme = read_theme(&mut Disk, path)?;
        vec![(theme.name.clone().unwrap_or_else(|| title(path)), theme)]
    };
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut ids = Vec::new();
    for (name, theme) in found {
        let id = free_id(&name, &ids);
        let file = serde_json::json!({ "name": name, "type": theme.kind, "colors": theme.colors });
        let text = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
        write_atomic(&dir.join(format!("{id}.json")), &text).map_err(|e| e.to_string())?;
        ids.push(id);
    }
    Ok(ids)
}

/// Deletes an added theme.
pub fn remove(id: &str) -> Result<(), String> {
    remove_from(id, &folder())
}

pub fn remove_from(id: &str, dir: &Path) -> Result<(), String> {
    if id.is_empty() || id.contains(['/', '\\']) || id.starts_with('.') {
        return Err(format!("no theme named {id:?}"));
    }
    std::fs::remove_file(dir.join(format!("{id}.json"))).map_err(|e| e.to_string())
}

/// A theme as read, before it has an id.
#[derive(Debug, PartialEq)]
struct Theme {
    name: Option<String>,
    kind: String,
    colors: BTreeMap<String, String>,
}

/// Where a theme's file and the files it includes are read from.
trait Source {
    fn read(&mut self, path: &Path) -> Result<String, String>;
}

struct Disk;

impl Source for Disk {
    fn read(&mut self, path: &Path) -> Result<String, String> {
        let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
        if size > MAX_THEME {
            return Err("the file is larger than 2 MB".into());
        }
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        String::from_utf8(bytes).map_err(|_| "the file isn't text".into())
    }
}

/// Files inside a `.vsix`, which is a zip archive.
struct Zip(zip::ZipArchive<std::fs::File>);

impl Source for Zip {
    fn read(&mut self, path: &Path) -> Result<String, String> {
        let name = zip_name(path)
            .ok_or_else(|| format!("{} points outside the extension", path.display()))?;
        let entry = self
            .0
            .by_name(&name)
            .map_err(|_| format!("the extension has no {name}"))?;
        if entry.size() > MAX_THEME {
            return Err(format!("{name} is larger than 2 MB"));
        }
        let mut text = String::new();
        entry
            .take(MAX_THEME)
            .read_to_string(&mut text)
            .map_err(|e| format!("{name}: {e}"))?;
        Ok(text)
    }
}

/// `a/./b/../c` as `a/c`, the way names are stored in a zip; None if it
/// climbs out of the archive.
fn zip_name(path: &Path) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for part in path.to_string_lossy().split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            p => parts.push(p.to_owned()),
        }
    }
    Some(parts.join("/"))
}

/// Reads a theme and the themes it builds on through `include`; its own
/// colors win over theirs.
fn read_theme(source: &mut dyn Source, path: &Path) -> Result<Theme, String> {
    let mut chain = Vec::new();
    let mut next = Some(path.to_path_buf());
    while let Some(at) = next.take() {
        if chain.len() == MAX_INCLUDES {
            return Err("too many themes include one another".into());
        }
        let text = source.read(&at)?;
        let value =
            parse_jsonc(&text).map_err(|e| format!("{}: not valid JSON: {e}", at.display()))?;
        let Value::Object(object) = value else {
            return Err(format!("{}: a theme must be a JSON object", at.display()));
        };
        if let Some(Value::String(include)) = object.get("include") {
            next = Some(at.parent().unwrap_or(Path::new("")).join(include));
        }
        chain.push(object);
    }
    let own = &chain[0];
    let mut colors = BTreeMap::new();
    for object in chain.iter().rev() {
        if let Some(Value::Object(table)) = object.get("colors") {
            for (key, value) in table {
                match value {
                    Value::String(c) if is_color(c) => {
                        colors.insert(key.clone(), c.to_ascii_lowercase());
                    }
                    // VS Code clears an inherited color with null.
                    Value::Null => {
                        colors.remove(key);
                    }
                    _ => {}
                }
            }
        }
    }
    let name = own
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_owned);
    let kind = chain
        .iter()
        .find_map(|o| o.get("type").and_then(Value::as_str))
        .map(kind_of)
        .unwrap_or_else(|| guess_kind(&colors));
    Ok(Theme { name, kind, colors })
}

/// Every color theme a VS Code extension contributes, with its label.
fn read_extension(path: &Path) -> Result<Vec<(String, Theme)>, String> {
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > MAX_EXTENSION {
        return Err("the extension is larger than 64 MB".into());
    }
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let archive = zip::ZipArchive::new(file)
        .map_err(|_| "this isn't a VS Code extension (.vsix) file".to_string())?;
    let mut zip = Zip(archive);
    let package = zip
        .read(Path::new("extension/package.json"))
        .map_err(|_| "this isn't a VS Code extension (.vsix) file".to_string())?;
    let package = parse_jsonc(&package).map_err(|e| format!("extension/package.json: {e}"))?;
    // Labels like "%theme.label%" are looked up in package.nls.json.
    let strings = zip
        .read(Path::new("extension/package.nls.json"))
        .ok()
        .and_then(|t| parse_jsonc(&t).ok());
    let entries = package
        .pointer("/contributes/themes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if entries.is_empty() {
        return Err("this extension has no color themes".into());
    }
    let mut out = Vec::new();
    for entry in &entries {
        let Some(theme_path) = entry.get("path").and_then(Value::as_str) else {
            continue;
        };
        let mut theme = read_theme(&mut zip, &Path::new("extension").join(theme_path))?;
        if let Some(ui) = entry.get("uiTheme").and_then(Value::as_str) {
            theme.kind = kind_of(ui);
        }
        let label = entry
            .get("label")
            .and_then(Value::as_str)
            .map(|l| localized(l, strings.as_ref()))
            .filter(|l| !l.trim().is_empty())
            .or_else(|| theme.name.clone())
            .unwrap_or_else(|| title(Path::new(theme_path)));
        out.push((label, theme));
    }
    if out.is_empty() {
        return Err("this extension has no color themes".into());
    }
    Ok(out)
}

fn localized(label: &str, strings: Option<&Value>) -> String {
    match label.strip_prefix('%').and_then(|l| l.strip_suffix('%')) {
        Some(key) => strings
            .and_then(|s| s.get(key))
            .and_then(Value::as_str)
            .unwrap_or(key)
            .to_owned(),
        None => label.to_owned(),
    }
}

/// VS Code's names for a theme's base: `vs-dark`, `vs`, `hc-black`, `hc-light`
/// in extensions, `dark`, `light`, `hc`, `hcLight` in theme files.
fn kind_of(name: &str) -> String {
    match name.to_ascii_lowercase().as_str() {
        "vs" | "light" | "hclight" | "hc-light" => "light",
        "hc" | "hc-black" | "hcdark" | "high-contrast" => "high-contrast",
        _ => "dark",
    }
    .to_owned()
}

/// A theme that doesn't say: light when its background is.
fn guess_kind(colors: &BTreeMap<String, String>) -> String {
    let light = colors
        .get("editor.background")
        .and_then(|c| luminance(c))
        .is_some_and(|l| l > 0.5);
    if light { "light" } else { "dark" }.to_owned()
}

fn luminance(color: &str) -> Option<f64> {
    let hex = color.strip_prefix('#')?;
    let digit = |i: usize, n: usize| {
        u8::from_str_radix(&hex[i..i + n], 16)
            .ok()
            .map(|v| if n == 1 { v * 17 } else { v })
    };
    let (r, g, b) = match hex.len() {
        3 | 4 => (digit(0, 1)?, digit(1, 1)?, digit(2, 1)?),
        6 | 8 => (digit(0, 2)?, digit(2, 2)?, digit(4, 2)?),
        _ => return None,
    };
    Some((0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64) / 255.0)
}

/// `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`, the forms VS Code themes use.
fn is_color(value: &str) -> bool {
    value.strip_prefix('#').is_some_and(|h| {
        matches!(h.len(), 3 | 4 | 6 | 8) && h.chars().all(|c| c.is_ascii_hexdigit())
    })
}

/// JSON with `//` and `/* */` comments and trailing commas, as VS Code
/// writes its theme files.
fn parse_jsonc(text: &str) -> Result<Value, String> {
    let text = text.trim_start_matches('\u{feff}');
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if c == '\\' {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match (c, chars.peek()) {
            ('"', _) => {
                in_string = true;
                out.push(c);
            }
            ('/', Some('/')) => {
                while chars.peek().is_some_and(|&n| n != '\n') {
                    chars.next();
                }
            }
            ('/', Some('*')) => {
                chars.next();
                let mut last = ' ';
                for n in chars.by_ref() {
                    if last == '*' && n == '/' {
                        break;
                    }
                    // Keep line breaks so error positions still match the file.
                    if n == '\n' {
                        out.push('\n');
                    }
                    last = n;
                }
                out.push(' ');
            }
            _ => out.push(c),
        }
    }
    let cleaned = drop_trailing_commas(&out);
    serde_json::from_str(&cleaned).map_err(|e| e.to_string())
}

/// Removes a comma that only has white space between it and `}` or `]`.
fn drop_trailing_commas(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 1;
            } else if c == '"' {
                in_string = false;
            }
        } else if c == '"' {
            in_string = true;
            out.push(c);
        } else if c == ',' {
            let next = chars[i + 1..].iter().find(|n| !n.is_whitespace());
            if !matches!(next, Some('}') | Some(']')) {
                out.push(c);
            }
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

/// A readable name from a file name: `one-dark-pro.json` is "One Dark Pro".
fn title(path: &Path) -> String {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let stem = stem
        .trim_end_matches("-color-theme")
        .trim_end_matches("-theme");
    stem.split(['-', '_', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A file name for a theme called `name`, not one of `taken` or a built-in.
fn free_id(name: &str, taken: &[String]) -> String {
    let mut slug = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    let base = if slug.is_empty() {
        "theme".to_owned()
    } else if BUILT_IN.contains(&slug) {
        format!("{slug}-theme")
    } else {
        slug.chars()
            .take(60)
            .collect::<String>()
            .trim_end_matches('-')
            .to_owned()
    };
    let mut id = base.clone();
    let mut n = 2;
    while taken.contains(&id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn jsonc_comments_and_trailing_commas() {
        let value = parse_jsonc(
            "\u{feff}{\n  // a comment\n  \"a\": \"http://x\", /* b */\n  \"list\": [1, 2,],\n  \"s\": \"// not a comment, \\\" ,}\",\n}",
        )
        .unwrap();
        assert_eq!(value["a"], "http://x");
        assert_eq!(value["list"], serde_json::json!([1, 2]));
        assert_eq!(value["s"], "// not a comment, \" ,}");
    }

    #[test]
    fn colors_kind_and_includes() {
        let dir = crate::testutil::tempdir();
        std::fs::write(
            dir.join("base.json"),
            r##"{ "type": "light", "colors": { "editor.background": "#FFFFFF", "focusBorder": "#0000ff", "badge.background": "#ff0000" } }"##,
        )
        .unwrap();
        std::fs::write(
            dir.join("child.json"),
            r##"{ "name": "Child", "include": "./base.json",
                 "colors": { "editor.background": "#eeeeee", "badge.background": null, "sideBar.border": "red", "x": 3 } }"##,
        )
        .unwrap();
        let theme = read_theme(&mut Disk, &dir.join("child.json")).unwrap();
        assert_eq!(theme.name.as_deref(), Some("Child"));
        assert_eq!(theme.kind, "light");
        let keys: Vec<_> = theme
            .colors
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        assert_eq!(keys, ["editor.background=#eeeeee", "focusBorder=#0000ff"]);
    }

    #[test]
    fn kind_is_guessed_from_the_background() {
        let mut colors = BTreeMap::new();
        colors.insert("editor.background".to_owned(), "#fafafa".to_owned());
        assert_eq!(guess_kind(&colors), "light");
        colors.insert("editor.background".to_owned(), "#282a36".to_owned());
        assert_eq!(guess_kind(&colors), "dark");
        assert_eq!(kind_of("hc-black"), "high-contrast");
        assert_eq!(kind_of("vs"), "light");
    }

    #[test]
    fn imports_a_json_theme_and_lists_it() {
        let dir = crate::testutil::tempdir();
        let source = dir.join("dracula-color-theme.json");
        std::fs::write(
            &source,
            r##"{ "colors": { "editor.background": "#282a36" } }"##,
        )
        .unwrap();
        let themes = dir.join("themes");
        assert_eq!(import_into(&source, &themes).unwrap(), ["dracula"]);
        std::fs::write(themes.join("dark.json"), "{}").unwrap();
        std::fs::write(themes.join("broken.json"), "{ oops").unwrap();

        let files = load_from(&themes);
        assert_eq!(files.themes.len(), 1);
        let theme = &files.themes[0];
        assert_eq!(
            (theme.id.as_str(), theme.name.as_str(), theme.kind.as_str()),
            ("dracula", "Dracula", "dark")
        );
        assert_eq!(theme.colors["editor.background"], "#282a36");
        let bad: Vec<_> = files.problems.iter().map(|p| p.file.as_str()).collect();
        assert_eq!(bad, ["broken.json", "dark.json"]);

        remove_from("dracula", &themes).unwrap();
        assert!(load_from(&themes).themes.is_empty());
        assert!(remove_from("../settings", &themes).is_err());
    }

    #[test]
    fn imports_every_theme_in_an_extension() {
        let dir = crate::testutil::tempdir();
        let vsix = dir.join("one-dark.vsix");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&vsix).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        let files = [
            (
                "extension/package.json",
                r#"{ "contributes": { "themes": [
                    { "label": "%dark%", "uiTheme": "vs-dark", "path": "./themes/dark.json" },
                    { "label": "One Light", "uiTheme": "vs", "path": "./themes/light.json" } ] } }"#,
            ),
            ("extension/package.nls.json", r#"{ "dark": "One Dark" }"#),
            (
                "extension/themes/dark.json",
                r##"{ "include": "../shared/base.json", "colors": { "editor.background": "#282c34" } }"##,
            ),
            (
                "extension/shared/base.json",
                r##"{ "colors": { "focusBorder": "#528bff" } }"##,
            ),
            (
                "extension/themes/light.json",
                r##"{ "colors": { "editor.background": "#fafafa" } }"##,
            ),
        ];
        for (name, text) in files {
            zip.start_file(name, options).unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        zip.finish().unwrap();

        let themes = dir.join("themes");
        assert_eq!(
            import_into(&vsix, &themes).unwrap(),
            ["one-dark", "one-light"]
        );
        let files = load_from(&themes);
        let dark = &files.themes[0];
        assert_eq!(
            (dark.name.as_str(), dark.kind.as_str()),
            ("One Dark", "dark")
        );
        assert_eq!(dark.colors["focusBorder"], "#528bff");
        assert_eq!(files.themes[1].kind, "light");
    }

    #[test]
    fn not_an_extension() {
        let dir = crate::testutil::tempdir();
        let path = dir.join("x.vsix");
        std::fs::write(&path, "plain text").unwrap();
        assert!(import_into(&path, &dir.join("themes"))
            .unwrap_err()
            .contains(".vsix"));
    }

    /// Popular themes from Open VSX import with the colors the window needs.
    #[test]
    #[ignore]
    fn live_imports_popular_themes() {
        let http = crate::download::Http::new();
        let dir = crate::testutil::tempdir();
        for (ext, wanted) in [
            ("dracula-theme/theme-dracula", "dracula"),
            ("zhuangtongfa/material-theme", "one-dark-pro"),
            ("GitHub/github-vscode-theme", "github-dark-default"),
        ] {
            let info: Value = serde_json::from_str(
                &http
                    .get_text(&format!("https://open-vsx.org/api/{ext}/latest"))
                    .unwrap(),
            )
            .unwrap();
            let url = info["files"]["download"].as_str().unwrap();
            let sum = http
                .get_text(info["files"]["sha256"].as_str().unwrap())
                .unwrap();
            let sum = sum.split_whitespace().next().unwrap();
            let vsix = dir.join(format!("{wanted}.vsix"));
            http.download(url, &vsix, sum, &mut |_, _| {}).unwrap();
            let ids = import_into(&vsix, &dir.join("themes")).unwrap();
            println!("{ext}: {ids:?}");
            assert!(
                ids.iter().any(|id| id.starts_with(wanted)),
                "{ext}: {ids:?}"
            );
        }
        let files = load_from(&dir.join("themes"));
        assert!(files.problems.is_empty(), "{:?}", files.problems);
        for theme in &files.themes {
            for key in ["editor.background", "editor.foreground"] {
                assert!(theme.colors.contains_key(key), "{} has no {key}", theme.id);
            }
        }
    }

    #[test]
    fn ids_and_names() {
        assert_eq!(free_id("One Dark Pro", &[]), "one-dark-pro");
        assert_eq!(free_id("Dark", &[]), "dark-theme");
        assert_eq!(free_id("!!!", &[]), "theme");
        assert_eq!(free_id("Nord", &["nord".to_owned()]), "nord-2");
        assert_eq!(
            title(Path::new("one-dark-pro-color-theme.json")),
            "One Dark Pro"
        );
        assert_eq!(
            zip_name(Path::new("extension/themes/../shared/./a.json")).as_deref(),
            Some("extension/shared/a.json")
        );
        assert_eq!(zip_name(Path::new("../a.json")), None);
    }
}
