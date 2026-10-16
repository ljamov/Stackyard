//! Git for a site's folder: link it to a repository on GitHub, GitLab,
//! Bitbucket or any other host, then pull, commit and push.
//!
//! This drives the Git installed on the computer rather than bundling one,
//! so sign-in works as it already does there: Git Credential Manager on
//! Windows (a browser sign-in for GitHub), the Keychain on macOS, SSH keys
//! anywhere. Git is never allowed to wait for a password typed in a
//! terminal, since there is none.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;

use crate::{platform, Error, Result};

/// Changed files listed at most; the rest are counted.
const MAX_CHANGES: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    /// New and not yet added to Git.
    New,
    /// Changed on both sides of a pull; needs fixing by hand.
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    pub path: String,
    pub kind: ChangeKind,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Status {
    /// Git is installed.
    pub installed: bool,
    /// The folder is in a Git repository.
    pub repo: bool,
    /// The repository's top folder, when the site is inside a larger one.
    pub root: Option<String>,
    pub branch: Option<String>,
    /// e.g. `origin/main`; none until the branch is first pushed or pulled.
    pub upstream: Option<String>,
    /// Commits here that the server does not have yet.
    pub ahead: u32,
    /// Commits on the server not pulled yet (as of the last fetch).
    pub behind: u32,
    /// Where `origin` points, without any password in it.
    pub remote: Option<String>,
    pub changes: Vec<Change>,
    /// Changes beyond those listed.
    pub more_changes: usize,
    /// A name and email are set for commits.
    pub identity: bool,
}

/// The `git` program: on `PATH`, else where Git for Windows installs.
pub fn program() -> Option<PathBuf> {
    let name = if cfg!(windows) { "git.exe" } else { "git" };
    let on_path = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .map(|dir| dir.join(name))
        .find(|p| p.is_file());
    on_path.or_else(|| {
        if !cfg!(windows) {
            return None;
        }
        ["ProgramFiles", "ProgramW6432", "LOCALAPPDATA"]
            .iter()
            .filter_map(std::env::var_os)
            .flat_map(|base| {
                let base = PathBuf::from(base);
                [
                    base.join("Git").join("cmd").join(name),
                    base.join("Programs").join("Git").join("cmd").join(name),
                ]
            })
            .find(|p| p.is_file())
    })
}

