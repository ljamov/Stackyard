//! Python, Go and Java apps: how to tell a project folder holds one, and the
//! command that runs its server while the web server runs.
//!
//! These sites always run their own server, so Stackyard starts it with the
//! site's version on the site's port and the web server passes requests on:
//!
//! - Django: `python manage.py runserver`
//! - FastAPI: `python -m uvicorn <module>:app --reload`
//! - Flask: `python -m flask run --debug`
//! - Go: `go run .` (or `./cmd/<name>`), with the port in `PORT`
//! - Spring Boot, Quarkus, Micronaut: the project's Maven or Gradle wrapper,
//!   with the port in the variable each framework reads
//!
//! A project's own `.venv` (or `venv`) is used for Python when it has one,
//! so the packages installed there are found.
//!
//! Detection only reads files; it never runs project code.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::catalog::RuntimeKind;
use crate::platform::Os;
use crate::shell::{Context, Launch, Tool};
use crate::{Error, Result};

/// What [`detect`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct App {
    pub kind: RuntimeKind,
    pub framework: &'static str,
    /// The port its server usually listens on.
    pub port: u16,
}

pub fn detect(project: &Path) -> Option<App> {
    python(project)
        .or_else(|| go(project))
        .or_else(|| java(project))
}

fn read(project: &Path, files: &[&str]) -> String {
    files
        .iter()
        .filter_map(|f| std::fs::read_to_string(project.join(f)).ok())
        .collect::<Vec<_>>()
        .join("\n")
        .to_ascii_lowercase()
}

fn python(project: &Path) -> Option<App> {
    let app = |framework, port| {
        Some(App {
            kind: RuntimeKind::Python,
            framework,
            port,
        })
    };
    if project.join("manage.py").is_file() {
        return app("Django", 8000);
    }
    let deps = read(
        project,
        &["requirements.txt", "pyproject.toml", "Pipfile", "setup.py"],
    );
    if deps.contains("fastapi") && fastapi_module(project).is_some() {
        return app("FastAPI", 8000);
    }
    if deps.contains("flask") {
        return app("Flask", 5000);
    }
    None
}

/// Where a FastAPI app lives: (module, folder to import from).
fn fastapi_module(project: &Path) -> Option<(&'static str, Option<&'static str>)> {
    [
        ("main.py", "main", None),
        ("app/main.py", "app.main", None),
        ("app.py", "app", None),
        ("src/main.py", "main", Some("src")),
        ("api/main.py", "api.main", None),
    ]
    .into_iter()
    .find(|(file, ..)| project.join(file).is_file())
    .map(|(_, module, dir)| (module, dir))
}

fn go(project: &Path) -> Option<App> {
    let go_mod = std::fs::read_to_string(project.join("go.mod")).ok()?;
    go_target(project)?;
    let framework = [
        ("github.com/gin-gonic/gin", "Gin"),
        ("github.com/labstack/echo", "Echo"),
        ("github.com/gofiber/fiber", "Fiber"),
        ("github.com/go-chi/chi", "Chi"),
    ]
    .into_iter()
    .find(|(module, _)| go_mod.contains(module))
    .map_or("Go", |(_, name)| name);
    Some(App {
        kind: RuntimeKind::Go,
        framework,
        port: 8080,
    })
}

