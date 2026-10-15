use super::*;

#[test]
fn quotes_names_and_strings() {
    assert_eq!(ident("shop"), "`shop`");
    assert_eq!(ident("we`ird"), "`we``ird`");
    assert_eq!(literal("it's"), "'it''s'");
    assert_eq!(literal(r"a\b"), r"'a\\b'");
    assert!(check_name("shop_2-test").is_ok());
    for bad in ["", "a b", "a.b", "x;DROP", "ü", &"x".repeat(65)] {
        assert!(check_name(bad).is_err(), "{bad}");
    }
    assert!(check_user_database("MySQL").is_err());
    assert!(check_user_database("shop").is_ok());
}

#[test]
fn explains_client_errors() {
    let e = |s: &str| client_error(s, "mysql").to_string();
    assert_eq!(
        e("ERROR 1049 (42000): Unknown database 'nope'\n"),
        "Unknown database 'nope'"
    );
    assert_eq!(
        e("ERROR 1064 (42000) at line 1: You have an error in your SQL syntax"),
        "You have an error in your SQL syntax"
    );
    assert!(e("ERROR 2002 (HY000): Can't connect to local server through socket")
        .starts_with("MySQL is not running"));
    assert!(client_error("ERROR 2003 (HY000): Can't connect", "mariadb")
        .to_string()
        .contains("stackyard start mariadb"));
    assert_eq!(e("mysqldump: Got error: 1044: Access denied"), "mysqldump: Got error: 1044: Access denied");
}

#[test]
fn backup_paths_stay_in_their_folder() {
    let home = crate::testutil::tempdir();
    let dir = backups_dir(&home).join("shop");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("shop-20261014-010203.sql.gz"), b"x").unwrap();
    std::fs::write(dir.join(".shop-x.sql.gz.partial"), b"x").unwrap();
    assert!(backup_path(&home, "shop", "shop-20261014-010203.sql.gz").is_ok());
    for (db, file) in [
        ("shop", "../shop/shop-20261014-010203.sql.gz"),
        ("..", "x.sql.gz"),
        ("shop", "missing.sql.gz"),
        ("shop", ".shop-x.sql.gz.partial"),
    ] {
        assert!(backup_path(&home, db, file).is_err(), "{db}/{file}");
    }
    let all = backups(&home);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].database, "shop");
    remove_backup(&home, "shop", "shop-20261014-010203.sql.gz").unwrap();
    assert!(backups(&home).is_empty());
    assert_eq!(stamp().len(), 15);
}

