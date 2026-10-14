//! How the app and the CLI share one set of running services.
//!
//! One process at a time runs services: the desktop app, `stackyard daemon`
//! (started in the background by `stackyard start`) or `stackyard up`. It
//! listens on a random port on 127.0.0.1 and writes that port and a secret
//! token to `run/control.json`, readable only by the user. Other processes
//! send it one JSON request per connection, with the token.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::supervisor::ProcessState;
use crate::{Error, Result};

const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
/// Starting a database the first time can take a while.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(180);
const MAX_LINE: u64 = 64 * 1024;

/// Which kind of process runs the services.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Owner {
    /// The desktop app.
    App,
    /// `stackyard daemon`, in the background.
    Daemon,
    /// `stackyard up`, in a terminal.
    Up,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "do", rename_all = "kebab-case")]
pub enum Request {
    Status,
    /// Start services by id; `caddy` also starts PHP for the sites.
    Start {
        ids: Vec<String>,
    },
    /// Start every installed service that is switched on.
    StartAll,
    Stop {
        ids: Vec<String>,
    },
    StopAll,
    /// Sites or domain endings changed: refresh the web server and DNS.
    SitesChanged,
    /// Bring the app window to the front (the app only).
    Show,
    /// Stop everything and quit, saying what was running, so another
    /// process (the app) can take over.
    HandOver,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub id: String,
    pub name: String,
    #[serde(flatten)]
    pub state: ProcessState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "kebab-case")]
pub enum Response {
    Services { services: Vec<ServiceStatus> },
    HandedOver { running: Vec<String> },
    Done,
    Failed { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ControlFile {
    port: u16,
    token: String,
    pid: u32,
    owner: Owner,
}

#[derive(Serialize, Deserialize)]
struct Envelope {
    token: String,
    request: Request,
}

pub fn control_file(home: &Path) -> PathBuf {
    home.join("run/control.json")
}

pub type Handler = dyn Fn(Request) -> Response + Send + Sync;

/// Answers requests until dropped.
pub struct ControlServer {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    port: u16,
    file: PathBuf,
    token: String,
}

impl ControlServer {
    pub fn start(
        home: &Path,
        owner: Owner,
        handler: impl Fn(Request) -> Response + Send + Sync + 'static,
    ) -> Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|source| Error::Io {
            path: PathBuf::from("127.0.0.1"),
            source,
        })?;
        let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
        let token = new_token()?;
        let file = control_file(home);
        write_private(
            &file,
            &ControlFile {
                port,
                token: token.clone(),
                pid: std::process::id(),
                owner,
            },
        )?;
        let stop = Arc::new(AtomicBool::new(false));
        let handler: Arc<Handler> = Arc::new(handler);
        let thread = {
            let (stop, token) = (stop.clone(), token.clone());
            std::thread::Builder::new()
                .name("stackyard-control".into())
                .spawn(move || serve(listener, &stop, &token, handler))
                .map_err(|source| Error::Spawn {
                    program: "control server".into(),
                    source,
                })?
        };
        Ok(Self {
            stop,
            thread: Some(thread),
            port,
            file,
            token,
        })
    }
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Wake the accept loop.
        let _ = TcpStream::connect_timeout(
            &SocketAddr::from((Ipv4Addr::LOCALHOST, self.port)),
            CONNECT_TIMEOUT,
        );
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        // Remove the file only if a newer owner has not replaced it.
        if read_file(&self.file).is_some_and(|f| f.token == self.token) {
            let _ = std::fs::remove_file(&self.file);
        }
    }
}

fn serve(listener: TcpListener, stop: &AtomicBool, token: &str, handler: Arc<Handler>) {
    for stream in listener.incoming() {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let Ok(stream) = stream else { continue };
        let (handler, token) = (handler.clone(), token.to_string());
        // One thread per request, so a slow start does not block `status`.
        let _ = std::thread::Builder::new()
            .name("stackyard-request".into())
            .spawn(move || answer(stream, &token, &*handler));
    }
}

