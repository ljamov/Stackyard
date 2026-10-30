//! Projects: what a site is for, picked when it is made and changeable
//! later.
//!
//! Most projects are websites, served from their own folder. A WordPress
//! theme or plugin is written in a folder of its own and tried in a
//! WordPress made for it: the site's folder is that WordPress, and the
//! theme's or plugin's folder is linked into its `wp-content`, so a change
//! saved in one shows in the other at once. The editor, the terminal and
//! Git open the theme's or plugin's own folder ([`folder`]).
//!
//! The WordPress for a theme or plugin lives in Stackyard's data folder,
//! `projects/<name>/wordpress`, out of the way of the project. Changing a
//! theme back into a website unlinks it and serves its folder again; the
//! WordPress is kept, so changing it back once more finds it ready.
//!
//! The link is a symbolic link on macOS and Linux and a directory junction
//! on Windows, which needs no administrator rights. Removing a site's
//! folder removes a link, never what it points to: Rust's `remove_dir_all`
//! does not follow links or junctions.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::sites::{Site, SiteDatabase, SiteRuntime};
use crate::starters::{self, Starter};
use crate::wordpress::cli::{self, Load};
use crate::{Error, Result};

fn fail(message: impl Into<String>) -> Error {
    Error::Project(message.into())
}

/// What a project is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectKind {
    /// A website or web app: WordPress, Laravel, a static site, a Node app
    /// with its dev server, and so on. What it runs is found in its folder.
    #[default]
    Website,
    /// A WordPress theme, tried in a WordPress made for it.
    WordpressTheme,
    /// A WordPress plugin, tried in a WordPress made for it.
    WordpressPlugin,
    /// A PHP library or package, served with PHP so it can be tried in a
    /// page of its own.
    PhpPackage,
    /// A Node.js app or package, run with its own dev server.
    NodeApp,
}

impl ProjectKind {
    pub const ALL: [ProjectKind; 5] = [
        ProjectKind::Website,
        ProjectKind::WordpressTheme,
        ProjectKind::WordpressPlugin,
        ProjectKind::PhpPackage,
        ProjectKind::NodeApp,
    ];

    pub fn id(self) -> &'static str {
        match self {
            ProjectKind::Website => "website",
            ProjectKind::WordpressTheme => "wordpress-theme",
            ProjectKind::WordpressPlugin => "wordpress-plugin",
            ProjectKind::PhpPackage => "php-package",
            ProjectKind::NodeApp => "node-app",
        }
    }

    pub fn from_id(id: &str) -> Option<ProjectKind> {
        ProjectKind::ALL.into_iter().find(|k| k.id() == id)
    }

    pub fn name(self) -> &'static str {
        match self {
            ProjectKind::Website => "Website",
            ProjectKind::WordpressTheme => "WordPress theme",
            ProjectKind::WordpressPlugin => "WordPress plugin",
            ProjectKind::PhpPackage => "PHP package",
            ProjectKind::NodeApp => "Node.js app",
        }
    }

    /// A theme or a plugin: tried in a WordPress of its own.
    pub fn in_wordpress(self) -> bool {
        matches!(
            self,
            ProjectKind::WordpressTheme | ProjectKind::WordpressPlugin
        )
    }

    /// The folder in `wp-content` a theme or plugin is linked into.
    fn content_dir(self) -> Option<&'static str> {
        match self {
            ProjectKind::WordpressTheme => Some("themes"),
            ProjectKind::WordpressPlugin => Some("plugins"),
            _ => None,
        }
    }
}

/// Which list an item is in: a project (scripts, a theme or plugin,
/// plain files), an app, or a site. It only sorts and labels; every one is
/// served and opened the same way, and it can be changed at any time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Project,
    App,
    Site,
}

impl Category {
    pub const ALL: [Category; 3] = [Category::Project, Category::App, Category::Site];

    pub fn id(self) -> &'static str {
        match self {
            Category::Project => "project",
            Category::App => "app",
            Category::Site => "site",
        }
    }

    pub fn from_id(id: &str) -> Option<Category> {
        Category::ALL.into_iter().find(|c| c.id() == id)
    }
}

/// What a site is for, kept with its settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    #[serde(default)]
    pub kind: ProjectKind,
    /// A theme's or plugin's own folder, linked into the WordPress in the
    /// site's folder. `None` for the other kinds, whose folder is the site's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<PathBuf>,
    /// The list it is in; `None` (sites saved before there were lists)
    /// follows from the kind, see [`Project::category`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<Category>,
}

impl Project {
    pub fn is_default(&self) -> bool {
        *self == Project::default()
    }