/// Everything the screen does, against a real server that `home` knows.
pub(crate) fn exercise(home: &Path) {
    let client = Client::open(home).unwrap();
    let server = client.server();
    assert!(server.version.is_some(), "{server:?}");

    client.create("stackyard_test").unwrap();
    assert!(client.create("stackyard_test").is_err());
    let sets = client
        .query(
            Some("stackyard_test"),
            "CREATE TABLE people (id INT PRIMARY KEY, name VARCHAR(50), note TEXT);\n\
             INSERT INTO people VALUES (1, 'Ana & <Bo>', NULL), (2, 'line\\nbreak', '');\n\
             SELECT * FROM people ORDER BY id; SELECT COUNT(*) AS n FROM people;",
        )
        .unwrap();
    assert_eq!(sets.len(), 2, "{sets:?}");
    assert_eq!(sets[0].columns, ["id", "name", "note"]);
    assert_eq!(sets[0].rows[0], [Some("1".into()), Some("Ana & <Bo>".into()), None]);
    assert_eq!(sets[0].rows[1][1].as_deref(), Some("line\nbreak"));
    assert_eq!(sets[0].rows[1][2].as_deref(), Some(""));
    assert_eq!(sets[1].rows[0][0].as_deref(), Some("2"));

    let err = client.query(Some("stackyard_test"), "SELEC 1").unwrap_err();
    assert!(err.to_string().contains("SQL syntax"), "{err}");

    let dbs = client.databases().unwrap();
    let db = dbs.iter().find(|d| d.name == "stackyard_test").unwrap();
    assert_eq!(db.tables, 1);
    assert!(!dbs.iter().any(|d| d.name == "mysql" || d.name == "sys"));
    let tables = client.tables("stackyard_test").unwrap();
    assert_eq!(tables[0].name, "people");
    let page = client.browse("stackyard_test", "people", 1, 10).unwrap();
    assert_eq!(page.rows.len(), 1);
    let empty = client.browse("stackyard_test", "people", 10, 10).unwrap();
    assert!(empty.rows.is_empty());
    assert_eq!(empty.columns, ["id", "name", "note"]);

    let many = client
        .query(
            None,
            "SELECT a.n FROM (SELECT 1 AS n UNION SELECT 2 UNION SELECT 3 UNION SELECT 4 UNION SELECT 5 UNION SELECT 6 UNION SELECT 7 UNION SELECT 8 UNION SELECT 9 UNION SELECT 10 UNION SELECT 11) a \
             CROSS JOIN (SELECT 1 UNION SELECT 2 UNION SELECT 3 UNION SELECT 4 UNION SELECT 5 UNION SELECT 6 UNION SELECT 7 UNION SELECT 8 UNION SELECT 9 UNION SELECT 10) b \
             CROSS JOIN (SELECT 1 UNION SELECT 2 UNION SELECT 3 UNION SELECT 4 UNION SELECT 5 UNION SELECT 6 UNION SELECT 7 UNION SELECT 8 UNION SELECT 9 UNION SELECT 10) c",
        )
        .unwrap();
    assert_eq!(many[0].rows.len(), MAX_ROWS);
    assert!(many[0].truncated);

    // Back up, lose the data, restore it; and restore a copy elsewhere.
    let backup = client.backup("stackyard_test").unwrap();
    assert!(backup.size > 0);
    assert_eq!(backups(home)[0], backup);
    client.drop("stackyard_test").unwrap();
    assert!(!client.databases().unwrap().iter().any(|d| d.name == "stackyard_test"));
    client.restore("stackyard_test", &backup.file, None).unwrap();
    client
        .restore("stackyard_test", &backup.file, Some("stackyard_copy"))
        .unwrap();
    for db in ["stackyard_test", "stackyard_copy"] {
        let rows = client
            .query(Some(db), "SELECT name FROM people ORDER BY id")
            .unwrap();
        assert_eq!(rows[0].rows[0][0].as_deref(), Some("Ana & <Bo>"), "{db}");
    }
    assert!(client.drop("mysql").is_err());
    client.drop("stackyard_test").unwrap();
    client.drop("stackyard_copy").unwrap();
}

/// Runs [`exercise`] against a MariaDB installed by the system package
/// manager, laid out the way Stackyard installs one. Set
/// `STACKYARD_TEST_SYSTEM_MARIADB=1` to run it; it needs `mariadbd`,
/// `mariadb`, `mariadb-dump` and `mariadb-install-db` on PATH.
#[cfg(unix)]
#[test]
fn against_a_system_mariadb() {
    if std::env::var_os("STACKYARD_TEST_SYSTEM_MARIADB").is_none() {
        return;
    }
    use std::os::unix::fs::symlink;
    let home = crate::testutil::tempdir();
    let install = home.join("services/mariadb/11.8");
    std::fs::create_dir_all(install.join("bin")).unwrap();
    crate::install::write_receipt_for_tests(&install, "11.8.0");
    for tool in ["mariadb", "mariadb-dump"] {
        symlink(format!("/usr/bin/{tool}"), install.join("bin").join(tool)).unwrap();
    }
    let mut settings = crate::settings::Settings::default();
    settings.set_module("mariadb", true).unwrap();
    settings.save_to(&home.join("settings.toml")).unwrap();

    let data = home.join("data/mariadb");
    let db = data.join("db");
    std::fs::create_dir_all(&data).unwrap();
    let ok = std::process::Command::new("mariadb-install-db")
        .args(["--no-defaults", "--auth-root-authentication-method=normal", "--skip-test-db"])
        .arg(format!("--datadir={}", db.display()))
        .output()
        .unwrap();
    assert!(ok.status.success(), "{}", String::from_utf8_lossy(&ok.stderr));
    let socket = data.join("mysql.sock");
    let mut server = std::process::Command::new("mariadbd")
        .arg("--no-defaults")
        .arg(format!("--datadir={}", db.display()))
        .arg(format!("--socket={}", socket.display()))
        .args(["--skip-networking", "--user=root"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    for _ in 0..100 {
        if socket.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let result = std::panic::catch_unwind(|| exercise(&home));
    let _ = server.kill();
    let _ = server.wait();
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}
