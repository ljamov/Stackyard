use super::*;

/// Pretends `tool` was installed with npm on Node.js `node`, its package
/// naming `bin` as its program.
fn fake_tool(home: &Path, tool: &AiTool, node: &str, bin: serde_json::Value) -> PathBuf {
    let dest = dir(home, tool);
    let package = package_dir(&dest, tool);
    std::fs::create_dir_all(package.join("bin")).unwrap();
    let json = serde_json::json!({ "name": tool.package, "version": "1.2.3", "bin": bin });
    std::fs::write(package.join("package.json"), json.to_string()).unwrap();
    write_toml(&dest.join(RECEIPT), &Receipt { node: node.into() }).unwrap();
    dest
}

fn fake_node(home: &Path, version: &str) -> PathBuf {
    let runtimes = RuntimeManager::new(home);
    let dir = runtimes.dir(RuntimeKind::Node, version);
    let binary = dir.join(crate::runtimes::binary_path(
        RuntimeKind::Node,
        Platform::current().os,
    ));
    std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
    std::fs::write(&binary, "").unwrap();
    crate::install::write_receipt_for_tests(&dir, &format!("{version}.0.0"));
    binary
}

#[test]
fn catalog_is_sound() {
    let mut ids: Vec<_> = TOOLS.iter().map(|t| t.id).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), TOOLS.len());
    let node = crate::catalog::runtime(RuntimeKind::Node);
    assert!(node.versions.contains(&NODE_FOR_TOOLS));
    for tool in TOOLS {
        assert!(tool.min_node <= major(NODE_FOR_TOOLS), "{}", tool.id);
        assert!(tool.docs.starts_with("https://"));
        assert_eq!(from_command(tool.command).unwrap().id, tool.id);
        // A command of its own, so it never hides `php` or `node`.
        assert!(crate::shell::Tool::from_name(tool.command).is_none());
    }
    assert_eq!(from_command("CLAUDE.EXE").unwrap().id, "claude");
    assert!(from_command("php").is_none());
    assert!(find("nope").is_err());
}

#[test]
fn compares_versions_by_number() {
    assert!(is_newer("0.10.0", "0.9.3"));
    assert!(is_newer("2.1.302", "2.1.30"));
    assert!(!is_newer("1.0.0", "1.0.0"));
    assert!(!is_newer("1.0.0", "1.0.1"));
    assert!(is_newer("1.0.1-beta.1", "1.0.0"));
    assert!(is_newer("weird", "other"));
}

#[test]
fn runs_scripts_on_node_and_native_programs_directly() {
    let home = crate::testutil::tempdir();
    let gemini = find("gemini").unwrap();
    let claude = find("claude").unwrap();
    assert!(launch(&home, gemini, vec![]).is_err(), "not installed");

    let node = fake_node(&home, "24");
    let dest = fake_tool(
        &home,
        gemini,
        "24",
        serde_json::json!({ "gemini": "bundle/gemini.js" }),
    );
    let script = package_dir(&dest, gemini).join("bundle/gemini.js");
    std::fs::create_dir_all(script.parent().unwrap()).unwrap();
    std::fs::write(&script, "").unwrap();
    let run = launch(&home, gemini, vec!["-p".into()]).unwrap();
    assert_eq!(run.program, node);
    assert_eq!(run.args, vec![script.into_os_string(), "-p".into()]);
    assert_eq!(run.path_first, node.parent().unwrap());
    assert_eq!(run.summary, "Gemini CLI 1.2.3 (Node.js 24)");

    let dest = fake_tool(&home, claude, "24", serde_json::json!("bin/claude.exe"));
    let exe = package_dir(&dest, claude).join("bin/claude.exe");
    std::fs::write(&exe, "").unwrap();
    let run = launch(&home, claude, vec!["doctor".into()]).unwrap();
    assert_eq!(run.program, exe);
    assert_eq!(run.args, vec![OsString::from("doctor")]);

    let listed = list(&home);
    let shown: Vec<_> = listed
        .iter()
        .filter(|s| s.installed.is_some())
        .map(|s| (s.tool.id, s.node.as_deref()))
        .collect();
    assert_eq!(shown, [("claude", Some("24")), ("gemini", Some("24"))]);

    // The program went missing: say so instead of starting nothing.
    std::fs::remove_file(&exe).unwrap();
    let err = launch(&home, claude, vec![]).unwrap_err().to_string();
    assert!(err.contains("install it again"), "{err}");
}

