//! Translation files the user adds for the app's screens.
//!
//! The app ships its own languages. Anyone can add one, or fix a word in a
//! shipped one, by putting `<code>.json` (e.g. `nl.json` or `pt-BR.json`) in
//! the `languages` folder of Stackyard's home. The file maps the English text
//! to the translation; `"_name"` holds the language's own name for the menu.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::paths;

/// Larger files are skipped; a full translation is a few dozen kilobytes.
const MAX_FILE: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LanguageFile {
    /// Language code from the file name, e.g. `nl` or `pt-BR`.
    pub code: String,
    /// The language's own name, from `"_name"`; the code when missing.
    pub name: String,
    pub strings: BTreeMap<String, String>,
}

/// A file that could not be used, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LanguageProblem {
    pub file: String,
    pub problem: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct LanguageFiles {
    pub folder: PathBuf,
    pub languages: Vec<LanguageFile>,
    pub problems: Vec<LanguageProblem>,
}

pub fn folder() -> PathBuf {
    paths::home().join("languages")
}

/// Reads every translation file in [`folder`]; a missing folder is no files.
pub fn load() -> LanguageFiles {
    load_from(&folder())
}

pub fn load_from(dir: &Path) -> LanguageFiles {
    let mut out = LanguageFiles {
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
        match read(&path) {
            Ok(language) => out.languages.push(language),
            Err(problem) => out.problems.push(LanguageProblem { file, problem }),
        }
    }
    out
}

fn read(path: &Path) -> Result<LanguageFile, String> {
    let code = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    if !valid_code(&code) {
        return Err("name the file after a language code, like nl.json or pt-BR.json".into());
    }
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > MAX_FILE {
        return Err("the file is larger than 1 MB".into());
    }
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut strings: BTreeMap<String, String> =
        serde_json::from_str(text.trim_start_matches('\u{feff}'))
            .map_err(|e| format!("not valid JSON: {e}"))?;
    let name = strings
        .remove("_name")
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| code.clone());
    strings.retain(|key, value| !key.starts_with('_') && !value.is_empty());
    Ok(LanguageFile {
        code,
        name,
        strings,
    })
}

/// `xx`, `xxx`, or either followed by `-` parts such as `pt-BR` or `zh-Hant`.
fn valid_code(code: &str) -> bool {
    let mut parts = code.split('-');
    let first = parts.next().unwrap_or_default();
    (2..=3).contains(&first.len())
        && first.chars().all(|c| c.is_ascii_alphabetic())
        && parts.all(|p| (2..=8).contains(&p.len()) && p.chars().all(|c| c.is_ascii_alphanumeric()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes() {
        for ok in ["nl", "pt-BR", "zh-Hant", "fil"] {
            assert!(valid_code(ok), "{ok}");
        }
        for bad in ["", "n", "dutch", "nl_", "nl-", "../x", "nl-B"] {
            assert!(!valid_code(bad), "{bad}");
        }
    }

    #[test]
    fn reads_files_and_reports_bad_ones() {
        let dir = std::env::temp_dir().join(format!("stackyard-languages-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("nl.json"),
            "\u{feff}{\"_name\":\"Nederlands\",\"Settings\":\"Instellingen\",\"Sites\":\"\"}",
        )
        .unwrap();
        std::fs::write(dir.join("pt-BR.json"), "{\"Sites\":\"Sites\"}").unwrap();
        std::fs::write(dir.join("dutch.json"), "{}").unwrap();
        std::fs::write(dir.join("fr.json"), "{oops").unwrap();
        std::fs::write(dir.join("notes.txt"), "ignored").unwrap();

        let files = load_from(&dir);
        std::fs::remove_dir_all(&dir).unwrap();

        let nl = &files.languages[0];
        assert_eq!((nl.code.as_str(), nl.name.as_str()), ("nl", "Nederlands"));
        assert_eq!(nl.strings.len(), 1, "empty values fall back to English");
        assert_eq!(files.languages[1].name, "pt-BR");
        let bad: Vec<_> = files.problems.iter().map(|p| p.file.as_str()).collect();
        assert_eq!(bad, ["dutch.json", "fr.json"]);
    }

    #[test]
    fn missing_folder_is_empty() {
        let files = load_from(Path::new("/no/such/stackyard/languages"));
        assert!(files.languages.is_empty() && files.problems.is_empty());
    }
}
