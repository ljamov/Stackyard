//! Works out what a project folder holds: which framework, which folder the
//! web server should serve, and whether it needs PHP, Node, Python, Go, Java
//! or nothing.
//!
//! Detection only looks for marker files; it never runs project code.

use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase", tag = "kind")]
pub enum Needs {
    Php,
    /// A Node dev server, usually listening on this port.
    Node {
        port: u16,
    },
    /// A Python, Go or Java app whose server Stackyard runs ([`crate::apps`]).
    App {
        runtime: crate::catalog::RuntimeKind,
        port: u16,
    },
    Static,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Detected {
    /// e.g. `Laravel`; `None` for a plain folder.
    pub framework: Option<&'static str>,
    /// The folder to serve, relative to the project (empty = the project itself).
    pub root: PathBuf,
    pub needs: Needs,
}

/// PHP projects: (framework, files that must all exist, folder to serve).
const PHP: &[(&str, &[&str], &str)] = &[
    ("Laravel", &["artisan", "public/index.php"], "public"),
    ("Symfony", &["bin/console", "public/index.php"], "public"),
    ("CodeIgniter", &["spark", "public/index.php"], "public"),
    ("Magento", &["bin/magento", "pub/index.php"], "pub"),
    ("CakePHP", &["bin/cake", "webroot/index.php"], "webroot"),
    ("Craft CMS", &["craft", "web/index.php"], "web"),
    ("Yii", &["yii", "web/index.php"], "web"),
    ("Drupal", &["web/core/lib/Drupal.php"], "web"),
    ("Drupal", &["core/lib/Drupal.php"], ""),
    ("WordPress", &["web/wp/wp-load.php"], "web"),
    ("WordPress", &["wp-load.php"], ""),
    ("PHP", &["public/index.php"], "public"),
    ("PHP", &["index.php"], ""),
];

/// Node projects: (framework, package that marks it, its usual dev port).
/// The first match wins, so meta-frameworks come before the tools they use.
const NODE: &[(&str, &str, u16)] = &[
    ("Next.js", "next", 3000),
    ("Nuxt", "nuxt", 3000),
    ("Astro", "astro", 4321),
    ("SvelteKit", "@sveltejs/kit", 5173),
    ("Remix", "@remix-run/dev", 5173),
    ("Angular", "@angular/core", 4200),
    ("Gatsby", "gatsby", 8000),
    ("Create React App", "react-scripts", 3000),
    ("Vite", "vite", 5173),
    ("Express", "express", 3000),
];

pub fn detect(project: &Path) -> Detected {
    for (framework, markers, root) in PHP {
        if markers.iter().all(|m| project.join(m).is_file()) {
            let framework = (*framework != "PHP").then_some(*framework);
            return Detected {
                framework,
                root: PathBuf::from(root),
                needs: Needs::Php,
            };
        }
    }
    // Before Node: these projects often have a package.json for their CSS.
    if let Some(app) = crate::apps::detect(project) {
        return Detected {
            framework: Some(app.framework),
            root: PathBuf::new(),
            needs: Needs::App {
                runtime: app.kind,
                port: app.port,
            },
        };
    }
    if let Some((framework, port)) = node_project(project) {
        return Detected {
            framework,
            root: PathBuf::new(),
            needs: Needs::Node { port },
        };
    }
    // Plain files: serve `public/` or `dist/` when that is where the page is.
    let root = ["public", "dist"]
        .into_iter()
        .find(|dir| {
            !project.join("index.html").is_file() && project.join(dir).join("index.html").is_file()
        })
        .unwrap_or("");
    Detected {
        framework: None,
        root: PathBuf::from(root),
        needs: Needs::Static,
    }
}

/// A Node project needs a dev server only when it has a `dev` or `start`
/// script; otherwise its files are served as they are.
fn node_project(project: &Path) -> Option<(Option<&'static str>, u16)> {
    let text = std::fs::read_to_string(project.join("package.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let scripts = json.get("scripts")?;
    let script = ["dev", "start"]
        .into_iter()
        .find_map(|name| scripts.get(name)?.as_str())?;
    let has = |package: &str| {
        ["dependencies", "devDependencies"]
            .iter()
            .any(|section| json.get(section).and_then(|s| s.get(package)).is_some())
    };
    let found = NODE.iter().find(|(_, package, _)| has(package));
    let usual = found.map_or(3000, |(_, _, port)| *port);
    Some((
        found.map(|(name, _, _)| *name),
        port_in_script(script).unwrap_or(usual),
    ))
}

/// `vite --port 4000`, `next dev -p 4000` or `--port=4000` → 4000.
fn port_in_script(script: &str) -> Option<u16> {
    let words: Vec<&str> = script.split_whitespace().collect();
    words.iter().enumerate().find_map(|(i, word)| {
        if let Some(value) = word.strip_prefix("--port=") {
            return value.parse().ok();
        }
        if matches!(*word, "--port" | "-p") {
            return words.get(i + 1)?.parse().ok();
        }
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(files: &[(&str, &str)]) -> PathBuf {
        let dir = crate::testutil::tempdir();
        for (path, text) in files {
            let path = dir.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        dir
    }

    #[test]
    fn finds_php_frameworks_and_their_public_folder() {
        let laravel = detect(&project(&[("artisan", ""), ("public/index.php", "")]));
        assert_eq!(laravel.framework, Some("Laravel"));
        assert_eq!(laravel.root, PathBuf::from("public"));
        assert_eq!(laravel.needs, Needs::Php);

        let craft = detect(&project(&[("craft", ""), ("web/index.php", "")]));
        assert_eq!(craft.framework, Some("Craft CMS"));
        assert_eq!(craft.root, PathBuf::from("web"));

        let wordpress = detect(&project(&[("wp-load.php", ""), ("index.php", "")]));
        assert_eq!(wordpress.framework, Some("WordPress"));
        assert_eq!(wordpress.root, PathBuf::new());

        let plain = detect(&project(&[("index.php", "")]));
        assert_eq!(plain.framework, None);
        assert_eq!(plain.needs, Needs::Php);
    }

    #[test]
    fn finds_node_dev_servers_and_ports() {
        let vite = detect(&project(&[(
            "package.json",
            r#"{"scripts": {"dev": "vite"}, "devDependencies": {"vite": "^7"}}"#,
        )]));
        assert_eq!(vite.framework, Some("Vite"));
        assert_eq!(vite.needs, Needs::Node { port: 5173 });

        let next = detect(&project(&[(
            "package.json",
            r#"{"scripts": {"dev": "next dev -p 4100"}, "dependencies": {"next": "16", "react": "19"}}"#,
        )]));
        assert_eq!(next.framework, Some("Next.js"));
        assert_eq!(next.needs, Needs::Node { port: 4100 });

        // A package.json with no dev server is just files.
        let lib = detect(&project(&[
            ("package.json", r#"{"scripts": {"test": "x"}}"#),
            ("public/index.html", ""),
        ]));
        assert_eq!(lib.needs, Needs::Static);
        assert_eq!(lib.root, PathBuf::from("public"));
    }

    #[test]
    fn reads_ports_from_scripts() {
        assert_eq!(port_in_script("vite --port 4000"), Some(4000));
        assert_eq!(port_in_script("astro dev --port=4400 --host"), Some(4400));
        assert_eq!(port_in_script("next dev"), None);
        assert_eq!(port_in_script("x -p nope"), None);
    }

    #[test]
    fn empty_and_missing_folders_are_static() {
        let empty = detect(&project(&[]));
        assert_eq!(empty.needs, Needs::Static);
        assert_eq!(empty.root, PathBuf::new());
        assert_eq!(detect(Path::new("/no/such/folder")).needs, Needs::Static);
    }
}
