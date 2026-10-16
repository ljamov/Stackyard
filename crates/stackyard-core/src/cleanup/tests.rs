use super::*;
use crate::install::write_receipt_for_tests;
use crate::sites::{Site, SiteStore};

fn write(path: &Path, bytes: usize) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, vec![b'x'; bytes]).unwrap();
}

fn age(path: &Path, by: Duration) {
    let mut options = std::fs::File::options();
    options.write(true);
    #[cfg(windows)]
    {
        // Folders open only with backup semantics.
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0200_0000);
    }
    let file = if path.is_dir() && cfg!(unix) {
        std::fs::File::open(path)
    } else {
        options.open(path)
    };
    file.unwrap().set_modified(SystemTime::now() - by).unwrap();
}

fn ids(report: &Report) -> Vec<&str> {
    report.items.iter().map(|i| i.id.as_str()).collect()
}

fn add_site(home: &Path, name: &str, runtime: SiteRuntime) -> PathBuf {
    let path = home.join("code").join(name);
    std::fs::create_dir_all(&path).unwrap();
    let mut store = SiteStore::load_from(&home.join("sites.toml")).unwrap();
    store
        .add(Site {
            name: name.into(),
            tld: ".stack".into(),
            path: path.clone(),
            runtime,
            https: true,
            dev_server: false,
        })
        .unwrap();
    store.save().unwrap();
    path
}

#[test]
fn an_empty_home_has_nothing_to_clean() {
    let home = crate::testutil::tempdir();
    let report = scan(&home);
    assert!(report.items.is_empty(), "{report:?}");
    assert!(report.usage.is_empty());
}

#[test]
fn finds_downloads_leftovers_and_old_logs_but_not_fresh_ones() {
    let home = crate::testutil::tempdir();
    let old = Duration::from_secs(3600);
    write(&home.join("downloads/php-8.4.tar.xz"), 1000);
    age(&home.join("downloads/php-8.4.tar.xz"), old);
    // Still being downloaded.
    write(&home.join("downloads/node-22.tar.xz.part"), 50);
    write(&home.join("runtimes/php/.staging-8.4-1-0/bin/php"), 300);
    age(&home.join("runtimes/php/.staging-8.4-1-0"), old);
    write(&home.join("services/mysql/.trash-8.4-1-1/bin/mysqld"), 200);
    age(&home.join("services/mysql/.trash-8.4-1-1"), old);
    write(&home.join("logs/caddy.log"), 40);
    write(&home.join("logs/caddy.log.1"), 70);

    let report = scan(&home);
    assert_eq!(ids(&report), ["downloads", "leftovers", "old-logs", "logs"]);
    let size = |id: &str| report.items.iter().find(|i| i.id == id).unwrap().size;
    assert_eq!(size("downloads"), 1000);
    assert_eq!(size("leftovers"), 500);
    assert_eq!(size("old-logs"), 70);
    assert_eq!(size("logs"), 40);
    assert!(report.items[..3]
        .iter()
        .all(|i| i.recommended && !i.permanent));
    assert!(!report.items[3].recommended);
    assert_eq!(report.usage[0].name, "Downloads");
    assert_eq!(report.total, 1000 + 50 + 500 + 40 + 70);

    let ids: Vec<String> = ["downloads", "leftovers", "old-logs", "logs"]
        .map(String::from)
        .to_vec();
    let outcome = clean(&home, &ids);
    assert_eq!(outcome.problems, Vec::<String>::new());
    assert_eq!(outcome.freed, 1610);
    assert!(home.join("downloads/node-22.tar.xz.part").exists());
    assert!(!home.join("downloads/php-8.4.tar.xz").exists());
    assert!(!home.join("runtimes/php/.staging-8.4-1-0").exists());
    assert!(!home.join("logs/caddy.log.1").exists());
    // Emptied, not removed.
    assert_eq!(
        std::fs::metadata(home.join("logs/caddy.log"))
            .unwrap()
            .len(),
        0
    );
    assert!(scan(&home).items.is_empty());

    let again = clean(&home, &["downloads".to_string()]);
    assert_eq!(again.freed, 0);
    assert_eq!(again.problems.len(), 1);
}

#[test]
fn keeps_runtimes_that_anything_relies_on() {
    let home = crate::testutil::tempdir();
    for (kind, version) in [
        ("php", "8.4"),
        ("php", "8.3"),
        ("php", "8.2"),
        ("php", "7.3"),
        ("node", "24"),
        ("node", "22"),
    ] {
        let dir = home.join("runtimes").join(kind).join(version);
        write_receipt_for_tests(&dir, &format!("{version}.1"));
        write(&dir.join("bin/program"), 10);
    }
    let mut settings = Settings::default();
    settings.set_default_runtime(RuntimeKind::Php, Some("8.4"));
    settings.save_to(&home.join("settings.toml")).unwrap();
    add_site(
        &home,
        "shop",
        SiteRuntime::Node {
            version: "24".into(),
            port: 5173,
        },
    );
    let blog = add_site(
        &home,
        "blog",
        SiteRuntime::Php {
            version: "8.4".into(),
        },
    );
    std::fs::write(blog.join(pins::PIN_FILE), "php = \"8.3\"\n").unwrap();

    let report = scan(&home);
    assert_eq!(
        ids(&report),
        ["runtime-php-8.2", "runtime-node-22", "runtime-php-7.3"]
    );
    assert!(!report.items[0].recommended);
    assert!(report.items[2].recommended, "retired versions can go");

    let outcome = clean(&home, &["runtime-php-8.2".into(), "runtime-php-7.3".into()]);
    assert_eq!(outcome.problems, Vec::<String>::new());
    assert_eq!(outcome.cleaned, ["PHP 8.2", "PHP 7.3"]);
    assert!(!home.join("runtimes/php/8.2").exists());
    assert!(home.join("runtimes/php/8.3").exists());
}

#[test]
fn a_site_on_a_retired_version_keeps_it() {
    let home = crate::testutil::tempdir();
    write(&home.join("runtimes/php/7.3/bin/php"), 10);
    add_site(
        &home,
        "legacy",
        SiteRuntime::Php {
            version: "7.3".into(),
        },
    );
    assert!(scan(&home).items.is_empty());
}

#[test]
fn removed_services_leave_data_and_old_lines_to_clean() {
    let home = crate::testutil::tempdir();
    write(&home.join("data/mariadb/db/ibdata1"), 900);
    write(&home.join("data/mysql/db/ibdata1"), 400);
    write_receipt_for_tests(&home.join("services/mysql/8.4"), "8.4.6");
    write(&home.join("services/mysql/8.0/bin/mysqld"), 300);

    let report = scan(&home);
    assert_eq!(ids(&report), ["service-mysql-8.0", "data-mariadb"]);
    let data = &report.items[1];
    assert!(data.permanent && !data.recommended);
    assert_eq!(data.size, 900);

    let outcome = clean(&home, &["data-mariadb".into()]);
    assert_eq!(outcome.freed, 900);
    assert!(!home.join("data/mariadb").exists());
    assert!(home.join("data/mysql/db/ibdata1").exists());
}

#[cfg(unix)]
#[test]
fn sizes_never_follow_links() {
    let home = crate::testutil::tempdir();
    let outside = crate::testutil::tempdir();
    write(&outside.join("big"), 5000);
    write(&home.join("logs/a.log"), 10);
    std::os::unix::fs::symlink(&outside, home.join("logs/elsewhere")).unwrap();
    assert!(size_of(&home) < 100);
}
