# Stackyard

A local development environment for Windows, macOS and Linux. Manage PHP, Node.js and more,
databases, web servers and local HTTPS sites such as `https://myapp.stack` from one desktop app,
with no containers.

> Status: early prototype. Runtimes, services, local sites with HTTPS, terminal commands, the
> background service, the tray icon, installers, the MCP server for AI coding tools and the task
> scheduler work.

## Install

Unsigned prototype installers are built by `.github/workflows/release.yml` for every version tag
(as a draft release) and can be run by hand from the Actions tab:

| System | Files |
| --- | --- |
| Windows 10 and 11 (x64) | `Stackyard_<version>_x64-setup.exe` (installs for you, no admin) or `.msi` |
| macOS 11 or later | `.dmg` for Apple silicon (`aarch64`) or Intel (`x64`) |
| Linux (x64) | `.deb`, `.rpm` or `.AppImage` |

They are not signed yet, so the first start needs a click: on macOS right-click Stackyard and
choose **Open**; on Windows choose **More info**, then **Run anyway**. Each installer ships the
`stackyard` command next to the app; **Add to PATH** on the Runtimes screen (or
`stackyard shims on`) puts it and the terminal commands on your PATH.

## What's here

| Path | What it is |
| --- | --- |
| `crates/stackyard-core` | Core logic shared by the app, the CLI and the MCP server: settings, sites, domain rules, catalog |
| `crates/stackyard-cli` | The `stackyard` command-line tool |
| `src-tauri` | Tauri 2 desktop app: thin commands over the core |
| `src` | React + TypeScript interface |

Settings and sites are stored as TOML in the OS data folder (for example
`~/Library/Application Support/Stackyard` on macOS). Set `STACKYARD_HOME` to use another folder.

## Local domains

New sites end in `.stack` by default. Change it in the app (Sites › Domain ending) or with
`stackyard tld .test`. If `.stack` ever becomes a public domain ending, the default moves to `.test`,
which is reserved for testing. `.local` works but is not recommended because Bonjour/mDNS uses it.

## Sites

Add a site by hand, or park a folder (`stackyard sites park ~/Code`) so every project folder in it
becomes a site named after the folder. Stackyard looks at each project to decide how to serve it:

- **PHP**: Laravel, Symfony, CodeIgniter, Magento, CakePHP, Craft CMS, Yii, Drupal, WordPress
  (including Bedrock), or any folder with an `index.php`. The right public folder is served, through
  `php-fpm` on macOS and Linux and `php-cgi` on Windows, one process per PHP version in use.
  Each version has its own `php.ini` in `config/php/<version>/`, created once with development
  defaults and then left to you.
- **Node**: a project with a `dev` or `start` script is proxied to its dev server. The port comes
  from the script (`--port 4000`) or the framework's usual one (Vite 5173, Next.js 3000, Astro 4321…).
- **Static files**: anything else, from `public/` or `dist/` when the page lives there.

Every site answers at `https://<name><ending>` (`.stack` by default) and at `<name>.localhost`, which
browsers send to your own computer, so sites work before setup.

### Local domains and HTTPS

`stackyard system setup` (or **Set up** on the Sites screen) asks for your password once and:

| | macOS | Linux | Windows |
| --- | --- | --- | --- |
| Local names | `/etc/resolver/<ending>` pointing at Stackyard's DNS server on 127.0.0.1:53535 | A systemd-resolved drop-in doing the same (the hosts file without systemd-resolved) | A marked block in the hosts file |
| HTTPS | Trusts Caddy's local root certificate in the System keychain | Adds it to the system certificates (and Chrome's list when `certutil` is installed) | Adds it to the trusted root certificates |
| Ports 80 and 443 | Already allowed | Lets programs use ports from 80 (`/etc/sysctl.d/50-stackyard.conf`) | Already allowed |

Only Stackyard's domain endings go to its DNS server; everything else resolves as before. The DNS
server answers `127.0.0.1` for names under those endings and refuses everything else. When ports
80 and 443 are taken, the web server uses 8080 and 8443 instead. `stackyard system setup --print`
shows the exact commands first. With the hosts file, adding a site asks for setup again.

## Runtimes

Each version installs into its own folder (`runtimes/php/8.4`, `runtimes/node/22`) inside the data
folder, so versions sit side by side. Every download is checked against a published SHA-256
checksum before it is unpacked.

