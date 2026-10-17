# Contributing to Stackyard

Thanks for helping out. Stackyard is a Tauri 2 desktop app: a Rust core that does the work, a thin
Tauri layer, a React + TypeScript interface, and a `stackyard` CLI that shares the same core.

## Project layout

| Path | What it is |
| --- | --- |
| `crates/stackyard-core` | Core logic shared by the app, the CLI and the MCP server: settings, sites, domain rules, catalog |
| `crates/stackyard-cli` | The `stackyard` command-line tool |
| `src-tauri` | Tauri 2 desktop app: thin commands over the core |
| `src` | React + TypeScript interface |

Settings and sites are stored as TOML in the OS data folder (for example
`~/Library/Application Support/Stackyard` on macOS). Set `STACKYARD_HOME` to use another folder.

## Set up and run

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

## Screenshots

The pictures in `docs/images` (README and website) come from the browser preview with its sample data.
After a change to how a screen looks, retake them:

```sh
npm i --no-save playwright
npx playwright install chromium
node scripts/screenshots.mjs              # every screen
node scripts/screenshots.mjs logs images  # only these
```

With Python and Pillow installed, the script also cuts each PNG to 256 colours, which keeps them around 50 KB.

## Translations

Interface text is English inside `t("…")`, and `src/locales/<code>.json` maps it to each language.
When you add or change text, wrap it in `t()`; `node scripts/i18n.mjs` lists what each language file
is missing. Adding a language is described in [docs/appearance-and-languages.md](docs/appearance-and-languages.md).

## Before you open a pull request

Run the same checks CI runs:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm run build              # type-checks the interface and builds it
```

## Ground rules

- **Write it yourself.** Stackyard is written from scratch. Please don't copy code, text or designs
  from other local development tools, whatever their license.
- **Keep it light.** Every runtime, service and screen is a module that can be switched off. Off
  means nothing is downloaded, nothing runs and the screen is hidden. New features should follow
  the same rule and avoid heavy dependencies.
- **Check every download.** Anything Stackyard downloads is verified against a published checksum
  before it is unpacked.
- **Stay on 127.0.0.1.** Services listen only on the local machine.

By contributing you agree that your work is released under the [MIT License](LICENSE).