    /// The list it is in: the one picked, else a theme, plugin or package
    /// is a project, a Node.js app an app, and the rest sites.
    pub fn category(&self) -> Category {
        self.category.unwrap_or(match self.kind {
            ProjectKind::WordpressTheme
            | ProjectKind::WordpressPlugin
            | ProjectKind::PhpPackage => Category::Project,
            ProjectKind::NodeApp => Category::App,
            ProjectKind::Website => Category::Site,
        })
    }
}

/// The project's own folder: what the editor, the terminal and Git open.
pub fn folder(site: &Site) -> &Path {
    site.settings
        .project
        .source
        .as_deref()
        .unwrap_or(&site.path)
}

/// Where the WordPress made for a theme or plugin project lives.
pub fn wordpress_dir(home: &Path, name: &str) -> PathBuf {
    home.join("projects").join(name).join("wordpress")
}

/// A theme's or plugin's slug: its folder's name.
fn slug(source: &Path) -> Result<String> {
    source
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .filter(|n| !n.is_empty())
        .ok_or_else(|| fail(format!("{} has no folder name", source.display())))
}

/// Where the link to the theme or plugin goes in the WordPress at `wp`.
fn link_path(kind: ProjectKind, wp: &Path, source: &Path) -> Result<PathBuf> {
    let dir = kind
        .content_dir()
        .ok_or_else(|| fail(format!("a {} is not tried in WordPress", kind.name())))?;
    Ok(wp.join("wp-content").join(dir).join(slug(source)?))
}

/// Links `link` to the folder `target`.
fn make_link(target: &Path, link: &Path) -> Result<()> {
    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent).map_err(|source| Error::Io {
            path: parent.to_owned(),
            source,
        })?;
    }
    if std::fs::symlink_metadata(link).is_ok() {
        return Err(fail(format!(
            "{} is already there; remove it or rename the project's folder",
            link.display()
        )));
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).map_err(|source| Error::Io {
            path: link.to_owned(),
            source,
        })
    }
    #[cfg(windows)]
    {
        // A junction, unlike a symbolic link, needs neither administrator
        // rights nor Developer Mode; `mklink` is the plain way to make one.
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let out = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|source| Error::Io {
                path: link.to_owned(),
                source,
            })?;
        if out.status.success() {
            Ok(())
        } else {
            Err(fail(format!(
                "could not link {} to {}: {}",
                link.display(),
                target.display(),
                String::from_utf8_lossy(&out.stderr).trim()
            )))
        }
    }
}

/// Removes the link at `link`, never what it points to. Does nothing when
/// it is gone, and refuses a real folder.
fn remove_link(link: &Path) -> Result<()> {
    let Ok(meta) = std::fs::symlink_metadata(link) else {
        return Ok(());
    };
    if !is_link(&meta) {
        return Err(fail(format!(
            "{} is a folder, not a link to the project; leaving it alone",
            link.display()
        )));
    }
    // A directory junction or a symbolic link to a folder goes with
    // remove_dir on Windows, a symbolic link with remove_file elsewhere.
    #[cfg(windows)]
    let removed = std::fs::remove_dir(link).or_else(|_| std::fs::remove_file(link));
    #[cfg(not(windows))]
    let removed = std::fs::remove_file(link);
    removed.map_err(|source| Error::Io {
        path: link.to_owned(),
        source,
    })
}

