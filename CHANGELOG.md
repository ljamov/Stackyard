# What's new in Stackyard

Each release has a section headed `## <version>`. The Installers workflow puts the section for the
version it builds into the release notes and into `latest.json`, so the app shows it next to the
Update button, and the app shows its own version's section after it updates.

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
