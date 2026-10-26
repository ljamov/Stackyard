//! One-time system setup, run with administrator rights after the user
//! agrees. It does three things, and only these:
//!
//! - **Local domains.** macOS gets `/etc/resolver/<ending>` and Linux with
//!   systemd-resolved gets a drop-in, both sending only Stackyard's endings
//!   to [`crate::dns`]. Windows, and Linux without systemd-resolved, get a
//!   marked block in the hosts file instead, which is rewritten when sites
//!   change. On Windows setup also lets the user's own account change the
//!   hosts file, so Stackyard keeps its block up to date by itself
//!   ([`sync_hosts`]) and new sites need no password prompt.
//! - **HTTPS.** The web server's local root certificate is added to the
//!   system's trusted certificates, so browsers accept the sites.
//! - **Standard ports (Linux only).** Lets normal programs listen on ports
//!   80 and up, so the web server can use 80 and 443 without root.
//!
//! The commands are built here as one script and run in one go: through
//! the system's own password prompt in the app (osascript, pkexec, UAC),
//! or `sudo` in the terminal. What was applied is recorded in
//! `config/system.toml`, so Stackyard can tell when setup is needed again.

use std::path::{Path, PathBuf};
use std::process::Command;

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::platform::Os;
use crate::settings::{read_toml, write_toml};
use crate::{dns, domain, Error, Result};

const BEGIN: &str = "# BEGIN Stackyard";
const END: &str = "# END Stackyard";
const CERT_NAME: &str = "stackyard-local-ca";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Resolver {
    /// `/etc/resolver/<ending>` files (macOS).
    MacResolver,
    /// A systemd-resolved drop-in (most current Linux desktops).
    SystemdResolved,
    /// The hosts file, one line per site.
    HostsFile,
}

impl Resolver {
    pub fn for_this_system(os: Os) -> Self {
        match os {
            Os::Macos => Resolver::MacResolver,
            Os::Linux if Path::new("/run/systemd/resolve").is_dir() => Resolver::SystemdResolved,
            _ => Resolver::HostsFile,
        }
    }
}

/// What setup should make true.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wanted {
    pub os: Os,
    pub resolver: Resolver,
    /// Domain endings in use, like `.stack`.
    pub endings: Vec<String>,
    /// Every site's domain, for the hosts file.
    pub domains: Vec<String>,
    /// The web server's root certificate (PEM), once it exists.
    pub certificate: Option<String>,
}

/// What setup last applied.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Applied {
    pub resolver: Option<Resolver>,
    pub endings: Vec<String>,
    pub domains: Vec<String>,
    /// SHA-256 of the trusted certificate.
    pub certificate: Option<String>,
    pub ports: bool,
    /// The user's own account may change the hosts file (Windows), so
    /// [`sync_hosts`] adds new sites without asking for a password.
    pub hosts_writable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Status {
    /// Plain descriptions of what setup would change; empty when all is done.
    pub needed: Vec<String>,
    /// True until the web server has made its certificate.
    pub waiting_for_certificate: bool,
    /// Sites whose own name already works, so the app can show that name
    /// for them. A new site can wait for setup while the others work.
    pub ready_domains: Vec<String>,
}

pub fn applied_file(home: &Path) -> PathBuf {
    home.join("config").join("system.toml")
}

pub fn applied(home: &Path) -> Applied {
    read_toml(&applied_file(home))
        .ok()
        .flatten()
        .unwrap_or_default()
}

impl Wanted {
    /// Gathers what setup should cover from the endings, sites and the
    /// web server's certificate file.
    pub fn new(os: Os, endings: &[String], domains: &[String], certificate: &Path) -> Self {
        let mut endings: Vec<String> = endings
            .iter()
            .filter_map(|e| domain::normalize_tld(e).ok())
            .collect();
        endings.sort();
        endings.dedup();
        let mut domains: Vec<String> = domains
            .iter()
            .filter(|d| is_domain(d))
            .map(|d| d.to_ascii_lowercase())
            .collect();
        domains.sort();
        domains.dedup();
        let certificate = std::fs::read_to_string(certificate)
            .ok()
            .filter(|pem| is_certificate(pem));
        Self {
            os,
            resolver: Resolver::for_this_system(os),
            endings,
            domains,
            certificate,
        }
    }

