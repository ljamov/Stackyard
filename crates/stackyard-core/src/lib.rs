//! Core logic shared by the Stackyard desktop app, the `stackyard` CLI and,
//! later, the MCP server. Nothing in here knows about windows or terminals.

pub mod catalog;
pub mod domain;
pub mod error;
pub mod modules;
pub mod paths;
pub mod ports;
pub mod settings;
pub mod sites;
pub mod supervisor;

pub use error::{Error, Result};

#[cfg(test)]
pub(crate) mod testutil {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A fresh, empty folder for one test.
    pub fn tempdir() -> PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "stackyard-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
