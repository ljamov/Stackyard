# What's new in Stackyard

Each release has a section headed `## <version>`. The Installers workflow puts the section for the
version it builds into the release notes and into `latest.json`, so the app shows it next to the
Update button, and the app shows its own version's section after it updates.

## Unreleased

- A new look. A warm side bar with every screen by name, **New site**, your first few sites and
  Settings at the bottom; the screen beside it sits on a rounded sheet with softer boxes.
- **Home** greets you and has a box that runs any command, then your services, sites and runtimes.
- Services and runtimes show their own logos, and sites the logo of their framework (WordPress,
  WooCommerce, Laravel, Astro and more). WooCommerce shops are now detected as WooCommerce.
- The button at the top left, or **Ctrl+B**, narrows the side bar to just its icons and back.
- The lines beside the side bar and above the panel light up under the pointer; drag them to resize.
- The status bar has a sun or moon to switch between light and dark, and can be hidden
  (`statusBar = false` in settings.toml, or the palette).
- Clicking a screen in the side bar shows it in place of the one shown; Ctrl+click opens it as a
  tab beside it. The row of tabs only shows while more than one screen is open.
- Pick any accent color: `accent = "#3fa7d6"` in settings.toml (a picker in Settings comes next).

## 0.7.0

- A **High contrast** theme in Settings › Appearance: black with bright borders, a thick focus
  outline and underlined links. **Match my computer** picks it when Windows or macOS is set to more
  contrast.
- The focus outline on the Light theme uses the darker accent, so it's easier to see on white.
- Every color in the app is now a named theme token, which prepares for custom themes later.
- A new window layout: a bar of screen icons on the left, an Explorer beside it with your services
  (start and stop them right there) and sites, screens that open as tabs, Logs in a panel along the
  bottom, and a status bar with how many services are running. Drag the edges to resize the
  Explorer and the panel; Stackyard opens the way you left it.
- Shortcuts: **Ctrl+B** shows or hides the Explorer, **Ctrl+J** the Logs panel, **Ctrl+W** closes
  a tab, **Ctrl+Tab** and **Ctrl+PageDown/PageUp** move between tabs (**⌘** instead of Ctrl on a Mac).
- A denser, flatter look: your computer's own interface font, smaller buttons and fields, and
  squarer corners, so more fits on screen. The app is also smaller, with two fewer bundled fonts.
- A command palette: **Ctrl+Shift+P** (or **F1**) finds any command by a few of its letters: start
  or stop services, open a site in the browser, switch theme, close tabs and more. **Ctrl+P**, or the
  new search box at the top of the window, jumps to a site or screen.
- **Keyboard shortcuts** (in Settings, or **Ctrl+K Ctrl+S**): change the keys for any command by
  pressing the new ones, remove them or reset them, with a warning when two commands share a key.
  **Ctrl+,** opens Settings.
- Settings you can edit by hand: the theme, accent color and language are now saved in
  `settings.toml` (in a new `[interface]` section) and changed shortcuts in `keybindings.toml`.
  Edit either in a text editor and Stackyard picks the change up within a few seconds. **Settings ›
  Settings files** shows them in your file manager. Your current look and shortcuts are copied into
  the files the first time this version starts.
- A **Problems** panel beside Logs (**Ctrl+Shift+M**, or the count in the status bar) lists anything
  Stackyard can't use in those files or in language files, such as a misspelled value or a file
  that isn't valid TOML, and says what to fix. Stackyard keeps working with the defaults meanwhile.
- **VS Code themes**: Settings › Appearance › **Import a VS Code theme…** takes a theme's `.json`
  file or a whole `.vsix` extension, and every color theme in it appears under **Added themes**.
  Stackyard uses the theme's colors for its own window, so your editor and Stackyard can match.
- A theme picker: **Ctrl+K Ctrl+T** lists every theme and shows each one on the window as you move
  through them; Escape goes back.
- **Icons**: pick the Stackyard line icons or Codicons, the icons VS Code uses.
- **Density**: Compact, Default or Comfortable, for more on screen or more room.
- **Zoom** from 80% to 150%: **Ctrl+=**, **Ctrl+-** and **Ctrl+0**.
- The side bar can sit on the right and the Logs panel on the right, beside the screen.
- All of these are kept in `settings.toml` with the rest, and can be changed there too.
- Keyboard only: **F6** and **Shift+F6** move between the icon bar, Explorer, screen, panel and
  status bar; the arrow keys, Home and End move along the icon bar and the tabs, and **Delete**
  closes a tab. The command palette puts the focus back where it was when it closes.