    pub fn status(&self, applied: &Applied) -> Status {
        let mut needed = Vec::new();
        let same_resolver = applied.resolver == Some(self.resolver);
        let names_ok = same_resolver
            && match self.resolver {
                Resolver::HostsFile => {
                    self.domains.iter().all(|d| applied.domains.contains(d))
                        && (self.os != Os::Windows || applied.hosts_writable)
                }
                _ => self.endings.iter().all(|e| applied.endings.contains(e)),
            };
        if !names_ok {
            needed.push(match self.resolver {
                Resolver::HostsFile => "Add your sites to the hosts file".to_string(),
                _ => format!("Send {} names to Stackyard", self.endings.join(", ")),
            });
        }
        let mut trusted = true;
        if let Some(pem) = &self.certificate {
            if applied.certificate.as_deref() != Some(&sha256(pem)) {
                needed.push("Trust Stackyard's local HTTPS certificate".into());
                trusted = false;
            }
        }
        let ready_domains = if same_resolver && trusted {
            self.domains
                .iter()
                .filter(|d| match self.resolver {
                    Resolver::HostsFile => applied.domains.contains(d),
                    _ => applied.endings.iter().any(|e| d.ends_with(e.as_str())),
                })
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        if self.os == Os::Linux && !applied.ports {
            needed.push("Let the web server use ports 80 and 443".into());
        }
        Status {
            needed,
            waiting_for_certificate: self.certificate.is_none(),
            ready_domains,
        }
    }

    /// What applying this setup records.
    pub fn applied(&self) -> Applied {
        Applied {
            resolver: Some(self.resolver),
            endings: self.endings.clone(),
            domains: self.domains.clone(),
            certificate: self.certificate.as_deref().map(sha256),
            ports: self.os == Os::Linux,
            hosts_writable: self.os == Os::Windows && self.resolver == Resolver::HostsFile,
        }
    }

    /// The script that makes it so: POSIX `sh` on macOS and Linux,
    /// PowerShell on Windows. Every value in it has been checked first.
    pub fn script(&self) -> String {
        self.script_for(None)
    }

    /// The script, with `user` (a Windows account SID) allowed to change
    /// the hosts file from then on.
    fn script_for(&self, user: Option<&str>) -> String {
        if self.os == Os::Windows {
            self.powershell(user)
        } else {
            self.shell()
        }
    }

    fn shell(&self) -> String {
        let mut s = String::from("set -e\n");
        match self.resolver {
            Resolver::MacResolver => {
                s.push_str("mkdir -p /etc/resolver\n");
                for ending in &self.endings {
                    let file = ending.trim_start_matches('.');
                    s.push_str(&format!(
                        "printf 'nameserver 127.0.0.1\\nport {}\\n' > /etc/resolver/{file}\n",
                        dns::PORT
                    ));
                }
            }
            Resolver::SystemdResolved => {
                let routes: Vec<String> = self
                    .endings
                    .iter()
                    .map(|e| format!("~{}", e.trim_start_matches('.')))
                    .collect();
                s.push_str("mkdir -p /etc/systemd/resolved.conf.d\n");
                s.push_str(&format!(
                    "printf '[Resolve]\\nDNS=127.0.0.1:{}\\nDomains={}\\n' > /etc/systemd/resolved.conf.d/stackyard.conf\n",
                    dns::PORT,
                    routes.join(" ")
                ));
                s.push_str("systemctl restart systemd-resolved\n");
            }
            Resolver::HostsFile => {
                s.push_str(&format!(
                    "sed -i.stackyard-backup '/^{BEGIN}$/,/^{END}$/d' /etc/hosts\n"
                ));
                s.push_str("cat >> /etc/hosts <<'STACKYARD_END'\n");
                s.push_str(&hosts_block(&self.domains));
                s.push_str("STACKYARD_END\n");
            }
        }
        if let Some(pem) = &self.certificate {
            s.push_str(&format!(
                "cert=$(mktemp)\ncat > \"$cert\" <<'STACKYARD_END'\n{}STACKYARD_END\n",
                pem_body(pem)
            ));
            if self.os == Os::Macos {
                s.push_str(
                    "security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain \"$cert\"\n",
                );
            } else {
                // Debian and Ubuntu, then Fedora, RHEL, Arch and openSUSE.
                s.push_str(&format!(
                    "if command -v update-ca-certificates >/dev/null 2>&1; then\n\
                     \x20 install -m 644 \"$cert\" /usr/local/share/ca-certificates/{CERT_NAME}.crt\n\
                     \x20 update-ca-certificates\n\
                     elif command -v update-ca-trust >/dev/null 2>&1; then\n\
                     \x20 install -m 644 \"$cert\" /etc/pki/ca-trust/source/anchors/{CERT_NAME}.crt\n\
                     \x20 update-ca-trust\n\
                     else\n\
                     \x20 echo 'No certificate tool found (update-ca-certificates or update-ca-trust)' >&2\n\
                     \x20 exit 1\n\
                     fi\n"
                ));
            }
            s.push_str("rm -f \"$cert\"\n");
        }
        if self.os == Os::Linux {
            s.push_str(
                "printf 'net.ipv4.ip_unprivileged_port_start = 80\\n' > /etc/sysctl.d/50-stackyard.conf\n\
                 sysctl -q -w net.ipv4.ip_unprivileged_port_start=80\n",
            );
        }
        s
    }

    fn powershell(&self, user: Option<&str>) -> String {
        let mut s = String::from("$ErrorActionPreference = 'Stop'\n");
        // Hosts file: drop the old block, add the new one.
        s.push_str(&format!(
            "$hosts = Join-Path $env:SystemRoot 'System32\\drivers\\etc\\hosts'\n\
             $keep = New-Object System.Collections.Generic.List[string]\n\
             $skip = $false\n\
             foreach ($line in (Get-Content -LiteralPath $hosts -ErrorAction SilentlyContinue)) {{\n\
             \x20 if ($line -eq '{BEGIN}') {{ $skip = $true; continue }}\n\
             \x20 if ($line -eq '{END}') {{ $skip = $false; continue }}\n\
             \x20 if (-not $skip) {{ $keep.Add($line) }}\n\
             }}\n"
        ));
        for line in hosts_block(&self.domains).lines() {
            s.push_str(&format!("$keep.Add('{line}')\n"));
        }
        s.push_str(
            "Set-Content -LiteralPath $hosts -Value $keep -Encoding ASCII\n\
             ipconfig /flushdns | Out-Null\n",
        );
        if let Some(sid) = user.filter(|s| is_sid(s)) {
            // Lets Stackyard, running as this user, keep its block current.
            s.push_str(&format!(
                "& icacls.exe $hosts /grant '*{sid}:(M)' | Out-Null\n\
                 if ($LASTEXITCODE -ne 0) {{ throw 'icacls could not change the hosts file permissions' }}\n"
            ));
        }
        if let Some(pem) = &self.certificate {
            s.push_str(&format!(
                "$cert = New-TemporaryFile\n\
                 Set-Content -LiteralPath $cert -Encoding ASCII -Value @'\n{}'@\n\
                 Import-Certificate -FilePath $cert -CertStoreLocation Cert:\\LocalMachine\\Root | Out-Null\n\
                 Remove-Item -LiteralPath $cert\n",
                pem_body(pem)
            ));
        }
        s
    }
}

/// The marked hosts file block: one name per line (Windows reads only a
/// few names per line).
fn hosts_block(domains: &[String]) -> String {
    let mut block = format!("{BEGIN}\n");
    for d in domains {
        block.push_str(&format!("127.0.0.1 {d}\n"));
    }
    block.push_str(END);
    block.push('\n');
    block
}

/// A Windows security identifier like `S-1-5-21-1-2-3-1001`.
fn is_sid(text: &str) -> bool {
    text.strip_prefix("S-1-").is_some_and(|rest| {
        !rest.is_empty()
            && rest
                .split('-')
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    })
}

/// The hosts file with Stackyard's block replaced by one for `domains`,
/// keeping everything else and the file's own line endings.
fn with_block(text: &str, domains: &[String]) -> String {
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut out = String::new();
    let mut skip = false;
    for line in text.lines() {
        let bare = line.trim_end_matches('\r');
        if bare == BEGIN {
            skip = true;
        } else if bare == END {
            skip = false;
        } else if !skip {
            out.push_str(bare);
            out.push_str(newline);
        }
    }
    for line in hosts_block(domains).lines() {
        out.push_str(line);
        out.push_str(newline);
    }
    out
}

/// Where the hosts file is.
pub fn hosts_path() -> PathBuf {
    if cfg!(windows) {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        PathBuf::from(root).join(r"System32\drivers\etc\hosts")
    } else {
        PathBuf::from("/etc/hosts")
    }
}

/// Keeps the hosts file's Stackyard block in step with the sites, without
/// a password prompt, once setup let the user's account change the file.
/// Returns whether the file was changed. When it can't be written any more
/// (its permissions were reset), setup is shown as needed again.
pub fn sync_hosts(home: &Path, wanted: &Wanted) -> Result<bool> {
    sync_hosts_at(home, wanted, &hosts_path())
}

fn sync_hosts_at(home: &Path, wanted: &Wanted, hosts: &Path) -> Result<bool> {
    let mut applied = applied(home);
    if !applied.hosts_writable
        || wanted.resolver != Resolver::HostsFile
        || applied.resolver != Some(Resolver::HostsFile)
    {
        return Ok(false);
    }
    let text = std::fs::read_to_string(hosts).unwrap_or_default();
    let next = with_block(&text, &wanted.domains);
    let changed = next != text;
    if changed {
        if let Err(e) = std::fs::write(hosts, &next) {
            applied.hosts_writable = false;
            write_toml(&applied_file(home), &applied)?;
            return Err(Error::SystemSetup(format!(
                "could not update the hosts file: {e}"
            )));
        }
        if wanted.os == Os::Windows {
            let mut flush = Command::new("ipconfig");
            flush.arg("/flushdns");
            crate::platform::hide_console(&mut flush);
            let _ = flush.output();
        }
    }
    if changed || applied.domains != wanted.domains {
        applied.domains = wanted.domains.clone();
        write_toml(&applied_file(home), &applied)?;
    }
    Ok(changed)
}

/// The account Stackyard runs as (Windows), for setup to give it the
/// right to change the hosts file. Asked before the password prompt, since
/// an elevated script may run as another (administrator) account.
fn current_user_sid() -> Option<String> {
    let output =
        powershell_command("[System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value")
            .output()
            .ok()?;
    let sid = String::from_utf8_lossy(&output.stdout).trim().to_string();
    is_sid(&sid).then_some(sid)
}

/// Only letters, digits, hyphens and dots, so a name is safe in any script.
fn is_domain(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 253
        && name.split('.').all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

/// One PEM certificate and nothing else, so it is safe to embed.
fn is_certificate(pem: &str) -> bool {
    let body = pem.trim();
    let Some(inner) = body
        .strip_prefix("-----BEGIN CERTIFICATE-----")
        .and_then(|b| b.strip_suffix("-----END CERTIFICATE-----"))
    else {
        return false;
    };
    let base64: String = inner.split_whitespace().collect();
    !base64.is_empty()
        && base64::engine::general_purpose::STANDARD
            .decode(base64.as_bytes())
            .is_ok()
}

/// The certificate re-wrapped from its own base64, so the embedded text is
/// exactly what was checked.
fn pem_body(pem: &str) -> String {
    let base64: String = pem
        .trim()
        .trim_start_matches("-----BEGIN CERTIFICATE-----")
        .trim_end_matches("-----END CERTIFICATE-----")
        .split_whitespace()
        .collect();
    let mut out = String::from("-----BEGIN CERTIFICATE-----\n");
    for chunk in base64.as_bytes().chunks(64) {
        out.push_str(std::str::from_utf8(chunk).unwrap_or_default());
        out.push('\n');
    }
    out.push_str("-----END CERTIFICATE-----\n");
    out
}

fn sha256(text: &str) -> String {
    let digest = Sha256::digest(pem_body(text).as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// How to get administrator rights for the script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Elevate {
    /// The system's own password dialog (for the desktop app).
    Dialog,
    /// `sudo` in the terminal (for the CLI).
    Sudo,
    /// Already running as an administrator.
    AlreadyAdmin,
}

/// Runs the script with administrator rights and records what it applied.
/// A user-level step follows on Linux: Chrome and Firefox there keep their
/// own certificate list, which `certutil` updates when it is installed.
pub fn apply(home: &Path, wanted: &Wanted, how: Elevate) -> Result<()> {
    let user = if wanted.os == Os::Windows {
        current_user_sid()
    } else {
        None
    };
    let script = wanted.script_for(user.as_deref());
    let output = elevated(wanted.os, &script, how)
        .output()
        .map_err(|e| Error::SystemSetup(format!("could not ask for administrator rights: {e}")))?;
    if !output.status.success() {
        let mut text = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if text.is_empty() {
            text = format!(
                "it ended with {} (was the password prompt cancelled?)",
                output.status
            );
        }
        return Err(Error::SystemSetup(text));
    }
    if wanted.os == Os::Linux {
        if let Some(pem) = &wanted.certificate {
            trust_in_browsers(pem);
        }
    }
    let mut applied = wanted.applied();
    applied.hosts_writable &= user.is_some();
    write_toml(&applied_file(home), &applied)
}

fn elevated(os: Os, script: &str, how: Elevate) -> Command {
    match (os, how) {
        (Os::Windows, Elevate::AlreadyAdmin) => powershell_command(script),
        (Os::Windows, _) => {
            // UAC: start an elevated PowerShell with the script, wait for it
            // and pass on its exit code.
            let encoded = encode_powershell(script);
            powershell_command(&format!(
                "$p = Start-Process -FilePath powershell -Verb RunAs -Wait -PassThru -WindowStyle Hidden \
                 -ArgumentList '-NoProfile','-NonInteractive','-EncodedCommand','{encoded}'; exit $p.ExitCode"
            ))
        }
        (_, Elevate::AlreadyAdmin) => {
            let mut c = Command::new("/bin/sh");
            c.args(["-c", script]);
            c
        }
        (_, Elevate::Sudo) => {
            let mut c = Command::new("sudo");
            c.args(["/bin/sh", "-c", script]);
            c
        }
        (Os::Macos, Elevate::Dialog) => {
            // The script goes in as an argument, so it needs no quoting.
            let mut c = Command::new("osascript");
            c.args([
                "-e",
                "on run argv",
                "-e",
                "do shell script (item 1 of argv) with administrator privileges",
                "-e",
                "end run",
                script,
            ]);
            c
        }
        (Os::Linux, Elevate::Dialog) => {
            let mut c = Command::new("pkexec");
            c.args(["/bin/sh", "-c", script]);
            c
        }
    }
}

pub(crate) fn powershell_command(script: &str) -> Command {
    let mut c = Command::new("powershell");
    c.args([
        "-NoProfile",
        "-NonInteractive",
        "-EncodedCommand",
        &encode_powershell(script),
    ]);
    crate::platform::hide_console(&mut c);
    c
}

/// PowerShell's `-EncodedCommand`: base64 of UTF-16LE.
fn encode_powershell(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Best effort: adds the certificate to the user's shared browser
/// certificate list (`~/.pki/nssdb`) when `certutil` is installed.
fn trust_in_browsers(pem: &str) {
    let Some(home) = dirs::home_dir() else {
        return;
    };
    let db = home.join(".pki/nssdb");
    if !db.is_dir() {
        return;
    }
    let file = std::env::temp_dir().join(format!("{CERT_NAME}-{}.crt", std::process::id()));
    if std::fs::write(&file, pem_body(pem)).is_err() {
        return;
    }
    let _ = Command::new("certutil")
        .args([
            "-d",
            &format!("sql:{}", db.display()),
            "-A",
            "-t",
            "C,,",
            "-n",
            "Stackyard local CA",
            "-i",
        ])
        .arg(&file)
        .output();
    let _ = std::fs::remove_file(file);
}

#[cfg(test)]
mod tests {
    use super::*;

    const PEM: &str = "-----BEGIN CERTIFICATE-----\nTUlJQmtUQ0NBVGVnQXdJQkFnSVFkZXZ0ZXN0\n-----END CERTIFICATE-----\n";

    fn wanted(os: Os, resolver: Resolver) -> Wanted {
        let dir = crate::testutil::tempdir();
        let cert = dir.join("root.crt");
        std::fs::write(&cert, PEM).unwrap();
        let mut w = Wanted::new(
            os,
            &[".stack".into(), "stack".into(), "Test".into()],
            &[
                "b.stack".into(),
                "a.stack".into(),
                "bad name; rm -rf /".into(),
            ],
            &cert,
        );
        w.resolver = resolver;
        w
    }

    #[test]
    fn cleans_what_goes_into_scripts() {
        let w = wanted(Os::Linux, Resolver::HostsFile);
        assert_eq!(w.endings, [".stack", ".test"]);
        assert_eq!(w.domains, ["a.stack", "b.stack"]);
        assert!(w.certificate.is_some());
        assert!(!is_certificate(
            "-----BEGIN CERTIFICATE-----\n'@; evil\n-----END CERTIFICATE-----"
        ));
        assert!(!is_certificate("hello"));
        let none = Wanted::new(Os::Linux, &[], &[], Path::new("/missing.crt"));
        assert!(none.certificate.is_none());
    }

    #[test]
    fn macos_uses_resolver_files_and_the_keychain() {
        let script = wanted(Os::Macos, Resolver::MacResolver).script();
        assert!(script.contains("> /etc/resolver/stack\n"), "{script}");
        assert!(script.contains("> /etc/resolver/test\n"), "{script}");
        assert!(script.contains(&format!("port {}", dns::PORT)));
        assert!(script.contains("security add-trusted-cert"));
        assert!(!script.contains("sysctl"));
    }

    #[test]
    fn linux_routes_endings_and_lowers_ports() {
        let script = wanted(Os::Linux, Resolver::SystemdResolved).script();
        assert!(script.contains("Domains=~stack ~test"), "{script}");
        assert!(script.contains("update-ca-certificates"));
        assert!(script.contains("ip_unprivileged_port_start=80"));

        let hosts = wanted(Os::Linux, Resolver::HostsFile).script();
        assert!(
            hosts.contains("127.0.0.1 a.stack\n127.0.0.1 b.stack\n# END Stackyard"),
            "{hosts}"
        );
        assert!(!hosts.contains("rm -rf"));
    }

    #[test]
    fn windows_edits_hosts_and_imports_the_certificate() {
        let script = wanted(Os::Windows, Resolver::HostsFile).script();
        assert!(
            script.contains("$keep.Add('127.0.0.1 a.stack')"),
            "{script}"
        );
        assert!(script.contains("Cert:\\LocalMachine\\Root"));
        assert_eq!(
            encode_powershell("ab"),
            base64::engine::general_purpose::STANDARD.encode([b'a', 0, b'b', 0])
        );
    }

    #[test]
    fn status_says_what_is_left() {
        let w = wanted(Os::Linux, Resolver::SystemdResolved);
        let fresh = w.status(&Applied::default());
        assert_eq!(fresh.needed.len(), 3, "{:?}", fresh.needed);
        assert!(w.status(&w.applied()).needed.is_empty());

        // A new ending needs setup again; a new site does not.
        let mut more = w.clone();
        more.endings.push(".dev".into());
        more.domains.push("c.stack".into());
        assert_eq!(more.status(&w.applied()).needed.len(), 1);

        // With the hosts file, a new site does.
        let hosts = wanted(Os::Windows, Resolver::HostsFile);
        let mut more = hosts.clone();
        more.domains.push("c.stack".into());
        assert_eq!(more.status(&hosts.applied()).needed.len(), 1);
        assert!(hosts.status(&hosts.applied()).needed.is_empty());
        // ...but the sites already in the hosts file keep working.
        assert_eq!(
            more.status(&hosts.applied()).ready_domains,
            ["a.stack", "b.stack"]
        );
        assert!(more.status(&Applied::default()).ready_domains.is_empty());

        // With resolver files every site under a set-up ending works.
        let mut more = w.clone();
        more.domains.push("c.stack".into());
        more.domains.push("d.dev".into());
        assert_eq!(
            more.status(&w.applied()).ready_domains,
            ["a.stack", "b.stack", "c.stack"]
        );
    }

    #[test]
    fn windows_setup_lets_the_user_keep_the_hosts_file_current() {
        let w = wanted(Os::Windows, Resolver::HostsFile);
        let script = w.script_for(Some("S-1-5-21-11-22-33-1001"));
        assert!(
            script.contains("icacls.exe $hosts /grant '*S-1-5-21-11-22-33-1001:(M)'"),
            "{script}"
        );
        assert!(!w.script_for(Some("S-1-5'; evil")).contains("icacls"));
        assert!(!w.script().contains("icacls"));
        assert!(!is_sid("S-1-"));
        assert!(!is_sid("S-1-5--1"));

        // Set up before Stackyard could keep the file current: once more.
        let mut old = w.applied();
        old.hosts_writable = false;
        assert_eq!(
            w.status(&old).needed.len(),
            1,
            "{:?}",
            w.status(&old).needed
        );
        assert!(w.status(&w.applied()).needed.is_empty());
    }

    #[test]
    fn rewrites_only_its_own_block_and_keeps_line_endings() {
        let text = format!(
            "127.0.0.1 localhost\r\n{BEGIN}\r\n127.0.0.1 old.stack\r\n{END}\r\n10.0.0.1 other\r\n"
        );
        assert_eq!(
            with_block(&text, &["a.stack".into()]),
            format!("127.0.0.1 localhost\r\n10.0.0.1 other\r\n{BEGIN}\r\n127.0.0.1 a.stack\r\n{END}\r\n")
        );
        assert_eq!(with_block("", &[]), format!("{BEGIN}\n{END}\n"));
    }

    #[test]
    fn new_sites_go_into_the_hosts_file_without_setup() {
        let home = crate::testutil::tempdir();
        let hosts = home.join("hosts");
        std::fs::write(&hosts, "127.0.0.1 localhost\n").unwrap();
        let w = wanted(Os::Windows, Resolver::HostsFile);
        // Not set up yet: nothing is touched.
        assert!(!sync_hosts_at(&home, &w, &hosts).unwrap());
        write_toml(&applied_file(&home), &w.applied()).unwrap();

        let mut more = w.clone();
        more.domains.push("c.stack".into());
        assert!(more.status(&applied(&home)).needed.len() == 1);
        assert!(sync_hosts_at(&home, &more, &hosts).unwrap());
        assert_eq!(
            std::fs::read_to_string(&hosts).unwrap(),
            format!("127.0.0.1 localhost\n{BEGIN}\n127.0.0.1 a.stack\n127.0.0.1 b.stack\n127.0.0.1 c.stack\n{END}\n")
        );
        assert!(more.status(&applied(&home)).needed.is_empty());
        // Already current: no write.
        assert!(!sync_hosts_at(&home, &more, &hosts).unwrap());
    }

    #[test]
    fn runs_the_shell_script_when_already_admin() {
        // The hosts-file part, pointed at a scratch copy instead of /etc/hosts.
        if cfg!(windows) {
            return;
        }
        let dir = crate::testutil::tempdir();
        let hosts = dir.join("hosts");
        std::fs::write(
            &hosts,
            format!("127.0.0.1 localhost\n{BEGIN}\n127.0.0.1 old.stack\n{END}\n"),
        )
        .unwrap();
        let w = Wanted {
            os: Os::Macos,
            resolver: Resolver::HostsFile,
            endings: vec![],
            domains: vec!["new.stack".into()],
            certificate: None,
        };
        let script = w
            .script()
            .replace("/etc/hosts", &hosts.display().to_string());
        let out = elevated(Os::Linux, &script, Elevate::AlreadyAdmin)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let text = std::fs::read_to_string(&hosts).unwrap();
        assert_eq!(
            text,
            format!("127.0.0.1 localhost\n{BEGIN}\n127.0.0.1 new.stack\n{END}\n")
        );
    }
}