| Runtime | Windows | macOS and Linux |
| --- | --- | --- |
| Node.js 20, 22, 24, 26 | Official builds from nodejs.org | Official builds from nodejs.org |
| PHP 7.4 to 8.5 | Official builds from the PHP project | Built from the official PHP source by `.github/workflows/php-builds.yml` |

The PHP build job uses static-php-cli to produce self-contained `php` and `php-fpm` binaries for
x86_64 and ARM64, and publishes them with their checksums and licenses on the `php-builds` release.
Download sources can be pointed at a mirror with `STACKYARD_NODE_MIRROR`,
`STACKYARD_PHP_WINDOWS_MIRROR` and `STACKYARD_PHP_MANIFEST`.

## Running in the background

One process at a time runs services, and every other part of Stackyard talks to it:

- **The app.** Closing the window keeps Stackyard in the tray (menu bar on macOS) and frees the
  window's memory; the tray menu opens it again, starts or stops every service, or quits. Quitting
  stops all services. Opening the app a second time brings the first window forward.
- **`stackyard start`** when the app is closed: it starts a small background process
  (`stackyard daemon`) that keeps services running after the terminal closes. `stackyard stop`
  stops everything and ends it; opening the app takes its services over.
- **`stackyard up`**: the same in the foreground of one terminal, until Ctrl+C.

`stackyard start`, `stop`, `status` and site changes from the CLI all reach whichever of these runs
services, so a site added in a terminal is served right away by the app. They talk over
`127.0.0.1` with a secret token kept in `run/control.json`, readable only by you.

## Terminal

`php`, `composer`, `node`, `npm`, `npx`, `mysql` and `mysqldump` run the version each project asks
for. In order, Stackyard uses:

1. the nearest `.stackyard` file in the folder or above it (`stackyard use php 8.3` writes one):
   ```toml
   php = "8.3"
   node = "22"
   ```
2. the version of a site added for that folder,
3. the default version (Runtimes screen, or `stackyard runtimes default php 8.4`),
4. the newest installed version.

Parked sites follow `.stackyard` files too, so the browser and the terminal agree.

`stackyard shims on` (or **Add to PATH** on the Runtimes screen) creates the commands in `bin/`
inside Stackyard's folder and puts that folder on your PATH: a marked block in `.zshrc`, `.bashrc`,
`.bash_profile` or `.profile` and a fish `conf.d` file on macOS and Linux, your user `Path` on
Windows. No password is needed, and `stackyard shims off` undoes it. `stackyard shell` opens one
terminal with the commands instead, without touching your PATH. `stackyard which` shows which version
each command runs in the current folder, and why.

The commands lead back to a copy of the `stackyard` program in the same folder (which also puts
`stackyard` itself on your PATH), and it runs the real one from the runtime's folder.
Composer is downloaded from getcomposer.org the first time you run `composer`, checked against its
published SHA-256. `mysql` connects to whichever of MySQL and MariaDB is running. `psql`, `pg_dump`
and `redis-cli` come from Stackyard's PostgreSQL and Redis, and `psql` and `pg_dump` connect as user
`postgres` without a password.

## Services

Services install into `services/<id>/<line>`, keep their data in `data/<id>` (kept when a service
is removed, so reinstalling brings databases back) and their config in `config/<id>`. Everything
listens on `127.0.0.1` only. MySQL and MariaDB start with user `root` and PostgreSQL with user
`postgres`, all without a password.

| Service | Ports | Source | Checked against |
| --- | --- | --- | --- |
| Caddy 2 (web server) | 80, 443 (8080, 8443 when taken) | Official GitHub releases | Caddy's SHA-512 list |
| MySQL 8.4 | 3306 | Oracle's official downloads | SHA-256 from `.github/workflows/mysql-manifest.yml`, which first checks Oracle's GnuPG signature |
| MariaDB 11.8 (Windows, Linux) | 3306 | MariaDB Foundation downloads | The Foundation's SHA-256 |
| PostgreSQL 18 | 5432 | Built from the official source (macOS, Linux); the official EDB binaries without pgAdmin (Windows) | SHA-256 from `.github/workflows/postgres-builds.yml`, which first checks postgresql.org's SHA-256 |
| Redis 8 (macOS, Linux) | 6379 | Built from the official source | SHA-256 from `.github/workflows/redis-builds.yml`, which first checks Redis's published SHA-256 |
| Mailpit (mail catcher) | 1025, 8025 | Official GitHub releases | GitHub's SHA-256 digest |
| Meilisearch 1 (search engine) | 7700 | Official GitHub releases | GitHub's SHA-256 digest |