fn answer(stream: TcpStream, token: &str, handler: &Handler) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut line = String::new();
    let mut reader = BufReader::new((&stream).take(MAX_LINE));
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let response = match serde_json::from_str::<Envelope>(&line) {
        Ok(envelope) if constant_time_eq(&envelope.token, token) => handler(envelope.request),
        Ok(_) => Response::Failed {
            message: "wrong token".into(),
        },
        Err(e) => Response::Failed {
            message: format!("bad request: {e}"),
        },
    };
    let mut stream = stream;
    if let Ok(mut text) = serde_json::to_string(&response) {
        text.push('\n');
        let _ = stream.write_all(text.as_bytes());
    }
}

/// A connection to the process that runs services.
#[derive(Debug, Clone)]
pub struct Client {
    port: u16,
    token: String,
    pub pid: u32,
    pub owner: Owner,
}

/// The process that runs services, if one is answering.
pub fn find(home: &Path) -> Option<Client> {
    let file = read_file(&control_file(home))?;
    let client = Client {
        port: file.port,
        token: file.token,
        pid: file.pid,
        owner: file.owner,
    };
    // A file left behind by a crash points at a dead port.
    client.connect(CONNECT_TIMEOUT).ok().map(|_| client)
}

/// The process that runs services, starting `program daemon` in the
/// background (logging to `logs/daemon.log`) when there is none.
pub fn start_background(home: &Path, program: &Path) -> Result<Client> {
    if let Some(client) = find(home) {
        return Ok(client);
    }
    let logs = home.join("logs");
    let io = |source| Error::Io {
        path: logs.clone(),
        source,
    };
    std::fs::create_dir_all(&logs).map_err(io)?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs.join("daemon.log"))
        .map_err(io)?;
    let mut command = std::process::Command::new(program);
    command
        .arg("daemon")
        .env("STACKYARD_HOME", home)
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone().map_err(io)?)
        .stderr(log);
    detach(&mut command);
    let mut child = spawn_alone(&mut command).map_err(|source| Error::Spawn {
        program: program.display().to_string(),
        source,
    })?;
    for _ in 0..100 {
        if let Some(client) = find(home) {
            return Ok(client);
        }
        if let Ok(Some(status)) = child.try_wait() {
            let tail =
                crate::supervisor::tail_file(&logs.join("daemon.log"), 5).unwrap_or_default();
            return Err(Error::Control(format!(
                "the background service stopped ({status}): {}",
                tail.join(" / ")
            )));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(Error::Control(
        "the background service did not start".into(),
    ))
}

/// Starts `command` without handing it this process's own stdin, stdout and
/// stderr. On Windows a child inherits every inheritable handle, so a
/// background service would keep the pipes of whoever started this process
/// (an AI tool, a script) open, and they would wait for it forever.
fn spawn_alone(command: &mut std::process::Command) -> std::io::Result<std::process::Child> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{
            GetHandleInformation, SetHandleInformation, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE,
        };
        use windows_sys::Win32::System::Console::{
            GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
        };
        let mut hidden = Vec::new();
        for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            // SAFETY: reads and changes flags of this process's own standard handles.
            unsafe {
                let handle = GetStdHandle(which);
                let mut flags = 0u32;
                if !handle.is_null()
                    && handle != INVALID_HANDLE_VALUE
                    && GetHandleInformation(handle, &mut flags) != 0
                    && flags & HANDLE_FLAG_INHERIT != 0
                    && SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) != 0
                {
                    hidden.push(handle);
                }
            }
        }
        let child = command.spawn();
        for handle in hidden {
            // SAFETY: as above, restoring what was there.
            unsafe {
                SetHandleInformation(handle, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT);
            }
        }
        child
    }
    #[cfg(not(windows))]
    command.spawn()
}