fn is_link(meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const REPARSE_POINT: u32 = 0x400;
        meta.file_type().is_symlink() || meta.file_attributes() & REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

fn is_empty_dir(folder: &Path) -> bool {
    folder
        .read_dir()
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(true)
}

/// Writes a minimal theme or plugin into `folder` when it is missing or
/// empty, named after the project; a folder with files is left as it is.
pub fn scaffold(kind: ProjectKind, folder: &Path, title: &str) -> Result<()> {
    if !is_empty_dir(folder) {
        return Ok(());
    }
    let slug = slug(folder)?;
    let title = title.replace(['\r', '\n', '*'], " ");
    let files: Vec<(String, String)> = match kind {
        ProjectKind::WordpressTheme => vec![
            (
                "style.css".into(),
                format!(
                    "/*\nTheme Name: {title}\nText Domain: {slug}\nVersion: 0.1.0\n*/\n\nbody {{\n\tmargin: 0 auto;\n\tmax-width: 48rem;\n\tpadding: 2rem;\n\tfont-family: system-ui, sans-serif;\n}}\n"
                ),
            ),
            (
                "functions.php".into(),
                format!(
                    "<?php\n// {title}: theme setup.\n\nadd_action( 'wp_enqueue_scripts', function () {{\n\twp_enqueue_style( '{slug}', get_stylesheet_uri(), array(), wp_get_theme()->get( 'Version' ) );\n}} );\n\nadd_theme_support( 'title-tag' );\n"
                ),
            ),
            (
                "index.php".into(),
                "<?php get_header(); ?>\n<main>\n<?php while ( have_posts() ) : the_post(); ?>\n\t<article>\n\t\t<h2><a href=\"<?php the_permalink(); ?>\"><?php the_title(); ?></a></h2>\n\t\t<?php the_content(); ?>\n\t</article>\n<?php endwhile; ?>\n</main>\n<?php get_footer(); ?>\n".into(),
            ),
            (
                "header.php".into(),
                "<!doctype html>\n<html <?php language_attributes(); ?>>\n<head>\n<meta charset=\"<?php bloginfo( 'charset' ); ?>\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<?php wp_head(); ?>\n</head>\n<body <?php body_class(); ?>>\n<?php wp_body_open(); ?>\n<header><h1><a href=\"<?php echo esc_url( home_url( '/' ) ); ?>\"><?php bloginfo( 'name' ); ?></a></h1></header>\n".into(),
            ),
            (
                "footer.php".into(),
                "<?php wp_footer(); ?>\n</body>\n</html>\n".into(),
            ),
        ],
        ProjectKind::WordpressPlugin => vec![(
            format!("{slug}.php"),
            format!(
                "<?php\n/**\n * Plugin Name: {title}\n * Version: 0.1.0\n * Text Domain: {slug}\n */\n\nif ( ! defined( 'ABSPATH' ) ) {{\n\texit;\n}}\n"
            ),
        )],
        _ => Vec::new(),
    };
    for (name, text) in files {
        let path = folder.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| Error::Io {
                path: parent.to_owned(),
                source,
            })?;
        }
        std::fs::write(&path, text).map_err(|source| Error::Io { path, source })?;
    }
    Ok(())
}

/// What a theme or plugin project runs on, once its WordPress is ready.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ready {
    pub path: PathBuf,
    pub runtime: SiteRuntime,
    pub database: SiteDatabase,
}

/// Makes the WordPress for a theme or plugin at `source`, or finds the one
/// made before, links `source` into it and switches the theme or plugin
/// on. MySQL or MariaDB must be running. `url` is the site's address.
pub fn prepare_wordpress(
    home: &Path,
    name: &str,
    kind: ProjectKind,
    source: &Path,
    url: &str,
    progress: &mut dyn FnMut(String),
) -> Result<Ready> {
    let wp = wordpress_dir(home, name);
    let database = starters::database_name(name);
    let fresh = !wp.join("wp-config.php").is_file();
    let runtime = if fresh {
        std::fs::create_dir_all(&wp).map_err(|source| Error::Io {
            path: wp.clone(),
            source,
        })?;
        let made = starters::create(home, Starter::WordPress, name, &wp, progress)?;
        made.runtime
    } else {
        SiteRuntime::Php {
            version: starters::php_version(home)?,
        }
    };
    let link = link_path(kind, &wp, source)?;
    if std::fs::symlink_metadata(&link).is_err() {
        make_link(source, &link)?;
    }
    let wp_cli = |load: Load, args: &[&str]| {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        cli::run(home, &wp, load, &args)
    };
    if fresh {
        progress("Installing WordPress…".into());
        let mut bytes = [0u8; 18];
        getrandom::fill(&mut bytes).map_err(|e| fail(format!("no randomness: {e}")))?;
        let password: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let host = url.split("://").nth(1).unwrap_or(url).trim_end_matches('/');
        wp_cli(
            Load::Core,
            &[
                "core",
                "install",
                &format!("--url={url}"),
                &format!("--title={name}"),
                "--admin_user=admin",
                &format!("--admin_password={password}"),
                &format!("--admin_email=admin@{host}"),
                "--skip-email",
            ],
        )?;
    }
    progress(format!("Switching the {} on…", kind.name()));
    let slug = slug(source)?;
    match kind {
        ProjectKind::WordpressTheme => wp_cli(Load::Core, &["theme", "activate", &slug])?,
        _ => wp_cli(Load::Core, &["plugin", "activate", &slug])?,
    };
    Ok(Ready {
        path: wp,
        runtime,
        database: SiteDatabase {
            server: "mysql".into(),
            name: database,
        },
    })
}