Each service is a module: MySQL and Caddy are on by default, MariaDB, PostgreSQL, Redis, Mailpit
and Meilisearch can be switched on in Settings. `STACKYARD_MYSQL_MANIFEST`,
`STACKYARD_POSTGRES_MANIFEST` and `STACKYARD_REDIS_MANIFEST` point those services at another
manifest, and
`STACKYARD_GITHUB_TOKEN` raises the GitHub API limit for Caddy, Mailpit and Meilisearch lookups.

Meilisearch runs in development mode with no master key, so apps connect to
`http://127.0.0.1:7700` without one and the address opens its search preview page. Its indexes
live in `data/meilisearch` and are carried over when Stackyard updates it to a newer 1.x release.

PostgreSQL keeps its databases in `data/postgres/db`, created the first time it starts with the C
locale and UTF-8, so sorting is the same on every system. It only accepts connections over TCP
(`127.0.0.1:5432`), never a socket file, so every client connects the same way everywhere. Redis
saves to `data/redis/dump.rdb` every few minutes while keys change, so queued jobs survive a
restart. Redis publishes no Windows version, so it is not offered there.

## Logs

The **Logs** screen shows every service's log (the web server, each PHP version, databases,
Mailpit and the background service) in one place, follows it as it grows, and searches it with
plain text or a regular expression. Errors and warnings stand out. Logs live in the `logs`
folder of Stackyard's data folder; one that grows past 5 MB starts over and the previous one is
kept as `.log.1`. Switch the screen off in Settings if you don't need it.

## Databases

The **Databases** screen works with whichever of MySQL and MariaDB is running: list and create
databases, page through a table's rows, run SQL (up to 1,000 rows are shown, and NULL is told
apart from an empty value), and back up or restore. Backups are gzipped SQL files in
`backups/<database>` inside Stackyard's data folder, made with the server's own dump tool, so they
load into any MySQL or MariaDB. A backup can replace the tables of its database or be restored as
a copy under another name. Dropping a database asks you to type its name; `mysql`, `sys` and the
other system databases can't be dropped from here. Switch the screen off in Settings if you use
another database tool.

## Updates

Stackyard checks for a newer version when it starts and twice a day while it runs (switch the
**Updates** module off in Settings to stop that). It never installs on its own: **Settings** shows
the new version and its notes, and **Install and restart** downloads it, stops the running
services, installs it and starts them again in the new version. Every update is signed with a
key only the maintainer holds, and the app refuses one whose signature doesn't match the public
key built into it.

To publish an update, set the `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
repository secrets (from `npx tauri signer generate`), put the public key in
`src-tauri/tauri.conf.json` under `plugins.updater.pubkey`, raise the version, push a `v*` tag, and
publish the draft release the Installers workflow makes. The app downloads updates from
`https://github.com/<repo>/releases/latest`, so that repository must be public; set the
`STACKYARD_RELEASES_REPO` variable (and a `STACKYARD_RELEASES_TOKEN` secret that can write to it)
to publish releases somewhere other than this repository.

## Disk space

**Settings → Disk space** shows how much Stackyard's own folder takes (runtimes, services, their
data, logs, downloads) and what can go. Safe items are picked for you: downloads and folders left
by interrupted installs, previous logs, versions Stackyard no longer offers. Others are yours to
pick: emptying the current logs, runtime versions that are not the default and that no site uses
or pins, and the kept data of a service you removed, which asks once more because it can't be
undone. Nothing is read until you ask, and project folders are never counted or touched.

## AI coding tools (MCP)