/// Keeps a background process running after the terminal that started it closes.
fn detach(command: &mut std::process::Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid is async-signal-safe and only affects the child.
        unsafe {
            command.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
}

impl Client {
    fn connect(&self, timeout: Duration) -> std::io::Result<TcpStream> {
        TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, self.port)), timeout)
    }

    pub fn send(&self, request: Request) -> Result<Response> {
        let fail = |e: std::io::Error| {
            Error::Control(format!("could not reach the running Stackyard: {e}"))
        };
        let mut stream = self.connect(CONNECT_TIMEOUT).map_err(fail)?;
        stream
            .set_read_timeout(Some(ANSWER_TIMEOUT))
            .map_err(fail)?;
        let mut text = serde_json::to_string(&Envelope {
            token: self.token.clone(),
            request,
        })
        .map_err(|e| Error::Control(e.to_string()))?;
        text.push('\n');
        stream.write_all(text.as_bytes()).map_err(fail)?;
        let mut line = String::new();
        BufReader::new((&stream).take(MAX_LINE * 16))
            .read_line(&mut line)
            .map_err(fail)?;
        match serde_json::from_str(&line) {
            Ok(Response::Failed { message }) => Err(Error::Control(message)),
            Ok(response) => Ok(response),
            Err(e) => Err(Error::Control(format!("unexpected answer: {e}"))),
        }
    }
}

fn read_file(path: &Path) -> Option<ControlFile> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Writes the control file so only this user can read the token.
fn write_private(path: &Path, file: &ControlFile) -> Result<()> {
    let io = |source| Error::Io {
        path: path.to_owned(),
        source,
    };
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(io)?;
    let tmp = path.with_extension("json.tmp");
    let _ = std::fs::remove_file(&tmp);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let text = serde_json::to_string(file).map_err(|e| Error::Control(e.to_string()))?;
    let mut out = options.open(&tmp).map_err(io)?;
    out.write_all(text.as_bytes()).map_err(io)?;
    drop(out);
    std::fs::rename(&tmp, path).map_err(io)
}

fn new_token() -> Result<String> {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).map_err(|e| Error::Control(format!("no randomness: {e}")))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0, |acc, (x, y)| acc | (x ^ y))
            == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_requests_with_the_token_only() {
        let home = crate::testutil::tempdir();
        assert!(find(&home).is_none());
        let server = ControlServer::start(&home, Owner::Daemon, |request| match request {
            Request::Status => Response::Services {
                services: vec![ServiceStatus {
                    id: "caddy".into(),
                    name: "Caddy".into(),
                    state: ProcessState::Running { pid: 7 },
                }],
            },
            Request::Start { ids } if ids == ["nope"] => Response::Failed {
                message: "unknown service".into(),
            },
            _ => Response::Done,
        })
        .unwrap();

        let client = find(&home).expect("server answers");
        assert_eq!(client.owner, Owner::Daemon);
        assert_eq!(client.pid, std::process::id());
        match client.send(Request::Status).unwrap() {
            Response::Services { services } => {
                assert_eq!(services[0].state, ProcessState::Running { pid: 7 })
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(client.send(Request::StopAll).unwrap(), Response::Done);
        let err = client
            .send(Request::Start {
                ids: vec!["nope".into()],
            })
            .unwrap_err();
        assert!(err.to_string().contains("unknown service"));

        let mut stranger = client.clone();
        stranger.token = "0".repeat(48);
        assert!(stranger.send(Request::Status).is_err());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(control_file(&home))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        drop(server);
        assert!(!control_file(&home).exists());
        assert!(find(&home).is_none());
    }

    #[test]
    fn ignores_a_file_left_by_a_crash() {
        let home = crate::testutil::tempdir();
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        write_private(
            &control_file(&home),
            &ControlFile {
                port,
                token: "x".into(),
                pid: 1,
                owner: Owner::App,
            },
        )
        .unwrap();
        assert!(find(&home).is_none());
    }
}
