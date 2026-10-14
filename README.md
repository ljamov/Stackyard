# Stackyard

A local development environment for Windows, macOS and Linux. Manage PHP, Node.js and more,
databases, web servers and local HTTPS sites such as `https://myapp.stack` from one desktop app,
with no containers.

> Status: early prototype. The app shell, screens, site list and settings work; installing
> runtimes and running services arrive in the next milestones.

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
cargo run -p stackyard-cli -- sites list
```

## CLI

```sh
stackyard sites add myapp --php 8.3 --path ~/Code/myapp
stackyard sites add shop --node 22 --port 5173
stackyard sites list
stackyard sites remove shop
stackyard runtimes
stackyard services
stackyard tld            # show the domain ending
stackyard tld .test      # change it
```

## License

Stackyard is MIT licensed (see `LICENSE`). It bundles the Space Grotesk, IBM Plex Sans and
JetBrains Mono fonts, which are under the SIL Open Font License 1.1. Runtimes and services that
Stackyard downloads keep their own licenses.
