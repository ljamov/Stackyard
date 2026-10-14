# Stackyard

A local development environment for Windows, macOS and Linux. Manage PHP, Node.js and more,
databases, web servers and local HTTPS sites such as `https://myapp.stack` from one desktop app,
with no containers.

> Status: early prototype. The app shell, site list, settings, runtime installs and services
> (install, start, stop, logs) work; local sites with HTTPS arrive in the next milestone.

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

## Services

Services install into `services/<id>/<line>`, keep their data in `data/<id>` (kept when a service
is removed, so reinstalling brings databases back) and their config in `config/<id>`. Everything
listens on `127.0.0.1` only. Databases start with user `root` and no password.

| Service | Ports | Source | Checked against |
| --- | --- | --- | --- |
| Caddy 2 (web server) | 8080 | Official GitHub releases | Caddy's SHA-512 list |
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
stackyard sites add myapp --php 8.3 --path ~/Code/myapp
stackyard sites add shop --node 22 --port 5173
stackyard sites list
stackyard sites remove shop
stackyard runtimes                    # versions, with installed and default marked
stackyard runtimes install php 8.4
stackyard runtimes default php 8.4
stackyard runtimes uninstall node 20
stackyard services                    # services, with installed and switched-off marked
stackyard services install caddy
stackyard services uninstall mailpit  # keeps its data
stackyard tld            # show the domain ending
stackyard tld .test      # change it
```

## License

Stackyard is MIT licensed (see `LICENSE`). It bundles the Space Grotesk, IBM Plex Sans and
JetBrains Mono fonts, which are under the SIL Open Font License 1.1. Runtimes and services that
Stackyard downloads keep their own licenses.
