//! Passwords kept in the system's own password store: Windows Credential
//! Manager, the macOS Keychain, or the desktop keyring on Linux (through
//! `secret-tool`). A Linux system without a keyring falls back to a file
//! in Stackyard's folder that only the user can read.

use crate::{Error, Result};

const SERVICE: &str = "Stackyard";

/// Where secrets are kept on this computer, to tell the user.
pub fn store_name() -> &'static str {
    if cfg!(windows) {
        "Windows Credential Manager"
    } else if cfg!(target_os = "macos") {
        "the macOS Keychain"
    } else if linux::keyring_available() {
        "your desktop keyring"
    } else {
        "a file in Stackyard's folder that only you can read"
    }
}

pub fn set(account: &str, secret: &str) -> Result<()> {
    imp::set(account, secret)
        .map_err(|e| Error::Remote(format!("could not save the password: {e}")))
}

/// The stored secret, or `None` when there is none.
pub fn get(account: &str) -> Result<Option<String>> {
    imp::get(account).map_err(|e| Error::Remote(format!("could not read the saved password: {e}")))
}

/// Removes the secret; succeeds when there was none.
pub fn delete(account: &str) -> Result<()> {
    imp::delete(account)
        .map_err(|e| Error::Remote(format!("could not remove the saved password: {e}")))
}

#[cfg(windows)]
mod imp {
    use std::ptr;

