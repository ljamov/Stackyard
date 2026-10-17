# What's new in Stackyard

Each release has a section headed `## <version>`. The Installers workflow puts the section for the
version it builds into the release notes and into `latest.json`, so the app shows it next to the
Update button, and the app shows its own version's section after it updates.

## Unreleased

- Sites: each site has version pickers for PHP and Node.js (Node.js for build tools such as Vite),
  parked sites included. Picking a version that is not installed downloads it first, then the site
  switches to it.
- Projects that already name a version for other tools need nothing new: `.nvmrc`,
  `.node-version`, `.php-version` and `.tool-versions` are read like a `.stackyard` file, by the web
  server and the terminal alike.
- Fixed the web server ignoring a project's `.stackyard` file for sites added by hand: the browser
  and the terminal now always use the same version.

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
