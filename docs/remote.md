# Servers and Git

Each site can be connected to the servers it runs on and to a Git repository.
Open **Sites**, then **Servers & Git** on the site's row.

Both parts are modules (Settings › Modules › *Remote servers* and *Git*). Turned
off, they hide their tab and do nothing.

## Servers (SFTP, FTPS, FTP)

| Type | Use it for |
| --- | --- |
| **SFTP (SSH)** | Cloudways, most managed hosts, any VPS. Logs in with a password or an SSH key. |
| **FTPS** | Shared hosts (cPanel, Plesk, DirectAdmin) that offer FTP with TLS. |
| **FTP** | Only when nothing else is offered: the password and files travel unencrypted. |

A connection pairs a **folder on the server** with the site's folder (or a part
of it, such as `public`). Then:

- **Upload changes** and **Download changes** compare both sides and list what
  would be copied before anything happens. **What to upload** (or download)
  decides which files go: new and changed files (size differs or the side it
  comes from is newer; the default), only newer files, different size or date,
  different size only, only missing files, or everything. Files missing on the
  other side are copied in every mode. Deleting files missing on the side they
  come from is a separate checkbox, off by default.
- **Browse files** lists the server's folders; files and folders under the
  connection's folder can be downloaded into the matching place in the site, and
  files picked from the site's folder uploaded to the matching place on the
  server.
- **Never transfer** skips names anywhere (`.git`, `*.log`) or paths from the
  top (`storage/logs`). `.git`, `node_modules`, `.env` and system clutter are
  skipped by default.
- **WordPress sites** keep their own `wp-config.php` (with this computer's
  database login), `wp-config.live.php`, `.user.ini`, `object-cache.php`,
  `.maintenance` and Stackyard's files on each side: Upload and Download leave
  them alone and say so. Posts, pages and settings are in the database, which
  files don't carry; use WordPress › Live › Pull and Push for those.

Transfers keep modification times, so a file just copied is seen as the same
next time. Downloads are written as `name.part` and renamed once complete.

### Passwords

Passwords and key passphrases are kept in the system's password store: Windows
Credential Manager, the macOS Keychain, or the desktop keyring on Linux
(through `secret-tool`). `remote.toml` in Stackyard's folder holds only the
server address, user name and folders. A Linux system without a keyring falls
back to `config/secrets.toml`, readable only by you.

### Trust

- **SFTP**: the server's key is remembered on the first connection. If it
  changes later, Stackyard refuses to connect and says so; choose *Trust the new
  key* only if you know the server was rebuilt or moved.
- **FTPS**: certificates are checked against the system's trusted roots. Shared
  hosts often use a certificate made for their own server name; **Test** then
  shows its fingerprint and offers *Trust this certificate*, after which only
  that exact certificate is accepted.

### When connecting fails

Every connection error says what failed, whose side the problem is on
(internet or firewall, the server, the login or settings, or Stackyard) and
what to try. **Check connection** (under a failed Test, and under a failed
Clone from a live server) goes through each step and stops at the one that
fails:

| Step | When it fails |
| --- | --- |
| **Find the server** | The address is mistyped, is a web address (`https://…`) instead of a server name, or this computer is offline. |
| **Reach the port** | *Nothing accepts connections*: wrong port or protocol (SFTP is usually 22, FTP 21; some hosts use their own). *No answer*: a firewall, often the host only allowing listed IP addresses or blocking yours after failed logins. |
| **Secure handshake** | The port is FTP or something else, not SSH; or the server closed the connection before login, usually a blocked IP. |
| **Log in** | Wrong user name, password or key. Use the SFTP/FTP user from the hosting panel. |
| **Open a session** | The login is right but the server hung up: SSH/SFTP is off for that user, the account is suspended, or it allows only one or two connections at once (close FileZilla, stop other clones). The server's own reason is shown when it gave one. |
| **Open the files** | SFTP is turned off for the user; turn it on, or use FTPS/FTP. |
| **Run commands** | Only a warning: the user may move files but not run commands. Clone from live then copies the database through a small temporary helper on the website. |

### Cloudways

Use SFTP with the server's **Public IP**, your **application's** SFTP user and
password (Application Management › Access Details), and the folder
`/home/master/applications/<app folder>/public_html`.

## Git

The Git tab uses the Git installed on your computer (on Windows, *Git for
Windows* from git-scm.com), so signing in works as it already does there: Git
Credential Manager opens a browser sign-in for GitHub on the first pull or push,
and SSH addresses use your SSH keys.

- **Link repository**: an empty site folder gets a copy of the repository. A
  folder that already has files keeps them all; it becomes a repository pointed
  at the server's latest commit, and anything that differs shows as changes.
- **Pull** only fast-forwards: when both sides have new commits it stops with
  Git's message instead of merging.
- **Commit all changes** adds every change and commits it. The first time, it
  asks for the name and email to sign commits with (saved for that repository).
- **Push** sends commits, setting up the branch on the server the first time.
- **Check the server** fetches, so the "to pull" count is current.