- Screen readers hear each service's state in the Explorer and are told when a service starts,
  stops or crashes. Labels that were only in English are translated.
- Fields, buttons, borders and the stopped-service dot have more contrast, and on the Light theme
  accent buttons use the darker accent shade, so every built-in theme meets WCAG AA with every
  accent color. The build now checks this.

## 0.6.0

- Site tools: **Manage** on a site's row on the Sites screen opens its settings, backups, public
  link, and servers and Git. Each is a module that can be switched off in Settings › Modules.
- Site settings: pick a site's own domain ending, the folder served (e.g. `public`), HTTPS on or
  off, environment variables, PHP settings for that site only, its database, and a start command
  Stackyard runs while the web server runs (e.g. `npm run dev` for Vite, or your own server
  command for a Node, Python, Go or Java site, with its port in `PORT`).
- Site backups: one click backs up a site's files (without `node_modules`), its database and its
  settings into one zip. **Restore** puts them back, after backing the site up as it is now.
- Move a site to another computer: **Export to a file…** saves the same zip anywhere, and
  **Import site…** on the Sites screen brings it in with its settings and database.
- Public links (off until you switch them on): share a local site at a temporary
  `https://….trycloudflare.com` address through Cloudflare's free quick tunnels. Stackyard
  downloads Cloudflare's `cloudflared` the first time. Meant for showing work, not hosting.
- On the command line: `stackyard sites backup`, `backups`, `restore`, `export` and `import`.
- WordPress sites made from the starter remember their database for backups.
- Updates are found sooner: Stackyard now checks every half hour and whenever you bring its window
  forward (at most once every ten minutes), instead of only at start and every four hours, so a
  new version shows up without quitting and reopening the app. **Check for updates** in Settings
  still checks right away.
- Smaller, faster builds: the app and the `stackyard` command are built with link-time
  optimization and without debug symbols, which keeps the installers under the 15 MB budget.
- The installers can be signed: Windows with Azure Artifact Signing and macOS with an Apple
  Developer ID (and notarized), as soon as the signing details are added to the repository. Until
  then they build unsigned as before. See `docs/signing.md`.
- Every change to master now builds the installers for Windows, macOS and Linux, installs them,
  starts the app and checks it against the performance budgets (installer size, start time, and
  memory with only the tray icon left), failing the build when one is exceeded.
- Light theme: **Settings › Appearance** has Dark, Light and **Match my computer**, which follows
  your computer's light or dark setting. Pick an accent color too: amber, blue, green, violet,
  rose or teal.
- Languages: Stackyard can now be used in Deutsch, Español, Français, Italiano, Македонски
  (Macedonian) and Português (Brasil) besides English (**Settings › Appearance › Language**).
  The menus, main screens, Settings and update notices are translated so far; the rest stays in
  English for now. Add your own language, or change a word, by putting one file in the
  `languages` folder: see `docs/appearance-and-languages.md`.
- The website has a **What's new** page with every release's notes, made from this changelog.

## 0.5.0

- Python, Go and Java: install Python 3.11 to 3.15, Go 1.25 to 1.27 and Java 11, 17, 21 and 25
  (Eclipse Temurin) on the Runtimes screen, side by side like PHP and Node.js. Each is a module,
  off until you switch it on in Settings › Modules. Downloads are checked against their published
  SHA-256.
- Django, FastAPI, Flask, Go, Spring Boot, Quarkus and Micronaut projects are detected when you add
  them as sites. Stackyard starts the app's server with the site's version on a port of its own and
  serves it at `https://<name>.stack`; a Python project's own `.venv` is used when it has one. Pick
  the version on the Sites screen like any other.
- Terminal: `python`, `python3`, `pip`, `pip3`, `go`, `gofmt`, `java`, `javac` and `jar` follow
  each project's version (`.python-version`, `.go-version`, `.java-version`, `.sdkmanrc` and
  `.tool-versions` are read too). Until Stackyard has its own version installed, they run the one
  already on your computer.
- New AI tools screen: install Claude Code, Codex CLI, Gemini CLI, GitHub Copilot CLI, Qwen Code
  and OpenCode with one click. Stackyard installs them with npm on its own Node.js (and installs
  Node.js 24 first when needed), so nothing else is required. The screen shows when an update is
  out, and **Open** starts a tool in a new terminal window in the site folder you pick. With the
  terminal commands on, `claude`, `codex` and the others also work in any terminal. Also on the
  command line as `stackyard ai`. The AI coding tools module can be switched off in Settings ›
  Modules.
