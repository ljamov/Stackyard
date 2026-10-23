use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Instant;

use super::*;
use crate::runtimes::RuntimeManager;
use crate::sites::{Site, SiteRuntime, SiteStore};

/// A Stackyard folder with the MCP server switched on.
fn home() -> PathBuf {
    let home = crate::testutil::tempdir();
    let mut settings = Settings::default();
    settings.set_module("mcp", true).unwrap();
    settings.save_to(&home.join("settings.toml")).unwrap();
    home
}

fn change_settings(home: &Path, change: impl FnOnce(&mut Settings)) {
    let path = home.join("settings.toml");
    let mut settings = Settings::load_from(&path).unwrap();
    change(&mut settings);
    settings.save_to(&path).unwrap();
}

fn server(home: &Path) -> Server {
    Server::new(home, Path::new("/no/such/stackyard"))
}

fn ask(server: &mut Server, method: &str, params: Value) -> Value {
    let line = json!({ "jsonrpc": "2.0", "id": 7, "method": method, "params": params });
    let answer: Value =
        serde_json::from_str(&server.handle_line(&line.to_string()).unwrap()).unwrap();
    assert_eq!(answer["id"], 7);
    answer
}

fn call(server: &mut Server, tool: &str, args: Value) -> Value {
    let answer = ask(
        server,
        "tools/call",
        json!({ "name": tool, "arguments": args }),
    );
    answer["result"].clone()
}

/// The answer of a call that worked.
fn ok(server: &mut Server, tool: &str, args: Value) -> Value {
    let result = call(server, tool, args);
    assert_eq!(result["isError"], false, "{result}");
    result["structuredContent"].clone()
}

/// The message of a call that failed.
fn failed(server: &mut Server, tool: &str, args: Value) -> String {
    let result = call(server, tool, args);
    assert_eq!(result["isError"], true, "{result}");
    result["content"][0]["text"].as_str().unwrap().to_string()
}

