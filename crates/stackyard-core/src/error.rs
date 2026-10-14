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
    #[error("could not save settings: {0}")]
    Serialize(#[from] toml::ser::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