`stackyard mcp` is an [MCP](https://modelcontextprotocol.io) server, so Claude Code, Cursor,
Claude Desktop, Windsurf and other AI tools can see and manage your local stack. Switch on the
**MCP server** module in Settings (or `stackyard modules on mcp`), then add it to your AI tool;
the MCP server screen and `stackyard mcp setup` show the exact line, for example:

```sh
claude mcp add --scope user stackyard -- stackyard mcp
```

AI tools can always look: `list_sites`, `list_services`, `list_runtimes` and `read_logs`. Each
action is off until you allow it on the MCP server screen or with `stackyard mcp allow <tool>`:
`start_service`, `stop_service`, `create_site` and `set_site_runtime`. Deleting sites, databases
or files is never offered. Every call is recorded with the AI tool's name; see it on the MCP
server screen or with `stackyard mcp log`. An AI tool that is already connected sees allowed
tools appear without reconnecting.

## Scheduled tasks

The **Task scheduler** module (off by default; switch it on in Settings or with
`stackyard modules on scheduler`) runs commands on a schedule, such as Laravel's
`php artisan schedule:run` every minute or a nightly backup script. Each task runs with the
system shell in its project folder, with Stackyard's terminal commands first on `PATH`, so `php`,
`node` and `composer` are the versions that project uses.

Schedules use crontab's five fields, `minute hour day month weekday`: `*/5 * * * *` is every five
minutes, `30 2 * * *` every day at 02:30, `0 9 * * mon-fri` weekdays at 09:00. `@hourly`,
`@daily`, `@weekly`, `@monthly` and `@yearly` work too. Times are this computer's local time.

Tasks run while Stackyard runs: the app (also when only its tray icon is left) or
`stackyard start` in the background. A task still running when it is due again is not started
twice, and a minute missed while the computer slept is not caught up. Each task's output goes to
`logs/task-<id>.log`; the Scheduler screen shows it, and how the last run went.

## Develop

Requirements: Rust (stable), Node.js 22, and on Linux the WebKitGTK libraries:

```sh
sudo apt-get install libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev libayatana-appindicator3-dev librsvg2-dev
```

```sh
npm install
npm run tauri dev          # run the desktop app with hot reload
npm run dev                # interface only, in a browser, with sample data
cargo test --workspace     # Rust tests
cargo test -p stackyard-core -- --ignored live_   # real downloads, and services started for real
cargo run -p stackyard-cli -- sites list
```

## CLI

```sh
stackyard sites add myapp --path ~/Code/myapp   # PHP, Node or static, detected
stackyard sites add legacy --php 7.4 --path ~/Code/legacy
stackyard sites add shop --node 22 --port 5173
stackyard sites park ~/Code           # every project folder in ~/Code becomes a site
stackyard sites list
stackyard sites remove shop
stackyard system setup                # local domains and HTTPS, asks for your password once
stackyard start caddy                 # run the web server in the background (`stackyard start` = everything)
stackyard status                      # what is running, and where
stackyard logs                        # every log there is
stackyard logs caddy -n 100 -f        # the end of a service's log, then new lines as they come
stackyard stop                        # stop everything (`stackyard stop mysql` for one)
stackyard up                          # run the web server (and e.g. `stackyard up mysql`) until Ctrl+C
stackyard runtimes                    # versions, with installed and default marked
stackyard runtimes install php 8.4
stackyard runtimes default php 8.4
stackyard runtimes uninstall node 20
stackyard services                    # services, with installed and switched-off marked
stackyard services install caddy
stackyard services uninstall mailpit  # keeps its data
stackyard tld            # show the domain ending
stackyard tld .test      # change it
stackyard shims on       # php, node, composer… with each project's versions, in every terminal
stackyard shell          # or just in this terminal
stackyard use php 8.3    # pin a version for this project
stackyard which          # which version each command runs here, and why
stackyard db                          # databases on the running MySQL or MariaDB
stackyard db create shop
stackyard db query shop "SELECT * FROM orders LIMIT 5"
stackyard db backup shop              # backups/shop/shop-<date>-<time>.sql.gz
stackyard db restore shop             # the newest backup (or name a file; --into another database)
stackyard db drop shop --yes
psql -c "create database shop"        # PostgreSQL, once it runs (`stackyard start postgres`)
pg_dump shop > shop.sql
redis-cli ping                        # Redis (`stackyard start redis`)
stackyard cleanup                     # what takes space, and what can go
stackyard cleanup --recommended       # remove what is safe to remove
stackyard cleanup runtime-php-8.2     # or name items; service data also needs --yes
stackyard mcp setup      # how to add Stackyard to Claude Code, Cursor and others
stackyard mcp tools      # what AI tools may do
stackyard mcp allow start_service   # or `allow all`; `deny` turns it off again
stackyard mcp log        # what AI tools did
stackyard tasks add "Laravel scheduler" --run "php artisan schedule:run" --every "* * * * *"
stackyard tasks          # tasks, with their next and last runs
stackyard tasks run "Laravel scheduler"   # run one now, here in the terminal
stackyard tasks off "Laravel scheduler"   # `on` switches it back on; `remove` deletes it
stackyard tasks check "0 9 * * mon-fri"   # when a schedule would run
```

## License

Stackyard is MIT licensed (see `LICENSE`). It bundles the Space Grotesk, IBM Plex Sans and
JetBrains Mono fonts, which are under the SIL Open Font License 1.1. Runtimes and services that
Stackyard downloads keep their own licenses.