#[test]
fn falls_back_to_another_new_enough_node() {
    let home = crate::testutil::tempdir();
    let codex = find("codex").unwrap();
    let dest = fake_tool(
        &home,
        codex,
        "22",
        serde_json::json!({ "codex": "bin/codex.js" }),
    );
    std::fs::write(package_dir(&dest, codex).join("bin/codex.js"), "").unwrap();
    let err = launch(&home, codex, vec![]).unwrap_err().to_string();
    assert!(err.contains("Node.js 18 or newer"), "{err}");

    fake_node(&home, "20");
    let run = launch(&home, codex, vec![]).unwrap();
    assert!(run.summary.ends_with("(Node.js 20)"), "{}", run.summary);

    // Claude Code needs 22: 20 does not do.
    let settings = Settings::default();
    assert_eq!(node_for(&home, &settings, find("claude").unwrap()), None);
    fake_node(&home, "26");
    fake_node(&home, "22");
    let mut settings = Settings::default();
    assert_eq!(
        node_for(&home, &settings, codex).as_deref(),
        Some("26"),
        "newest"
    );
    settings.set_default_runtime(RuntimeKind::Node, Some("22"));
    assert_eq!(
        node_for(&home, &settings, codex).as_deref(),
        Some("22"),
        "default"
    );
    settings.set_default_runtime(RuntimeKind::Node, Some("20"));
    assert_eq!(
        node_for(&home, &settings, find("claude").unwrap()).as_deref(),
        Some("26"),
        "default too old"
    );
}

#[test]
fn commands_follow_stackyard_shell() {
    let home = crate::testutil::tempdir();
    let codex = find("codex").unwrap();
    let dest = fake_tool(
        &home,
        codex,
        "22",
        serde_json::json!({ "codex": "bin/codex.js" }),
    );
    std::fs::write(package_dir(&dest, codex).join("bin/codex.js"), "").unwrap();

    // Shell off: no commands.
    link_shims(&home).unwrap();
    assert!(!shim_path(&home, codex).exists());

    let exe = home.join(format!("stackyard{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(&exe, "program").unwrap();
    shims::install(&home, &exe).unwrap();
    assert_eq!(
        std::fs::read_to_string(shim_path(&home, codex)).unwrap(),
        "program"
    );
    assert!(!shim_path(&home, find("claude").unwrap()).exists());

    uninstall(&home, codex).unwrap();
    assert!(!shim_path(&home, codex).exists());
    assert!(!dest.exists());
    assert!(uninstall(&home, codex).is_err());

    let dest = fake_tool(
        &home,
        codex,
        "22",
        serde_json::json!({ "codex": "bin/codex.js" }),
    );
    std::fs::write(package_dir(&dest, codex).join("bin/codex.js"), "").unwrap();
    link_shims(&home).unwrap();
    assert!(shim_path(&home, codex).exists());
    shims::remove(&home).unwrap();
    assert!(!shim_path(&home, codex).exists());
    assert!(dest.exists(), "the tool stays installed");
}

#[test]
fn checks_npm_for_the_newest_version() {
    let base = crate::download::testserver::serve(br#"{"name":"x","version":"9.9.9"}"#.to_vec());
    let http = Http::new();
    assert_eq!(latest(&http, &base, "@openai/codex").unwrap(), "9.9.9");
    assert!(latest(&http, "http://127.0.0.1:1", "x").is_err());
}

#[test]
fn off_module_stops_the_tools() {
    let home = crate::testutil::tempdir();
    let mut settings = Settings::default();
    settings.set_module("ai", false).unwrap();
    write_toml(&home.join("settings.toml"), &settings).unwrap();
    let err = launch(&home, find("codex").unwrap(), vec![])
        .unwrap_err()
        .to_string();
    assert!(err.contains("switched off"), "{err}");
    let err = install(&home, find("codex").unwrap(), &mut |_| {})
        .unwrap_err()
        .to_string();
    assert!(err.contains("switched off"), "{err}");
}

/// Installs Codex for real (or the tool `STACKYARD_AI_TOOL` names):
/// Node.js from nodejs.org, then npm.
#[test]
#[ignore]
fn live_install_ai_tool() {
    let home = crate::testutil::tempdir();
    let id = std::env::var("STACKYARD_AI_TOOL").unwrap_or_else(|_| "codex".into());
    let codex = find(&id).unwrap();
    let mut steps = Vec::new();
    let status = install(&home, codex, &mut |s| steps.push(s)).unwrap();
    assert!(status.installed.is_some(), "{steps:?}");
    let run = launch(&home, codex, vec!["--version".into()]).unwrap();
    let output = Command::new(&run.program)
        .args(&run.args)
        .env("PATH", path_with(&[&run.path_first]))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains(status.installed.as_deref().unwrap()),
        "{text}"
    );
    // Again: an update in place.
    install(&home, codex, &mut |_| {}).unwrap();
    let mut statuses = list(&home);
    check_updates(&mut statuses);
    let mine = statuses.iter().find(|s| s.tool.id == id).unwrap();
    assert!(mine.latest.is_some());
    assert!(!mine.update);
    uninstall(&home, codex).unwrap();
}