fn tool_names(server: &mut Server) -> Vec<String> {
    let answer = ask(server, "tools/list", json!({}));
    answer["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn initializes_with_a_version_both_sides_speak() {
    let home = home();
    let mut server = server(&home);
    let answer = ask(
        &mut server,
        "initialize",
        json!({ "protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": { "name": "claude-code", "version": "2" } }),
    );
    let result = &answer["result"];
    assert_eq!(result["protocolVersion"], "2025-03-26");
    assert_eq!(result["serverInfo"]["name"], "stackyard");
    assert_eq!(result["capabilities"]["tools"]["listChanged"], true);
    assert_eq!(server.client, "claude-code");
    // Older versions get no structured results.
    let listed = call(&mut server, "list_runtimes", json!({}));
    assert!(listed.get("structuredContent").is_none());
    assert!(listed["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("\"php\""));

    let answer = ask(
        &mut server,
        "initialize",
        json!({ "protocolVersion": "1999-01-01" }),
    );
    assert_eq!(answer["result"]["protocolVersion"], PROTOCOL_VERSIONS[0]);
}

#[test]
fn speaks_json_rpc() {
    let home = home();
    let mut server = server(&home);
    let parse = |s: Option<String>| -> Value { serde_json::from_str(&s.unwrap()).unwrap() };

    let answer = parse(server.handle_line("{not json"));
    assert_eq!(answer["error"]["code"], PARSE_ERROR);
    assert_eq!(answer["id"], Value::Null);
    assert_eq!(server.handle_line("   "), None);
    // Notifications and answers get no answer.
    assert_eq!(
        server.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#),
        None
    );
    assert_eq!(
        server.handle_line(r#"{"jsonrpc":"2.0","id":1,"result":{}}"#),
        None
    );

    let answer = ask(&mut server, "resources/list", json!({}));
    assert_eq!(answer["error"]["code"], METHOD_NOT_FOUND);
    assert_eq!(ask(&mut server, "ping", Value::Null)["result"], json!({}));
    let answer = ask(
        &mut server,
        "tools/call",
        json!({ "name": "delete_everything" }),
    );
    assert_eq!(answer["error"]["code"], INVALID_PARAMS);
    let answer = ask(
        &mut server,
        "tools/call",
        json!({ "name": "list_sites", "arguments": [1] }),
    );
    assert_eq!(answer["error"]["code"], INVALID_PARAMS);

    let batch = parse(server.handle_line(
        r#"[{"jsonrpc":"2.0","id":1,"method":"ping"},{"jsonrpc":"2.0","method":"notifications/initialized"},{"jsonrpc":"2.0","id":"b","method":"ping"}]"#,
    ));
    assert_eq!(batch.as_array().unwrap().len(), 2);
    assert_eq!(batch[1]["id"], "b");
    assert_eq!(
        parse(server.handle_line("[]"))["error"]["code"],
        INVALID_REQUEST
    );
    assert_eq!(
        parse(server.handle_line("3"))["error"]["code"],
        INVALID_REQUEST
    );
}

#[test]
fn action_tools_stay_off_until_allowed() {
    let home = home();
    let mut server = server(&home);
    let reads = [
        "list_sites",
        "list_services",
        "list_runtimes",
        "list_databases",
        "read_logs",
        "list_wordpress_environments",
    ];
    assert_eq!(tool_names(&mut server), reads);
    let message = failed(
        &mut server,
        "query_database",
        json!({ "database": "shop", "sql": "SELECT 1" }),
    );
    assert!(message.contains("Settings › AI tools"), "{message}");
    let message = failed(&mut server, "stop_service", json!({ "service": "caddy" }));
    assert!(
        message.contains("stackyard mcp allow stop_service"),
        "{message}"
    );

    change_settings(&home, |s| set_allowed(s, "stop_service", true).unwrap());
    assert_eq!(tool_names(&mut server).len(), reads.len() + 1);
    let stopped = ok(&mut server, "stop_service", json!({ "service": "caddy" }));
    assert_eq!(stopped["state"], "stopped");

    change_settings(&home, |s| set_allowed(s, "all", true).unwrap());
    assert_eq!(tool_names(&mut server).len(), TOOLS.len());
    change_settings(&home, |s| set_allowed(s, "all", false).unwrap());
    assert_eq!(tool_names(&mut server), reads);

    // With the module off, nothing is offered and nothing runs.
    change_settings(&home, |s| s.set_module("mcp", false).unwrap());
    assert!(tool_names(&mut server).is_empty());
    let message = failed(&mut server, "list_sites", json!({}));
    assert!(message.contains("switched off"), "{message}");

    let mut settings = Settings::default();
    assert!(set_allowed(&mut settings, "list_sites", true).is_err());
    assert!(set_allowed(&mut settings, "delete_site", true).is_err());
    assert!(settings.mcp.allow.is_empty());
}

#[test]
fn every_tool_has_a_schema_for_its_arguments() {
    for tool in TOOLS {
        let schema = input_schema(tool.name);
        assert_eq!(schema["type"], "object", "{}", tool.name);
        for name in schema["required"].as_array().unwrap() {
            let name = name.as_str().unwrap();
            assert!(
                schema["properties"].get(name).is_some(),
                "{}: {name}",
                tool.name
            );
        }
    }
}

#[test]
fn lists_sites_services_and_runtimes() {
    let home = home();
    let code = home.join("code");
    let parked = home.join("parked");
    std::fs::create_dir_all(code.join("shop")).unwrap();
    std::fs::write(code.join("shop/index.php"), "<?php").unwrap();
    std::fs::create_dir_all(parked.join("Blog")).unwrap();
    change_settings(&home, |s| {
        s.park(&parked).unwrap();
    });
    let mut store = SiteStore::load_from(&home.join("sites.toml")).unwrap();
    store
        .add(Site {
            name: "shop".into(),
            tld: ".stack".into(),
            path: code.join("shop"),
            runtime: SiteRuntime::Php {
                version: "8.4".into(),
            },
            https: true,
            dev_server: false,
            settings: Default::default(),
        })
        .unwrap();
    store.save().unwrap();

    let mut server = server(&home);
    let listed = ok(&mut server, "list_sites", json!({}));
    let sites = listed["sites"].as_array().unwrap();
    assert_eq!(sites.len(), 2);
    assert_eq!(sites[0]["domain"], "shop.stack");
    assert_eq!(sites[0]["runtime"]["version"], "8.4");
    assert!(sites[0]["problem"]
        .as_str()
        .unwrap()
        .contains("not installed"));
    assert_eq!(sites[1]["domain"], "blog.stack");
    assert_eq!(sites[1]["parked"], true);
    assert_eq!(listed["web_server_running"], false);

    let services = ok(&mut server, "list_services", json!({}));
    let caddy = &services["services"][0];
    assert_eq!(caddy["id"], "caddy");
    assert_eq!(caddy["state"], "stopped");
    assert_eq!(caddy["installed"], Value::Null);
    assert_eq!(services["running_in"], Value::Null);

    let runtimes = ok(&mut server, "list_runtimes", json!({}));
    assert_eq!(runtimes["runtimes"][0]["kind"], "php");
    assert!(runtimes["runtimes"][0]["not_installed"]
        .as_array()
        .unwrap()
        .contains(&json!("8.4")));
}

#[test]
fn reads_only_stackyard_logs() {
    let home = home();
    let mut server = server(&home);
    let empty = ok(&mut server, "read_logs", json!({ "service": "caddy" }));
    assert!(empty["note"].is_string());

    std::fs::create_dir_all(home.join("logs")).unwrap();
    let lines: Vec<String> = (0..600).map(|n| format!("line {n}")).collect();
    std::fs::write(home.join("logs/php-8.4.log"), lines.join("\n")).unwrap();
    let read = ok(
        &mut server,
        "read_logs",
        json!({ "service": "php-8.4", "lines": 2 }),
    );
    assert_eq!(read["lines"], json!(["line 598", "line 599"]));
    let read = ok(
        &mut server,
        "read_logs",
        json!({ "service": "php-8.4", "lines": 100000 }),
    );
    assert_eq!(read["lines"].as_array().unwrap().len(), 500);

    for bad in ["../../secret", "php-../../x", "settings"] {
        let message = failed(&mut server, "read_logs", json!({ "service": bad }));
        assert!(message.contains("no process named"), "{message}");
    }
    let message = failed(&mut server, "read_logs", json!({ "service": 3 }));
    assert!(message.contains("must be a string"), "{message}");
}

#[test]
fn explains_what_it_cannot_start() {
    let home = home();
    change_settings(&home, |s| set_allowed(s, "all", true).unwrap());
    let mut server = server(&home);
    let message = failed(&mut server, "start_service", json!({ "service": "caddy" }));
    assert!(message.contains("not installed"), "{message}");
    let message = failed(
        &mut server,
        "start_service",
        json!({ "service": "mailpit" }),
    );
    assert!(message.contains("switched off"), "{message}");
    let message = failed(&mut server, "start_service", json!({ "service": "nginx" }));
    assert!(message.contains("caddy, mysql"), "{message}");
    let message = failed(&mut server, "start_service", json!({}));
    assert!(message.contains("service is required"), "{message}");
    let message = failed(
        &mut server,
        "restart_service",
        json!({ "service": "caddy" }),
    );
    assert!(message.contains("not installed"), "{message}");
}

#[test]
fn tells_ai_tools_what_each_tool_does_to_data() {
    let home = home();
    change_settings(&home, |s| set_allowed(s, "all", true).unwrap());
    let mut server = server(&home);
    let answer = ask(&mut server, "tools/list", json!({}));
    let hints = |name: &str| {
        answer["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .unwrap()["annotations"]
            .clone()
    };
    assert_eq!(hints("query_database")["readOnlyHint"], true);
    assert_eq!(hints("change_database")["readOnlyHint"], false);
    assert_eq!(hints("change_database")["destructiveHint"], true);
    assert_eq!(hints("restart_service")["destructiveHint"], false);
}

#[test]
fn checks_sql_before_any_database_is_asked() {
    let home = home();
    change_settings(&home, |s| set_allowed(s, "all", true).unwrap());
    let mut server = server(&home);
    let message = failed(
        &mut server,
        "query_database",
        json!({ "database": "shop", "sql": "DELETE FROM users" }),
    );
    assert!(message.contains("change_database"), "{message}");
    let message = failed(
        &mut server,
        "change_database",
        json!({ "database": "shop", "sql": "DROP DATABASE shop" }),
    );
    assert!(message.contains("never allowed"), "{message}");
    let message = failed(&mut server, "query_database", json!({ "database": "shop" }));
    assert!(message.contains("sql is required"), "{message}");
    // Fine SQL, but no database server is installed.
    let message = failed(&mut server, "list_databases", json!({}));
    assert!(message.contains("MySQL or MariaDB"), "{message}");
}

/// The database tools against a real server that `home` knows, with a
/// `stackyard_test` database holding a `people` table.
pub(crate) fn exercise_databases(home: &Path) {
    change_settings(home, |s| {
        s.set_module("mcp", true).unwrap();
        set_allowed(s, "all", true).unwrap();
    });
    let mut server = server(home);
    let listed = ok(&mut server, "list_databases", json!({}));
    assert!(listed["databases"]
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["name"] == "stackyard_test"));
    let tables = ok(
        &mut server,
        "list_databases",
        json!({ "database": "stackyard_test" }),
    );
    assert_eq!(tables["tables"][0]["name"], "people");

    let rows = ok(
        &mut server,
        "query_database",
        json!({ "database": "stackyard_test", "sql": "SELECT id, name FROM people ORDER BY id", "max_rows": 1 }),
    );
    assert_eq!(rows["results"][0]["columns"], json!(["id", "name"]));
    assert_eq!(rows["results"][0]["rows"].as_array().unwrap().len(), 1);
    assert_eq!(rows["results"][0]["truncated"], true);
    // Read-only, even when a function would write.
    let message = failed(
        &mut server,
        "query_database",
        json!({ "database": "stackyard_test", "sql": "WITH x AS (SELECT 1) DELETE FROM people" }),
    );
    assert!(!message.is_empty());

    let changed = ok(
        &mut server,
        "change_database",
        json!({ "database": "stackyard_test", "sql": "UPDATE people SET note = 'seen';\nUPDATE people SET note = 'x' WHERE id = 1" }),
    );
    assert_eq!(changed["changed_rows"], 1, "{changed}");
    let rows = ok(
        &mut server,
        "query_database",
        json!({ "database": "stackyard_test", "sql": "SELECT note FROM people ORDER BY id" }),
    );
    assert_eq!(rows["results"][0]["rows"], json!([["x"], ["seen"]]));
}

#[test]
fn creates_sites_and_switches_versions() {
    let home = home();
    change_settings(&home, |s| set_allowed(s, "all", true).unwrap());
    let code = home.join("code");
    std::fs::create_dir_all(code.join("api")).unwrap();
    std::fs::write(code.join("api/index.php"), "<?php").unwrap();
    std::fs::create_dir_all(code.join("web")).unwrap();
    std::fs::write(
        code.join("web/package.json"),
        r#"{"scripts":{"dev":"vite"},"devDependencies":{"vite":"6"}}"#,
    )
    .unwrap();
    let mut server = server(&home);

    // Detected from the folder.
    let created = ok(
        &mut server,
        "create_site",
        json!({ "name": "api", "folder": code.join("api") }),
    );
    assert_eq!(created["site"]["url"], "https://api.stack");
    assert_eq!(created["site"]["runtime"]["kind"], "php");
    assert!(created["notes"][0]
        .as_str()
        .unwrap()
        .contains("not running"));

    let created = ok(
        &mut server,
        "create_site",
        json!({ "name": "web", "folder": code.join("web"), "version": "22", "port": 5174, "https": false }),
    );
    assert_eq!(created["site"]["url"], "http://web.stack");
    assert_eq!(
        created["site"]["runtime"],
        json!({ "kind": "node", "version": "22", "dev_server_port": 5174 })
    );

    let message = failed(
        &mut server,
        "create_site",
        json!({ "name": "api", "folder": code.join("api") }),
    );
    assert!(message.contains("already exists"), "{message}");
    let message = failed(
        &mut server,
        "create_site",
        json!({ "name": "x", "folder": "code/api" }),
    );
    assert!(message.contains("full path"), "{message}");
    let message = failed(
        &mut server,
        "create_site",
        json!({ "name": "x", "folder": code.join("nope") }),
    );
    assert!(message.contains("not a folder"), "{message}");
    let message = failed(
        &mut server,
        "create_site",
        json!({ "name": "x", "folder": code.join("api"), "runtime": "php", "version": "5.6" }),
    );
    assert!(message.contains("not offered"), "{message}");
    let message = failed(
        &mut server,
        "create_site",
        json!({ "name": "x", "folder": code.join("api"), "runtime": "static", "version": "8.4" }),
    );
    assert!(message.contains("static"), "{message}");
    let message = failed(
        &mut server,
        "create_site",
        json!({ "name": "Not Valid", "folder": code.join("api") }),
    );
    assert!(message.contains("invalid site name"), "{message}");
    assert_eq!(
        SiteStore::load_from(&home.join("sites.toml"))
            .unwrap()
            .list()
            .len(),
        2
    );

    // A site added by hand changes in sites.toml.
    let switched = ok(
        &mut server,
        "set_site_runtime",
        json!({ "site": "api.stack", "version": "8.2" }),
    );
    assert_eq!(switched["site"]["runtime"]["version"], "8.2");
    assert_eq!(switched["saved_in"], json!(home.join("sites.toml")));
    let store = SiteStore::load_from(&home.join("sites.toml")).unwrap();
    assert_eq!(
        store.list()[0].runtime,
        SiteRuntime::Php {
            version: "8.2".into()
        }
    );
    // Node keeps its port.
    let switched = ok(
        &mut server,
        "set_site_runtime",
        json!({ "site": "web", "version": "20" }),
    );
    assert_eq!(switched["site"]["runtime"]["dev_server_port"], 5174);

    // A parked site gets a .stackyard pin.
    let parked = home.join("parked");
    std::fs::create_dir_all(parked.join("blog")).unwrap();
    std::fs::write(parked.join("blog/index.php"), "<?php").unwrap();
    change_settings(&home, |s| {
        s.park(&parked).unwrap();
    });
    let switched = ok(
        &mut server,
        "set_site_runtime",
        json!({ "site": "blog", "version": "8.3" }),
    );
    assert_eq!(switched["site"]["runtime"]["version"], "8.3");
    assert_eq!(
        switched["saved_in"],
        json!(parked.join("blog").join(".stackyard"))
    );
    let pin = crate::shell::pins::pinned(&parked.join("blog"), crate::catalog::RuntimeKind::Php)
        .unwrap()
        .unwrap();
    assert_eq!(pin.version, "8.3");

    let message = failed(
        &mut server,
        "set_site_runtime",
        json!({ "site": "blog", "version": "99" }),
    );
    assert!(message.contains("not offered"), "{message}");
    let message = failed(
        &mut server,
        "set_site_runtime",
        json!({ "site": "nope", "version": "8.3" }),
    );
    assert!(message.contains("no site named"), "{message}");
    // The runtime manager was never asked to install anything.
    assert!(RuntimeManager::new(&home).installed().is_empty());
}

#[test]
fn records_what_each_ai_tool_did() {
    let home = home();
    let mut server = server(&home);
    ask(
        &mut server,
        "initialize",
        json!({ "clientInfo": { "name": "cursor\u{7}" } }),
    );
    ok(&mut server, "list_sites", json!({}));
    failed(
        &mut server,
        "create_site",
        json!({ "name": "x", "folder": "/tmp" }),
    );
    let log = activity::recent(&home, 10);
    assert_eq!(log.len(), 2);
    assert_eq!(log[0].tool, "create_site");
    assert_eq!(log[0].client, "cursor");
    assert!(!log[0].ok);
    assert!(log[0].error.as_deref().unwrap().contains("turned off"));
    assert_eq!(log[0].summary, "x in /tmp");
    assert_eq!(log[1].tool, "list_sites");
    assert!(log[1].ok);
}

#[test]
fn setup_points_at_the_program() {
    let home = crate::testutil::tempdir();
    let setup = client_setup(&home, Path::new("/Applications/My Apps/stackyard"));
    assert_eq!(
        setup.claude_code,
        "claude mcp add --scope user stackyard -- \"/Applications/My Apps/stackyard\" mcp"
    );
    let json: Value = serde_json::from_str(&setup.json).unwrap();
    assert_eq!(json["mcpServers"]["stackyard"]["args"], json!(["mcp"]));

    // The copy the terminal commands use wins: it survives app updates.
    let copy = shims::program_path(&home);
    std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
    std::fs::write(&copy, "").unwrap();
    assert_eq!(client_setup(&home, Path::new("/x")).program, copy);
}

/// Input that arrives when the test sends it, like an AI tool typing.
struct Typed {
    lines: Receiver<String>,
    pending: Vec<u8>,
}

impl Read for Typed {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pending.is_empty() {
            match self.lines.recv() {
                Ok(line) => self.pending = format!("{line}\n").into_bytes(),
                Err(_) => return Ok(0),
            }
        }
        let n = buf.len().min(self.pending.len());
        buf[..n].copy_from_slice(&self.pending[..n]);
        self.pending.drain(..n);
        Ok(n)
    }
}

#[derive(Clone, Default)]
struct Shared(Arc<Mutex<Vec<u8>>>);

impl Write for Shared {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Shared {
    fn wait_for(&self, text: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let out = String::from_utf8(self.0.lock().unwrap().clone()).unwrap();
            if out.contains(text) {
                return out;
            }
            assert!(Instant::now() < deadline, "no {text:?} in {out}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[test]
fn tells_the_ai_tool_when_tools_change() {
    let home = home();
    let (send, lines): (Sender<String>, _) = mpsc::channel();
    let out = Shared::default();
    let serving = {
        let (home, out) = (home.clone(), out.clone());
        std::thread::spawn(move || {
            let input = std::io::BufReader::new(Typed {
                lines,
                pending: Vec::new(),
            });
            serve(&home, Path::new("stackyard"), input, out)
        })
    };
    send.send(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#.into())
        .unwrap();
    out.wait_for("\"protocolVersion\"");
    change_settings(&home, |s| set_allowed(s, "create_site", true).unwrap());
    out.wait_for("notifications/tools/list_changed");
    send.send(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#.into())
        .unwrap();
    let all = out.wait_for("create_site");
    // Every message is one line of JSON.
    for line in all.lines() {
        serde_json::from_str::<Value>(line).unwrap();
    }
    drop(send);
    serving.join().unwrap().unwrap();
}

#[test]
fn wordpress_tools_follow_their_module() {
    let home = home();
    let mut server = server(&home);
    let listed = ok(&mut server, "list_wordpress_environments", json!({}));
    assert_eq!(listed["sites"], json!([]));
    assert!(listed["note"].as_str().unwrap().contains("live site"));

    change_settings(&home, |s| set_allowed(s, "run_wp_cli", true).unwrap());
    let message = failed(
        &mut server,
        "run_wp_cli",
        json!({ "site": "nowhere.stack", "args": ["plugin", "list"] }),
    );
    assert!(message.contains("no site called"), "{message}");
    let message = failed(
        &mut server,
        "run_wp_cli",
        json!({ "site": "nowhere.stack", "args": "plugin list" }),
    );
    assert!(!message.is_empty());

    change_settings(&home, |s| s.set_module("wordpress", false).unwrap());
    let names = tool_names(&mut server);
    assert!(!names
        .iter()
        .any(|n| n.contains("wordpress") || n == "run_wp_cli"));
    let message = failed(&mut server, "list_wordpress_environments", json!({}));
    assert!(
        message.contains("WordPress tools are switched off"),
        "{message}"
    );
}