/// What `go run` runs: the project itself when it has `main.go`, else the
/// first `cmd/<name>` that does.
fn go_target(project: &Path) -> Option<String> {
    if project.join("main.go").is_file() {
        return Some(".".into());
    }
    let mut commands: Vec<String> = std::fs::read_dir(project.join("cmd"))
        .ok()?
        .flatten()
        .filter(|e| e.path().join("main.go").is_file())
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .collect();
    commands.sort();
    commands
        .into_iter()
        .next()
        .map(|name| format!("./cmd/{name}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Build {
    Maven,
    Gradle,
}

fn java(project: &Path) -> Option<App> {
    build_tool(project)?;
    let text = read(project, &["pom.xml", "build.gradle", "build.gradle.kts"]);
    let framework = [
        ("spring-boot", "Spring Boot"),
        ("io.quarkus", "Quarkus"),
        ("micronaut", "Micronaut"),
    ]
    .into_iter()
    .find(|(marker, _)| text.contains(marker))?
    .1;
    Some(App {
        kind: RuntimeKind::Java,
        framework,
        port: 8080,
    })
}

fn build_tool(project: &Path) -> Option<Build> {
    if project.join("pom.xml").is_file() {
        Some(Build::Maven)
    } else if ["build.gradle", "build.gradle.kts"]
        .iter()
        .any(|f| project.join(f).is_file())
    {
        Some(Build::Gradle)
    } else {
        None
    }
}

/// How to run the server of the app in `project` on `port`.
pub fn server(context: &Context, project: &Path, kind: RuntimeKind, port: u16) -> Result<Launch> {
    let found = detect(project).filter(|app| app.kind == kind);
    let framework = found.map(|app| app.framework);
    let port_text = port.to_string();
    let words = |list: &[&str]| list.iter().map(OsString::from).collect::<Vec<_>>();
    let mut launch = match kind {
        RuntimeKind::Python => {
            let args = match framework {
                Some("Django") => words(&["manage.py", "runserver", &format!("127.0.0.1:{port}")]),
                Some("FastAPI") => {
                    let (module, dir) = fastapi_module(project).expect("checked by detect");
                    let mut args = words(&["-m", "uvicorn", &format!("{module}:app"), "--reload"]);
                    if let Some(dir) = dir {
                        args.extend(words(&["--app-dir", dir]));
                    }
                    args.extend(words(&["--host", "127.0.0.1", "--port", &port_text]));
                    args
                }
                Some("Flask") => words(&[
                    "-m",
                    "flask",
                    "run",
                    "--debug",
                    "--host",
                    "127.0.0.1",
                    "--port",
                    &port_text,
                ]),
                _ => return Err(no_server(project, "Django, FastAPI or Flask")),
            };
            let mut launch = context.launch(Tool::Python, project, args)?;
            if let Some(venv) = venv_python(project, context.os) {
                launch.path_first = venv.parent().map(Path::to_path_buf).unwrap_or_default();
                launch.env.push((
                    "VIRTUAL_ENV".into(),
                    venv.parent()
                        .and_then(Path::parent)
                        .map(|p| p.as_os_str().to_owned())
                        .unwrap_or_default(),
                ));
                launch.summary = format!("{} with {}", launch.summary, venv.display());
                launch.program = venv;
            }
            // Print right away, so the log viewer shows requests as they come.
            launch.env.push(("PYTHONUNBUFFERED".into(), "1".into()));
            launch
        }
        RuntimeKind::Go => {
            let target = go_target(project).ok_or_else(|| no_server(project, "main.go"))?;
            context.launch(Tool::Go, project, words(&["run", &target]))?
        }
        RuntimeKind::Java => {
            let (build, task) = match (build_tool(project), framework) {
                (Some(Build::Maven), Some("Spring Boot")) => (Build::Maven, "spring-boot:run"),
                (Some(Build::Maven), Some("Quarkus")) => (Build::Maven, "quarkus:dev"),
                (Some(Build::Maven), Some("Micronaut")) => (Build::Maven, "mn:run"),
                (Some(Build::Gradle), Some("Spring Boot")) => (Build::Gradle, "bootRun"),
                (Some(Build::Gradle), Some("Quarkus")) => (Build::Gradle, "quarkusDev"),
                (Some(Build::Gradle), Some("Micronaut")) => (Build::Gradle, "run"),
                _ => return Err(no_server(project, "Spring Boot, Quarkus or Micronaut")),
            };
            // JAVA_HOME and PATH for the project's Java version.
            let mut launch = context.launch(Tool::Java, project, Vec::new())?;
            launch.program = build_program(context, project, build)?;
            launch.args = words(&[task]);
            launch
        }
        RuntimeKind::Php | RuntimeKind::Node => {
            return Err(Error::Shell(format!(
                "{} sites are not run by apps::server",
                crate::catalog::runtime(kind).name
            )))
        }
    };
    // Every framework above reads one of these; setting all is harmless.
    for name in [
        "PORT",
        "SERVER_PORT",
        "QUARKUS_HTTP_PORT",
        "MICRONAUT_SERVER_PORT",
    ] {
        launch.env.push((name.into(), port_text.clone().into()));
    }
    Ok(launch)
}

fn no_server(project: &Path, wanted: &str) -> Error {
    Error::Shell(format!(
        "{} has no {wanted} app Stackyard knows how to start",
        project.display()
    ))
}

/// The Python of the project's own virtual environment, if it has one.
pub fn venv_python(project: &Path, os: Os) -> Option<PathBuf> {
    let program = match os {
        Os::Windows => "Scripts/python.exe",
        _ => "bin/python",
    };
    [".venv", "venv"]
        .iter()
        .map(|dir| project.join(dir).join(program))
        .find(|p| p.is_file())
}

/// The project's Maven or Gradle wrapper, else `mvn` or `gradle` from PATH.
fn build_program(context: &Context, project: &Path, build: Build) -> Result<PathBuf> {
    let (wrapper, installed) = match (build, context.os) {
        (Build::Maven, Os::Windows) => ("mvnw.cmd", "mvn"),
        (Build::Maven, _) => ("mvnw", "mvn"),
        (Build::Gradle, Os::Windows) => ("gradlew.bat", "gradle"),
        (Build::Gradle, _) => ("gradlew", "gradle"),
    };
    let wrapper = project.join(wrapper);
    if wrapper.is_file() {
        return Ok(wrapper);
    }
    context.elsewhere_on_path(installed).ok_or_else(|| {
        Error::Shell(format!(
            "{} has no {} and {installed} is not installed; add the wrapper with `{}`",
            project.display(),
            wrapper.file_name().unwrap_or_default().to_string_lossy(),
            match build {
                Build::Maven => "mvn wrapper:wrapper",
                Build::Gradle => "gradle wrapper",
            }
        ))
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

    fn found(files: &[(&str, &str)]) -> Option<(RuntimeKind, &'static str, u16)> {
        detect(&project(files)).map(|a| (a.kind, a.framework, a.port))
    }

    #[test]
    fn finds_python_apps() {
        use RuntimeKind::Python;
        assert_eq!(
            found(&[("manage.py", ""), ("requirements.txt", "Django==5.2")]),
            Some((Python, "Django", 8000))
        );
        assert_eq!(
            found(&[
                ("pyproject.toml", "dependencies = [\"FastAPI[standard]\"]"),
                ("app/main.py", "")
            ]),
            Some((Python, "FastAPI", 8000))
        );
        assert_eq!(
            found(&[("requirements.txt", "flask\nflask-cors"), ("app.py", "")]),
            Some((Python, "Flask", 5000))
        );
        // A library, or FastAPI with no app file Stackyard can find.
        assert_eq!(found(&[("requirements.txt", "requests")]), None);
        assert_eq!(
            found(&[("requirements.txt", "fastapi"), ("x/y.py", "")]),
            None
        );
    }

    #[test]
    fn finds_go_apps() {
        use RuntimeKind::Go;
        let gin = "module shop\n\nrequire github.com/gin-gonic/gin v1.11.0\n";
        assert_eq!(
            found(&[("go.mod", gin), ("main.go", "package main")]),
            Some((Go, "Gin", 8080))
        );
        assert_eq!(
            found(&[
                ("go.mod", "module x"),
                ("cmd/server/main.go", "package main")
            ]),
            Some((Go, "Go", 8080))
        );
        // A Go library has nothing to run.
        assert_eq!(
            found(&[("go.mod", "module x"), ("lib.go", "package x")]),
            None
        );
    }

    #[test]
    fn finds_java_apps() {
        use RuntimeKind::Java;
        let pom = "<parent><groupId>org.springframework.boot</groupId><artifactId>spring-boot-starter-parent</artifactId></parent>";
        assert_eq!(
            found(&[("pom.xml", pom)]),
            Some((Java, "Spring Boot", 8080))
        );
        assert_eq!(
            found(&[("build.gradle.kts", "plugins { id(\"io.quarkus\") }")]),
            Some((Java, "Quarkus", 8080))
        );
        assert_eq!(found(&[("pom.xml", "<project/>")]), None);
    }

    /// A context where `kind` `version` is installed (an empty program).
    fn context_with(kind: RuntimeKind, version: &str) -> Context {
        let home = crate::testutil::tempdir();
        let os = crate::platform::Platform::current().os;
        let runtimes = crate::runtimes::RuntimeManager::new(&home);
        let dir = runtimes.dir(kind, version);
        let binary = dir.join(crate::runtimes::binary_path(kind, os));
        std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
        std::fs::write(&binary, "").unwrap();
        crate::install::write_receipt_for_tests(&dir, &format!("{version}.1"));
        let mut settings = crate::settings::Settings::default();
        settings.set_module(kind.id(), true).unwrap();
        Context {
            home,
            settings,
            sites: Vec::new(),
            os,
        }
    }

    fn env<'a>(launch: &'a Launch, name: &str) -> Option<&'a std::ffi::OsStr> {
        launch
            .env
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_os_str())
    }

    #[test]
    fn runs_python_apps_with_the_projects_venv() {
        let context = context_with(RuntimeKind::Python, "3.13");
        let django = project(&[("manage.py", "")]);
        let launch = server(&context, &django, RuntimeKind::Python, 8001).unwrap();
        assert_eq!(
            launch.args,
            ["manage.py", "runserver", "127.0.0.1:8001"].map(OsString::from)
        );
        assert_eq!(env(&launch, "PORT"), Some("8001".as_ref()));

        let venv = match context.os {
            Os::Windows => ".venv/Scripts/python.exe",
            _ => ".venv/bin/python",
        };
        let fastapi = project(&[
            ("requirements.txt", "fastapi\nuvicorn"),
            ("src/main.py", ""),
            (venv, ""),
        ]);
        let launch = server(&context, &fastapi, RuntimeKind::Python, 8002).unwrap();
        assert_eq!(launch.program, fastapi.join(venv));
        assert!(env(&launch, "VIRTUAL_ENV").is_some());
        let args: Vec<_> = launch.args.iter().map(|a| a.to_string_lossy()).collect();
        assert_eq!(
            args.join(" "),
            "-m uvicorn main:app --reload --app-dir src --host 127.0.0.1 --port 8002"
        );

        let plain = project(&[("main.py", "")]);
        assert!(server(&context, &plain, RuntimeKind::Python, 8003).is_err());
    }

    #[test]
    fn runs_go_and_java_apps() {
        let go = context_with(RuntimeKind::Go, "1.26");
        let app = project(&[("go.mod", "module x"), ("cmd/api/main.go", "")]);
        let launch = server(&go, &app, RuntimeKind::Go, 8081).unwrap();
        assert_eq!(launch.args, ["run", "./cmd/api"].map(OsString::from));
        assert_eq!(env(&launch, "PORT"), Some("8081".as_ref()));

        let java = context_with(RuntimeKind::Java, "21");
        let wrapper = match java.os {
            Os::Windows => "mvnw.cmd",
            _ => "mvnw",
        };
        let spring = project(&[("pom.xml", "spring-boot-starter-web"), (wrapper, "")]);
        let launch = server(&java, &spring, RuntimeKind::Java, 8082).unwrap();
        assert_eq!(launch.program, spring.join(wrapper));
        assert_eq!(launch.args, ["spring-boot:run"].map(OsString::from));
        assert_eq!(env(&launch, "SERVER_PORT"), Some("8082".as_ref()));
        assert!(env(&launch, "JAVA_HOME").is_some());
    }

    #[test]
    fn finds_go_commands() {
        assert_eq!(
            go_target(&project(&[
                ("cmd/web/main.go", ""),
                ("cmd/api/main.go", "")
            ]))
            .as_deref(),
            Some("./cmd/api")
        );
    }

    #[test]
    fn finds_the_projects_venv() {
        let os = crate::platform::Platform::current().os;
        let program = match os {
            Os::Windows => ".venv/Scripts/python.exe",
            _ => ".venv/bin/python",
        };
        let dir = project(&[(program, "")]);
        assert_eq!(venv_python(&dir, os), Some(dir.join(program)));
        assert_eq!(venv_python(&project(&[]), os), None);
    }
}