/// Changes what `site` is. A theme or plugin needs MySQL or MariaDB
/// running, for its WordPress. `site` is changed in place; the caller
/// saves it.
pub fn change(
    home: &Path,
    site: &mut Site,
    to: ProjectKind,
    progress: &mut dyn FnMut(String),
) -> Result<()> {
    let from = site.settings.project.kind;
    if from == to {
        return Ok(());
    }
    let own = folder(site).to_path_buf();
    // Out of WordPress first: unlink, and serve the project's folder again.
    if from.in_wordpress() {
        remove_link(&link_path(from, &site.path, &own)?)?;
        if !to.in_wordpress() {
            site.path = own.clone();
            site.settings.project.source = None;
            site.settings.database = None;
        }
    }
    if to.in_wordpress() {
        let url = site.url();
        let ready = prepare_wordpress(home, &site.name, to, &own, &url, progress)?;
        site.path = ready.path;
        site.runtime = ready.runtime;
        site.settings.database = Some(ready.database);
        site.settings.project.source = Some(own);
        site.settings.root = None;
    }
    site.settings.project.kind = to;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_go_to_ids_and_back() {
        for kind in ProjectKind::ALL {
            assert_eq!(ProjectKind::from_id(kind.id()), Some(kind));
        }
        let saved = toml::to_string(&Project {
            kind: ProjectKind::WordpressTheme,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(saved.trim(), "kind = \"wordpress-theme\"");
    }

    #[test]
    fn lists_follow_the_kind_until_one_is_picked() {
        let mut project = Project::default();
        assert_eq!(project.category(), Category::Site);
        project.kind = ProjectKind::WordpressPlugin;
        assert_eq!(project.category(), Category::Project);
        project.kind = ProjectKind::NodeApp;
        assert_eq!(project.category(), Category::App);
        project.category = Some(Category::Site);
        assert_eq!(project.category(), Category::Site);
        let saved = toml::to_string(&project).unwrap();
        assert!(saved.contains("category = \"site\""), "{saved}");
        for c in Category::ALL {
            assert_eq!(Category::from_id(c.id()), Some(c));
        }
    }

    #[test]
    fn scaffolds_only_an_empty_folder() {
        let home = crate::testutil::tempdir();
        let theme = home.join("my-theme");
        scaffold(ProjectKind::WordpressTheme, &theme, "My Theme").unwrap();
        let style = std::fs::read_to_string(theme.join("style.css")).unwrap();
        assert!(style.contains("Theme Name: My Theme"));
        assert!(theme.join("index.php").is_file());

        let plugin = home.join("shop-tools");
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(plugin.join("mine.php"), "<?php // mine").unwrap();
        scaffold(ProjectKind::WordpressPlugin, &plugin, "Shop tools").unwrap();
        assert!(!plugin.join("shop-tools.php").exists());
    }

    /// The link is how the WordPress sees the project; removing the
    /// WordPress, or the link, must never reach the project's files.
    #[test]
    fn removing_the_wordpress_leaves_the_project_alone() {
        let home = crate::testutil::tempdir();
        let source = home.join("code").join("my-plugin");
        scaffold(ProjectKind::WordpressPlugin, &source, "My plugin").unwrap();
        let wp = home.join("wp");
        let link = link_path(ProjectKind::WordpressPlugin, &wp, &source).unwrap();
        make_link(&source, &link).unwrap();
        assert!(link.join("my-plugin.php").is_file(), "the link works");
        assert!(make_link(&source, &link).is_err(), "never over a link");

        std::fs::remove_dir_all(&wp).unwrap();
        assert!(source.join("my-plugin.php").is_file());

        make_link(&source, &link).unwrap();
        remove_link(&link).unwrap();
        assert!(std::fs::symlink_metadata(&link).is_err());
        assert!(source.join("my-plugin.php").is_file());
    }

    #[test]
    fn will_not_remove_a_real_folder_as_a_link() {
        let home = crate::testutil::tempdir();
        let real = home.join("wp-content").join("plugins").join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("a.php"), "").unwrap();
        assert!(remove_link(&real).is_err());
        assert!(real.join("a.php").is_file());
    }

    #[test]
    fn a_website_has_its_own_folder_and_a_theme_its_source() {
        let mut site = Site {
            name: "shop".into(),
            tld: ".stack".into(),
            path: PathBuf::from("/data/projects/shop/wordpress"),
            runtime: SiteRuntime::Static,
            https: true,
            dev_server: false,
            settings: Default::default(),
        };
        assert_eq!(folder(&site), Path::new("/data/projects/shop/wordpress"));
        site.settings.project = Project {
            kind: ProjectKind::WordpressTheme,
            source: Some(PathBuf::from("/code/shop-theme")),
            category: None,
        };
        assert_eq!(folder(&site), Path::new("/code/shop-theme"));
    }
}