fn git_command(dir: &Path) -> Result<Command> {
    let program = program().ok_or_else(|| {
        Error::Git("Git is not installed; get it from git-scm.com, then try again".into())
    })?;
    let mut command = Command::new(program);
    command
        .current_dir(dir)
        .args(["-c", "core.quotePath=false", "-c", "color.ui=false"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    platform::hide_console(&mut command);
    Ok(command)
}

/// Commands that talk to the server, maybe over SSH.
const NETWORK: &[&str] = &["clone", "fetch", "pull", "push", "ls-remote"];

/// Makes SSH fail rather than wait for an answer nobody can type, unless
/// the user set up their own SSH command for Git.
fn batch_ssh(command: &mut Command, dir: &Path) {
    if std::env::var_os("GIT_SSH_COMMAND").is_some() || std::env::var_os("GIT_SSH").is_some() {
        return;
    }
    let configured = git_command(dir)
        .ok()
        .and_then(|mut c| c.args(["config", "core.sshCommand"]).output().ok())
        .is_some_and(|o| o.status.success() && !o.stdout.trim_ascii().is_empty());
    if !configured {
        command.env(
            "GIT_SSH_COMMAND",
            "ssh -o BatchMode=yes -o StrictHostKeyChecking=accept-new",
        );
    }
}

fn run(dir: &Path, args: &[&str]) -> Result<String> {
    let mut command = git_command(dir)?;
    if args.first().is_some_and(|a| NETWORK.contains(a)) {
        batch_ssh(&mut command, dir);
    }
    let output = command
        .args(args)
        .output()
        .map_err(|e| Error::Git(format!("could not run Git: {e}")))?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    Err(Error::Git(explain(&String::from_utf8_lossy(
        &output.stderr,
    ))))
}

/// Git's own message, without its hints, and with the usual sign-in
/// failures put plainly.
fn explain(stderr: &str) -> String {
    let text: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("hint:"))
        .collect();
    let joined = text.join("\n");
    let lower = joined.to_lowercase();
    if lower.contains("terminal prompts disabled")
        || lower.contains("could not read username")
        || lower.contains("authentication failed")
        || lower.contains("permission denied (publickey")
    {
        return format!(
            "Git could not sign in to the server. Sign in once with Git (for GitHub, Git for Windows opens a browser sign-in on the first push or pull), or use an SSH address with a key that is set up.\n{joined}"
        );
    }
    if joined.is_empty() {
        "Git failed without saying why".into()
    } else {
        joined
    }
}

/// `https://user:token@github.com/me/app.git` → `https://github.com/me/app.git`.
fn without_password(url: &str) -> String {
    if let Some((scheme, rest)) = url.split_once("://") {
        if let Some((user_info, host)) = rest.split_once('@') {
            if !user_info.contains('/') {
                return format!("{scheme}://{host}");
            }
        }
    }
    url.to_string()
}

pub fn status(dir: &Path) -> Result<Status> {
    let mut status = Status {
        installed: program().is_some(),
        ..Status::default()
    };
    if !status.installed || !dir.is_dir() {
        return Ok(status);
    }
    let Ok(top) = run(dir, &["rev-parse", "--show-toplevel"]) else {
        return Ok(status);
    };
    status.repo = true;
    let top = PathBuf::from(top.trim());
    let same = match (top.canonicalize(), dir.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => true,
    };
    if !same {
        status.root = Some(top.display().to_string());
    }
    parse_status(
        &run(dir, &["status", "--porcelain=v2", "--branch"])?,
        &mut status,
    );
    status.remote = run(dir, &["remote", "get-url", "origin"])
        .ok()
        .map(|url| without_password(url.trim()))
        .filter(|url| !url.is_empty());
    let set = |key| run(dir, &["config", key]).is_ok_and(|v| !v.trim().is_empty());
    status.identity = set("user.name") && set("user.email");
    Ok(status)
}

fn parse_status(text: &str, status: &mut Status) {
    for line in text.lines() {
        if let Some(header) = line.strip_prefix("# ") {
            let (key, value) = header.split_once(' ').unwrap_or((header, ""));
            match key {
                "branch.head" if value != "(detached)" => status.branch = Some(value.into()),
                "branch.upstream" => status.upstream = Some(value.into()),
                "branch.ab" => {
                    for part in value.split(' ') {
                        if let Some(n) = part.strip_prefix('+') {
                            status.ahead = n.parse().unwrap_or(0);
                        } else if let Some(n) = part.strip_prefix('-') {
                            status.behind = n.parse().unwrap_or(0);
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        let (kind, path) = match line.split_at_checked(2) {
            Some(("? ", path)) => (ChangeKind::New, path.to_string()),
            Some(("1 ", rest)) => match rest.splitn(8, ' ').collect::<Vec<_>>()[..] {
                [xy, _, _, _, _, _, _, path] => (kind_of(xy), path.to_string()),
                _ => continue,
            },
            Some(("2 ", rest)) => match rest.splitn(9, ' ').collect::<Vec<_>>()[..] {
                [_, _, _, _, _, _, _, _, paths] => (
                    ChangeKind::Renamed,
                    paths.split('\t').next().unwrap_or(paths).to_string(),
                ),
                _ => continue,
            },
            Some(("u ", rest)) => match rest.splitn(10, ' ').collect::<Vec<_>>()[..] {
                [_, _, _, _, _, _, _, _, _, path] => (ChangeKind::Conflict, path.to_string()),
                _ => continue,
            },
            _ => continue,
        };
        if status.changes.len() < MAX_CHANGES {
            status.changes.push(Change { path, kind });
        } else {
            status.more_changes += 1;
        }
    }
}

/// `XY` from `git status`: the change in the folder wins over the staged one.
fn kind_of(xy: &str) -> ChangeKind {
    let mut chars = xy.chars();
    let (x, y) = (chars.next().unwrap_or('.'), chars.next().unwrap_or('.'));
    match if y != '.' { y } else { x } {
        'A' => ChangeKind::Added,
        'D' => ChangeKind::Deleted,
        'R' | 'C' => ChangeKind::Renamed,
        _ => ChangeKind::Modified,
    }
}

fn check_url(url: &str) -> Result<&str> {
    let url = url.trim();
    if url.is_empty() || url.starts_with('-') || url.contains(char::is_whitespace) {
        return Err(Error::Git(
            "enter the repository's address, e.g. https://github.com/you/app.git".into(),
        ));
    }
    Ok(url)
}

fn is_empty_dir(dir: &Path) -> bool {
    std::fs::read_dir(dir).map_or(true, |mut entries| entries.next().is_none())
}

/// Links the site's folder to a repository. An empty folder gets a copy
/// of the repository (`git clone`). A folder with files becomes a
/// repository pointing at it, with its files left exactly as they are:
/// where they differ from the repository, they show as changes to commit.
pub fn link(dir: &Path, url: &str) -> Result<Status> {
    let url = check_url(url)?;
    let is_repo = dir.is_dir() && run(dir, &["rev-parse", "--show-toplevel"]).is_ok();
    if is_repo {
        let has_origin = run(dir, &["remote", "get-url", "origin"]).is_ok();
        let verb = if has_origin { "set-url" } else { "add" };
        run(dir, &["remote", verb, "origin", url])?;
        return status(dir);
    }
    if is_empty_dir(dir) {
        let parent = dir
            .parent()
            .ok_or_else(|| Error::Git("the site's folder has no parent folder".into()))?;
        std::fs::create_dir_all(dir).map_err(crate::remote::io_error(dir))?;
        let target = dir.display().to_string();
        run(parent, &["clone", "--", url, &target])?;
        return status(dir);
    }
    run(dir, &["init", "--quiet"])?;
    run(dir, &["remote", "add", "origin", url])?;
    run(dir, &["fetch", "--quiet", "origin"])?;
    if let Some(branch) = default_branch(dir) {
        // Point the new repository at the server's latest commit without
        // touching any file, so the differences show up as changes.
        run(
            dir,
            &["symbolic-ref", "HEAD", &format!("refs/heads/{branch}")],
        )?;
        run(dir, &["reset", "--quiet", &format!("origin/{branch}")])?;
        run(
            dir,
            &[
                "branch",
                "--quiet",
                "--set-upstream-to",
                &format!("origin/{branch}"),
            ],
        )?;
    }
    status(dir)
}

/// The server's main branch, e.g. `main`; none for an empty repository.
fn default_branch(dir: &Path) -> Option<String> {
    let text = run(dir, &["ls-remote", "--symref", "origin", "HEAD"]).ok()?;
    let from_head = text.lines().find_map(|line| {
        line.strip_prefix("ref: refs/heads/")
            .and_then(|rest| rest.split_whitespace().next())
            .map(str::to_string)
    });
    from_head.or_else(|| {
        ["main", "master"]
            .into_iter()
            .find(|b| {
                run(
                    dir,
                    &["rev-parse", "--verify", "--quiet", &format!("origin/{b}")],
                )
                .is_ok()
            })
            .map(str::to_string)
    })
}

/// Fetches from the server, so "behind" counts are current.
pub fn fetch(dir: &Path) -> Result<Status> {
    run(dir, &["fetch", "--quiet", "--prune", "origin"])?;
    status(dir)
}

/// Brings in the server's commits; never merges or rewrites, so it stops
/// with Git's message when both sides have new commits.
pub fn pull(dir: &Path) -> Result<Status> {
    let current = status(dir)?;
    if current.upstream.is_some() {
        run(dir, &["pull", "--ff-only", "--quiet"])?;
    } else {
        let branch = current
            .branch
            .ok_or_else(|| Error::Git("check out a branch first".into()))?;
        run(dir, &["pull", "--ff-only", "--quiet", "origin", &branch])?;
        let _ = run(
            dir,
            &[
                "branch",
                "--quiet",
                "--set-upstream-to",
                &format!("origin/{branch}"),
            ],
        );
    }
    status(dir)
}

/// Commits every change in the folder. A name and email, when given, are
/// saved for this repository first.
pub fn commit(dir: &Path, message: &str, identity: Option<(&str, &str)>) -> Result<Status> {
    let message = message.trim();
    if message.is_empty() {
        return Err(Error::Git("write a short note about what changed".into()));
    }
    if let Some((name, email)) = identity {
        let (name, email) = (name.trim(), email.trim());
        if name.is_empty() || !email.contains('@') {
            return Err(Error::Git("enter your name and email for commits".into()));
        }
        run(dir, &["config", "user.name", name])?;
        run(dir, &["config", "user.email", email])?;
    }
    run(dir, &["add", "--all"])?;
    if run(dir, &["diff", "--cached", "--quiet"]).is_ok() {
        return Err(Error::Git(
            "nothing has changed since the last commit".into(),
        ));
    }
    run(dir, &["commit", "--quiet", "-m", message])?;
    status(dir)
}

/// Sends local commits to the server, setting the upstream on first push.
pub fn push(dir: &Path) -> Result<Status> {
    let current = status(dir)?;
    if current.upstream.is_some() {
        run(dir, &["push", "--quiet"])?;
    } else {
        run(
            dir,
            &["push", "--quiet", "--set-upstream", "origin", "HEAD"],
        )?;
    }
    status(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_status_output() {
        let text = "# branch.oid abc\n# branch.head main\n# branch.upstream origin/main\n# branch.ab +2 -1\n\
1 .M N... 100644 100644 100644 abc abc src/app file.php\n\
1 A. N... 000000 100644 100644 000 abc new.txt\n\
2 R. N... 100644 100644 100644 abc abc R100 moved.txt\told.txt\n\
u UU N... 100644 100644 100644 100644 a b c both.txt\n\
? untracked dir/x.js\n";
        let mut status = Status::default();
        parse_status(text, &mut status);
        assert_eq!(status.branch.as_deref(), Some("main"));
        assert_eq!(status.upstream.as_deref(), Some("origin/main"));
        assert_eq!((status.ahead, status.behind), (2, 1));
        let kinds: Vec<_> = status
            .changes
            .iter()
            .map(|c| (c.path.as_str(), c.kind))
            .collect();
        assert_eq!(
            kinds,
            [
                ("src/app file.php", ChangeKind::Modified),
                ("new.txt", ChangeKind::Added),
                ("moved.txt", ChangeKind::Renamed),
                ("both.txt", ChangeKind::Conflict),
                ("untracked dir/x.js", ChangeKind::New),
            ]
        );
    }

    #[test]
    fn hides_passwords_in_addresses() {
        assert_eq!(
            without_password("https://me:ghp_secret@github.com/me/app.git"),
            "https://github.com/me/app.git"
        );
        assert_eq!(
            without_password("git@github.com:me/app.git"),
            "git@github.com:me/app.git"
        );
        assert!(check_url("--upload-pack=evil").is_err());
        assert!(check_url(" ").is_err());
    }

    /// Clones, links a folder that already has files, commits, pushes and
    /// pulls, against a bare repository on disk.
    #[test]
    fn links_commits_pushes_and_pulls() {
        if program().is_none() {
            return;
        }
        std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
        let root = crate::testutil::tempdir();
        let server = root.join("server.git");
        std::fs::create_dir_all(&server).unwrap();
        run(
            &server,
            &["init", "--bare", "--quiet", "--initial-branch=main"],
        )
        .unwrap();
        let url = server.display().to_string();
        let me = Some(("Sam", "sam@example.com"));

        // An empty site folder gets a clone, here of an empty repository.
        let first = root.join("first");
        std::fs::create_dir_all(&first).unwrap();
        let status = link(&first, &url).unwrap();
        assert!(status.repo);
        std::fs::write(first.join("index.php"), "<?php echo 1;").unwrap();
        let status = super::status(&first).unwrap();
        assert_eq!(status.changes[0].kind, ChangeKind::New);
        run(&first, &["checkout", "--quiet", "-b", "main"]).unwrap();
        commit(&first, "First page", me).unwrap();
        let pushed = push(&first).unwrap();
        assert_eq!(pushed.upstream.as_deref(), Some("origin/main"));
        assert_eq!(pushed.ahead, 0);
        assert!(commit(&first, "Again", None).is_err());

        // A folder with its own files keeps them; differences show as changes.
        let second = root.join("second");
        std::fs::create_dir_all(&second).unwrap();
        std::fs::write(second.join("index.php"), "<?php echo 2;").unwrap();
        let status = link(&second, &url).unwrap();
        assert_eq!(status.branch.as_deref(), Some("main"));
        assert_eq!(status.upstream.as_deref(), Some("origin/main"));
        assert_eq!(
            std::fs::read_to_string(second.join("index.php")).unwrap(),
            "<?php echo 2;"
        );
        assert_eq!(status.changes.len(), 1);
        assert_eq!(status.changes[0].kind, ChangeKind::Modified);
        commit(&second, "Second page", me).unwrap();
        push(&second).unwrap();

        let behind = fetch(&first).unwrap();
        assert_eq!(behind.behind, 1);
        let pulled = pull(&first).unwrap();
        assert_eq!(pulled.behind, 0);
        assert_eq!(
            std::fs::read_to_string(first.join("index.php")).unwrap(),
            "<?php echo 2;"
        );
    }
}
