# Stackyard

A local development environment for Windows, macOS and Linux. Manage PHP, Node.js and more,
databases, web servers and local HTTPS sites such as `https://myapp.stack` from one desktop app,
with no containers.

> Status: early prototype. Runtimes, services, local sites with HTTPS and terminal commands work;
> a background service, tray icon and installers come next.

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
| `crates/stackyard-core` | Core logic shared by the app, the CLI and the future MCP server: settings, sites, domain rules, catalog |
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
published SHA-256. `mysql` connects to whichever of MySQL and MariaDB is running.

## Services

Services install into `services/<id>/<line>`, keep their data in `data/<id>` (kept when a service
is removed, so reinstalling brings databases back) and their config in `config/<id>`. Everything
listens on `127.0.0.1` only. Databases start with user `root` and no password.

| Service | Ports | Source | Checked against |
| --- | --- | --- | --- |
| Caddy 2 (web server) | 80, 443 (8080, 8443 when taken) | Official GitHub releases | Caddy's SHA-512 list |
| MySQL 8.4 | 3306 | Oracle's official downloads | SHA-256 from `.github/workflows/mysql-manifest.yml`, which first checks Oracle's GnuPG signature |
| MariaDB 11.8 (Windows, Linux) | 3306 | MariaDB Foundation downloads | The Foundation's SHA-256 |
| Mailpit (mail catcher) | 1025, 8025 | Official GitHub releases | GitHub's SHA-256 digest |

Each service is a module: MySQL and Caddy are on by default, MariaDB and Mailpit can be switched on
in Settings. `STACKYARD_MYSQL_MANIFEST` points MySQL at another manifest, and
`STACKYARD_GITHUB_TOKEN` raises the GitHub API limit for Caddy and Mailpit lookups.

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
```

## License

Stackyard is MIT licensed (see `LICENSE`). It bundles the Space Grotesk, IBM Plex Sans and
JetBrains Mono fonts, which are under the SIL Open Font License 1.1. Runtimes and services that
Stackyard downloads keep their own licenses.
