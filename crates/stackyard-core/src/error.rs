use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid site name {0:?}: use lowercase letters, digits and hyphens")]
    InvalidSiteName(String),
    #[error("invalid domain ending {0:?}: use something like .stack or .test")]
    InvalidTld(String),
    #[error("a site named {0:?} already exists")]
    SiteExists(String),
    #[error("no site named {0:?}")]
    SiteNotFound(String),
    #[error("could not read or write {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not parse {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("unknown module {0:?}")]
    UnknownModule(String),
    #[error("{0} is not available yet")]
    ModuleUnavailable(String),
    #[error("{name} is not available on {system}")]
    NotOnThisSystem { name: String, system: String },
    #[error("{name} is switched off in Settings › Modules")]
    ModuleOff { name: String },
    #[error("{other} is running; stop it to start {name} instead")]
    Alternative { name: String, other: String },
    #[error("port {port} is already in use, so {name} cannot start")]
    PortInUse { name: String, port: u16 },
    #[error("could not start {program}: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("no process named {0:?}")]
    UnknownProcess(String),
    #[error("could not save settings: {0}")]
    Serialize(#[from] toml::ser::Error),
    #[error("download from {url} failed: {message}")]
    Download { url: String, message: String },
    #[error("{file} is damaged or was tampered with (expected checksum {expected}, got {actual})")]
    Checksum {
        file: String,
        expected: String,
        actual: String,
    },
    #[error("could not unpack {path}: {message}")]
    Archive { path: PathBuf, message: String },
    #[error("{name} {version} is not offered by Stackyard")]
    UnknownVersion { name: String, version: String },
    #[error("{name} {version} is only offered on Windows")]
    WindowsOnly { name: String, version: String },
    #[error("{name} {version} has no download for {platform} yet")]
    NoBuild {
        name: String,
        version: String,
        platform: String,
    },
    #[error("could not set up {name}: {message}")]
    ServiceSetup { name: String, message: String },
    #[error("could not make the {name} site: {message}")]
    Starter { name: String, message: String },
    #[error("{name} {version} is not installed")]
    NotInstalled { name: String, version: String },
    #[error("{0} is not a folder")]
    NotAFolder(PathBuf),
    #[error("{0} is not parked")]
    NotParked(PathBuf),
    #[error("system setup failed: {0}")]
    SystemSetup(String),
    #[error("{0}")]
    Control(String),
    #[error("{}: {message}", file.display())]
    BadPin { file: PathBuf, message: String },
    #[error("{0}")]
    Shell(String),
    #[error("{0}")]
    Setup(String),
    #[error("{0}")]
    Mcp(String),
    #[error("{0}")]
    Database(String),
    #[error("{0}")]
    Task(String),
    #[error("{0}")]
    Cleanup(String),
    #[error("{0}")]
    Remote(String),
    #[error("the server's certificate is not trusted ({reason}); if this is your server, trust certificate {fingerprint}")]
    RemoteCertificate { fingerprint: String, reason: String },
    #[error("the server's key changed from {expected} to {found}: if the server was not rebuilt or moved, someone may be listening in; if it was, forget the old key in the connection's settings")]
    ServerKeyChanged { expected: String, found: String },
    #[error("{0}")]
    Git(String),
    #[error("{0}")]
    Images(String),
    #[error("{0}")]
    Ai(String),
}

pub type Result<T> = std::result::Result<T, Error>;