    use windows_sys::Win32::Foundation::{GetLastError, ERROR_NOT_FOUND, FILETIME};
    use windows_sys::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
        CRED_TYPE_GENERIC,
    };

    fn target(account: &str) -> Vec<u16> {
        format!("{}/{account}", super::SERVICE)
            .encode_utf16()
            .chain(Some(0))
            .collect()
    }

    fn last_error() -> String {
        // SAFETY: GetLastError has no preconditions.
        let code = unsafe { GetLastError() };
        std::io::Error::from_raw_os_error(code as i32).to_string()
    }

    pub fn set(account: &str, secret: &str) -> Result<(), String> {
        let mut name = target(account);
        let mut blob = secret.as_bytes().to_vec();
        let credential = CREDENTIALW {
            Flags: 0,
            Type: CRED_TYPE_GENERIC,
            TargetName: name.as_mut_ptr(),
            Comment: ptr::null_mut(),
            LastWritten: FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            },
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            AttributeCount: 0,
            Attributes: ptr::null_mut(),
            TargetAlias: ptr::null_mut(),
            UserName: ptr::null_mut(),
        };
        // SAFETY: every pointer in `credential` is valid for the call.
        if unsafe { CredWriteW(&credential, 0) } == 0 {
            return Err(last_error());
        }
        Ok(())
    }

    pub fn get(account: &str) -> Result<Option<String>, String> {
        let name = target(account);
        let mut found: *mut CREDENTIALW = ptr::null_mut();
        // SAFETY: `name` is NUL-terminated; `found` receives a buffer we free below.
        if unsafe { CredReadW(name.as_ptr(), CRED_TYPE_GENERIC, 0, &mut found) } == 0 {
            // SAFETY: GetLastError has no preconditions.
            if unsafe { GetLastError() } == ERROR_NOT_FOUND {
                return Ok(None);
            }
            return Err(last_error());
        }
        // SAFETY: CredReadW succeeded, so `found` points to a credential whose
        // blob is `CredentialBlobSize` bytes long.
        let secret = unsafe {
            let credential = &*found;
            let bytes = if credential.CredentialBlob.is_null() {
                Vec::new()
            } else {
                std::slice::from_raw_parts(
                    credential.CredentialBlob,
                    credential.CredentialBlobSize as usize,
                )
                .to_vec()
            };
            CredFree(found as *const _);
            bytes
        };
        String::from_utf8(secret)
            .map(Some)
            .map_err(|_| "the saved password is not text".to_string())
    }

    pub fn delete(account: &str) -> Result<(), String> {
        let name = target(account);
        // SAFETY: `name` is NUL-terminated.
        if unsafe { CredDeleteW(name.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0 {
            // SAFETY: GetLastError has no preconditions.
            if unsafe { GetLastError() } == ERROR_NOT_FOUND {
                return Ok(());
            }
            return Err(last_error());
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use security_framework::passwords::{
        delete_generic_password, get_generic_password, set_generic_password,
    };

    /// errSecItemNotFound.
    const NOT_FOUND: i32 = -25300;

    pub fn set(account: &str, secret: &str) -> Result<(), String> {
        set_generic_password(super::SERVICE, account, secret.as_bytes()).map_err(|e| e.to_string())
    }

    pub fn get(account: &str) -> Result<Option<String>, String> {
        match get_generic_password(super::SERVICE, account) {
            Ok(bytes) => String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| "the saved password is not text".to_string()),
            Err(e) if e.code() == NOT_FOUND => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn delete(account: &str) -> Result<(), String> {
        match delete_generic_password(super::SERVICE, account) {
            Err(e) if e.code() != NOT_FOUND => Err(e.to_string()),
            _ => Ok(()),
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    use super::linux;

    pub fn set(account: &str, secret: &str) -> Result<(), String> {
        if linux::keyring_available() {
            return linux::secret_tool_store(account, secret);
        }
        linux::file_set(account, Some(secret))
    }

    pub fn get(account: &str) -> Result<Option<String>, String> {
        if linux::keyring_available() {
            if let Some(secret) = linux::secret_tool_lookup(account)? {
                return Ok(Some(secret));
            }
        }
        linux::file_get(account)
    }

    pub fn delete(account: &str) -> Result<(), String> {
        if linux::keyring_available() {
            linux::secret_tool_clear(account)?;
        }
        linux::file_set(account, None)
    }
}

#[cfg_attr(any(windows, target_os = "macos"), allow(dead_code))]
mod linux {
    use std::collections::BTreeMap;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::sync::OnceLock;

    use base64::Engine;

    fn secret_tool(args: &[&str]) -> Command {
        let mut command = Command::new("secret-tool");
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    /// `secret-tool` is installed and a keyring answers it.
    pub fn keyring_available() -> bool {
        static AVAILABLE: OnceLock<bool> = OnceLock::new();
        if cfg!(any(windows, target_os = "macos")) {
            return false;
        }
        *AVAILABLE.get_or_init(|| {
            secret_tool(&[
                "search",
                "service",
                super::SERVICE,
                "account",
                "stackyard-probe",
            ])
            .stdin(Stdio::null())
            .output()
            .is_ok_and(|o| o.status.success())
        })
    }

    pub fn secret_tool_store(account: &str, secret: &str) -> Result<(), String> {
        let label = format!("{}: {account}", super::SERVICE);
        let mut child = secret_tool(&[
            "store",
            "--label",
            &label,
            "service",
            super::SERVICE,
            "account",
            account,
        ])
        .spawn()
        .map_err(|e| e.to_string())?;
        // The secret goes through stdin, so it never shows in a process list.
        child
            .stdin
            .take()
            .expect("piped")
            .write_all(secret.as_bytes())
            .map_err(|e| e.to_string())?;
        let output = child.wait_with_output().map_err(|e| e.to_string())?;
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
        }
    }

    pub fn secret_tool_lookup(account: &str) -> Result<Option<String>, String> {
        let output = secret_tool(&["lookup", "service", super::SERVICE, "account", account])
            .stdin(Stdio::null())
            .output()
            .map_err(|e| e.to_string())?;
        // secret-tool exits with 1 and prints nothing when there is no such secret.
        if !output.status.success() || output.stdout.is_empty() {
            return Ok(None);
        }
        String::from_utf8(output.stdout)
            .map(Some)
            .map_err(|_| "the saved password is not text".to_string())
    }

    pub fn secret_tool_clear(account: &str) -> Result<(), String> {
        secret_tool(&["clear", "service", super::SERVICE, "account", account])
            .stdin(Stdio::null())
            .output()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn file() -> PathBuf {
        crate::paths::home().join("config").join("secrets.toml")
    }

    pub fn file_get(account: &str) -> Result<Option<String>, String> {
        file_get_in(&file(), account)
    }

    pub fn file_set(account: &str, secret: Option<&str>) -> Result<(), String> {
        file_set_in(&file(), account, secret)
    }

    fn read(path: &Path) -> Result<BTreeMap<String, String>, String> {
        crate::settings::read_toml(path)
            .map(Option::unwrap_or_default)
            .map_err(|e| e.to_string())
    }

    pub(super) fn file_get_in(path: &Path, account: &str) -> Result<Option<String>, String> {
        let Some(encoded) = read(path)?.remove(account) else {
            return Ok(None);
        };
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|e| e.to_string())?;
        String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| "the saved password is not text".to_string())
    }

    pub(super) fn file_set_in(
        path: &Path,
        account: &str,
        secret: Option<&str>,
    ) -> Result<(), String> {
        let mut all = read(path)?;
        match secret {
            Some(secret) => {
                all.insert(
                    account.to_string(),
                    base64::engine::general_purpose::STANDARD.encode(secret),
                );
            }
            None if !all.contains_key(account) => return Ok(()),
            None => {
                all.remove(account);
            }
        }
        crate::settings::write_toml(path, &all).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::linux::{file_get_in, file_set_in};

    #[test]
    fn file_store_keeps_secrets_private() {
        let path = crate::testutil::tempdir().join("secrets.toml");
        assert_eq!(file_get_in(&path, "remote-a").unwrap(), None);
        file_set_in(&path, "remote-a", Some("p@ss \"word\"")).unwrap();
        file_set_in(&path, "remote-b", Some("other")).unwrap();
        assert_eq!(
            file_get_in(&path, "remote-a").unwrap().as_deref(),
            Some("p@ss \"word\"")
        );
        assert!(!std::fs::read_to_string(&path).unwrap().contains("p@ss"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        file_set_in(&path, "remote-a", None).unwrap();
        assert_eq!(file_get_in(&path, "remote-a").unwrap(), None);
        assert_eq!(
            file_get_in(&path, "remote-b").unwrap().as_deref(),
            Some("other")
        );
    }
}