- Three new services, each off until you switch it on in Settings › Modules: MongoDB 9.0
  (downloaded from mongodb.com), SeaweedFS for S3-compatible file storage (in place of MinIO,
  which no longer publishes downloads), and NATS, a small message queue that keeps its messages
  on disk. Each has its log, moves to a free port when its usual one is taken, and starts and stops
  from the tray and with `stackyard start`.
- Service cards show how apps connect, such as `mongodb://127.0.0.1:27017`, or the S3 address and
  keys for SeaweedFS.
- Databases screen: PostgreSQL databases too. Switch between MySQL and PostgreSQL at the top to
  list, create and drop databases, browse tables, run SQL, and back up and restore. In the
  terminal: `stackyard db --postgres`.
- Sites: each site has version pickers for PHP and Node.js (Node.js for build tools such as Vite),
  parked sites included. Picking a version that is not installed downloads it first, then the site
  switches to it.
- Projects that already name a version for other tools need nothing new: `.nvmrc`,
  `.node-version`, `.php-version` and `.tool-versions` are read like a `.stackyard` file, by the web
  server and the terminal alike.
- Fixed the web server ignoring a project's `.stackyard` file for sites added by hand: the browser
  and the terminal now always use the same version.
- MCP server: AI tools such as Claude Code and Cursor can now restart services, list your MySQL
  and MariaDB databases and their tables, run read-only queries (Query a database) and run SQL
  that changes data (Change a database). Querying and changing stay off until you allow them.
  Change a database never drops a whole database and never reads or writes files on your
  computer. You choose what AI tools may do in Settings › AI tools as well as on the MCP server
  screen.

## 0.4.0

- New Videos screen: make web-ready MP4 (H.264), WebM (VP9) and GIF copies of videos and GIFs,
  with a quality choice, a largest width (or a custom one), and an option to remove the sound.
  It uses FFmpeg, which is not bundled to keep Stackyard small: one click downloads it (about
  150 MB), or Stackyard uses the one already installed. The Video optimizer module can be
  switched off in Settings › Modules.
- Images: Choose images… opens in your Stackyard sites folder instead of Documents, and the site
  list shows the site you picked. Largest width has a Custom width option: type any width, and
  either keep the aspect ratio (the height follows) or set an exact height (edges are trimmed
  evenly, nothing is stretched).
- New site: the Name field no longer jumps the cursor to the end or swallows spaces while you
  type. Type any name, such as "My Blog"; the field shows the address it makes (my-blog.stack).
- Logs: Clear this log empties just the log you are looking at (say Caddy or MySQL), leaving the
  others alone. Clear all logs still empties every one.
- Servers: choose what to upload or download, like in an FTP program: new and changed files,
  only newer files, different size or date, different size only, only missing files, or
  everything. Every field in the server form has help when you hover over it or click in it, with
  clearer examples, including Folder on the server.

## 0.3.1

- Fixed MySQL failing to start with "ibdata1 must be writable": opening Stackyard again now brings
  back the window that is already open instead of starting a second copy, and a MySQL left over
  from an earlier Stackyard is stopped before a new one starts.
- Sites and other screens no longer sit in a short box with their own scroll bar: the whole
  window scrolls.
- Servers & Git, on each site in Sites: connect to a server over SFTP, FTPS or FTP, preview and
  upload or download changes, and browse the server. Link the site to a Git repository to pull,
  commit, push and fetch. Passwords stay in the system's password store.
- Images: the output folder field matches the rest of the app, Browse opens in your project's
  folder, and a message says when optimizing is done and how much space it saved.
- Stackyard tells you when an update is out: a notification while it runs (also from the tray),
  and a notice at the top of the window with an Update button.
- Turned off when an update came out? You see the notice the next time you start Stackyard.
- See what an update brings before you install it, and what changed after it installs.
- Checks for updates every four hours instead of twice a day.

## 0.3.0

- Start a new site as WordPress, Laravel or Astro.
- Image optimizer: make JPG and PNG files smaller, or convert them to WebP or AVIF.
- PHP in the terminal uses the same settings as your sites.

## 0.2.2

- Services stop when Stackyard quits, and leftovers are cleaned up before a start.
- Moving the Stackyard folder keeps every file until the whole move has worked.
- phpMyAdmin no longer shows in Sites.

## 0.2.1

- Moving the Stackyard folder works when files are in use.
- When another program uses a port, services move to the next free one.
- Empty site folders show a page that says where to put your files.

## 0.2.0

- First-run setup: choose where Stackyard keeps its files and sites.
- phpMyAdmin as a module.
- A new window frame, Clear logs, and Install all.

## 0.1.0

- First release.
