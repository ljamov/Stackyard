# What's new in Stackyard

Each release has a section headed `## <version>`. The Installers workflow puts the section for the
version it builds into the release notes and into `latest.json`, so the app shows it next to the
Update button, and the app shows its own version's section after it updates.

## Unreleased

- **Projects are apps or sites.** Projects lists everything; Apps and Sites, under it in the side
  bar, list each kind. New project asks first whether it is an app or a site, then what it starts
  from, with an empty app or site first. Sites start as WordPress, WooCommerce, Laravel, Astro,
  Next.js, React, Vue, Svelte or static HTML, or as a clone of a live WordPress site or a
  Stackyard site file. Apps start as a WordPress plugin or theme, a PHP package, a Node.js server
  or Python scripts. What was under the old Projects list moves to Apps, or to Sites when its
  folder has a start page or a framework.

- **Apps need no address.** An app gets no `.stack` name unless you ask for one: one with its own
  server opens at `http://localhost:<port>`, and one served by PHP or as plain files at its
  `.localhost` name. Manage › Settings turns the address on or off; clearing an app's domain
  ending turns it off instead of failing.

- **Groups stay in their list.** A group made under Apps shows only there and under Projects, not
  under Sites too.

- **Remove can delete the files.** Remove asks whether to keep the folder or delete it with
  everything in it. It refuses a folder that holds a parked folder or another project.

- **Version numbers on the lists.** Lists show which PHP or Node.js version each one uses; you
  change it in its own tab (Manage).

- **Apps and sites everywhere.** The side bar lists your apps and your sites under their own
  headings, the status bar counts both, and both update as soon as one is made or removed.

- **WordPress themes and plugins without a whole WordPress.** Making a theme or plugin project
  now only makes its folder, with a starting theme or plugin when it is empty; nothing is
  downloaded and no database is needed. Its Manage tab has a Theme or Plugin section that sets the
  version (the header's Version line, and Stable tag in readme.txt) and builds the .zip you upload
  in WordPress, with one folder inside named after it. Git, node_modules, editor settings, other
  zips and what `.distignore` lists are left out; zips go in the `builds` folder in Stackyard's
  data folder. Themes and plugins made before are served from their own folder again, and the
  WordPress made for them can be removed with Remove old WordPress.

## 0.19.0

- **Projects, Apps and Sites.** Everything you make is in one of three lists, each with its own
  place in the side bar. New asks first what you are making, then what it starts from. Sites can
  start as WordPress, WordPress with WooCommerce (switched on once WordPress's install page is
  done), Laravel, Astro, React, Vue, Svelte, Next.js or static HTML; apps as a small Node.js
  server; projects as a WordPress theme or plugin, a PHP package or Python scripts. Any of them
  can also start from an empty or existing folder.

- **Change what something is.** Manage › Settings has a Type picker that moves it between
  Projects, Apps and Sites. Sites saved before this stay under Sites, themes and plugins go under
  Projects, and Node.js apps under Apps.

- **Folders you make yourself count too.** A folder you create in a parked folder shows up under
  Projects. Its settings and type can now be changed like one made in the app; the first change
  saves it. Before, a parked folder's settings could not be changed at all.

- **Windows only.** Checks and installers are built for Windows alone from now on.

- **Sites follow their folder to another drive.** A site whose folder was moved into a parked
  folder (say, from C:\stackyard\www to D:\Sites) now uses the new folder by itself and keeps its
  settings, database and connections, instead of showing the old path. Before, the old entry won
  and the moved copy was ignored.

- **Move a site, or change its folder.** A site's Settings has Move to another folder (copies
  everything there, another drive works too, and removes the old folder only once the copy is
  complete) and Change folder (points the site at a folder you already copied).

- **New sites go into the parked folder.** Parking a folder when new sites went into one that
  isn't parked now makes new sites go there, and each parked folder has Use for new sites.
  Before, New project kept using the first folder from setup.

## 0.18.0

- **Put a site made here on a host.** A WordPress site that isn't a copy of a live one gets a
  Live site tab under Servers & Git: pick one of its servers (SFTP, FTP or the Stackyard Connect
  plugin) and the address, type the host's name to confirm, and Stackyard uploads every file and
  the whole database into the WordPress already on the host, with every address changed to it.
  The host's database is backed up first and put back if a step fails. From then on the site
  pushes and pulls like a cloned one. An Everything push now also keeps the plugin's connection
  code on the live site.

- **Clone a site without its FTP login.** On a site with the Stackyard Connect plugin, open
  Tools › Stackyard and make a connection code. In Stackyard, pick Clone › A connection code (no
  FTP) and paste it. The live site exports itself in short steps and Stackyard downloads the
  export, going on where it stopped if the connection drops, then deletes it from the live site.
  The code stops working after a day without use, and can be stopped at any time on the live site.
  The copy keeps the code as its Live connection, so pull works through the plugin, and push too
  when the code was made to allow changes. Servers can also use the plugin instead of FTP: pick
  Stackyard Connect plugin under Manage › Servers and paste a code.

- **Stackyard Connect, a WordPress plugin, and drag and drop.** Install the plugin on a live site
  and click Tools › Stackyard › Export this site: you get one file with the whole site, files and
  database, made in short steps so it works on any host. Drop that file on the Sites screen and
  Clone opens with it, ready to make the local copy. Any site backup can be dropped the same way.

- **New project, not just new site.** The Sites screen asks what you are making: a website, a
  WordPress theme, a WordPress plugin, a PHP package or a Node.js app. A theme or plugin keeps its
  own folder (an empty one gets a starting theme or plugin) and Stackyard makes a WordPress to try
  it in, installed and with the theme or plugin switched on; the folder is linked into it, so a
  change shows at once. The editor, the terminal, Git and Servers open the theme's or plugin's own
  folder. A project's type can be changed later under its Settings.

- **Much faster push, pull and clone for WordPress.** Stackyard's helper on the live server now lists
  every file in a few requests, where Stackyard used to ask the server for each folder in turn (over
  FTP, a new connection for each one). Small files then come many to a request instead of one by
  one. If the helper can't do it, Stackyard falls back to the old way by itself.

- **Publisher name on Windows.** The installer, the program file and Apps & features now name the
  publisher and the project page, which also helps antivirus programs tell Stackyard apart from
  unknown software.

## 0.17.0

- **Work keeps going in the background.** Every open tab stays alive while another one is shown, and
  so does every section and inner tab of a site. A sync check, pull, push, backup, image or video
  job keeps running when you switch tabs or sections, and its progress and result are still there
  when you come back, at the same scroll position. While something runs in the shown tab, picking a
  screen in the side bar opens it in a new tab beside it instead of in its place.
- **Sites open where you left them.** A site's tab comes back on the section and inner tab you last
  picked (for example Servers & Git › Live site), also after Stackyard restarts.
- **Fixed: the window disappeared right after opening.** An empty site group (one made with
  **New group** and left empty) broke the Sites screen, and since the window is see-through, it
  looked like Stackyard closed while it kept running in the tray. Empty groups now show as
  "0 sites". If a screen ever fails to draw again, it shows the error with **Try again**,
  **Reload the window** and **Open log file** instead of an empty window.
- **A log file for when something goes wrong.** Stackyard writes its start-up steps, errors, crashes
  and errors from the window to `stackyard.log`. On Windows it is in the folder Stackyard is
  installed in (next to the uninstaller); elsewhere, or when that folder can't be written, in the
  `logs` folder of the data home. It starts over after 1 MB and keeps the two before it. Open it
  from **Settings › Log file** or the tray menu (**Open log file**).
- **Fixed: Continue on a clone that stopped in its last steps** said "a site named … already exists".
  It now carries on with that site, and downloads the files again if its folder lost them.
- A saved clone the app can't read no longer stops the others from showing, and a clone that the app
  closed under twice in a row waits for **Try again** instead of starting by itself again.
- **Pause and Resume for clones from a live server.** Each running clone has **Pause** on its card
  and in the status bar's clones list. Pausing stops its download at once, so the other clones get
  the whole connection; what it downloaded is kept (a file cut off halfway goes on from where it
  stopped), and **Resume** continues from there, also after Stackyard restarts.
- **Much faster downloads over SFTP.** Small files now come 16 at a time over the same connection
  instead of one after another, and server folders are listed 16 at a time. On a site with
  thousands of small files (a usual WordPress site) the files come in about 15 times faster when
  the server is far away. Push, pull and Remote downloads get faster the same way.

## 0.16.0

- **Your own order for sites, and groups.** On the Sites screen, drag a site by its handle to move it,
  or put sites in a group (one per client, say) with **New group**, and drag groups around too. A
  group folds away with a click on its name and is renamed with the pencil. The order is kept and
  every list of sites follows it: the side bar, the command palette, Home, the tray and
  `stackyard sites`. The handle also moves with the arrow keys.
- **See how big a live site's files are before cloning it.** Clone from a live server now shows,
  next to the database, the size of WordPress core, themes, plugins, media (uploads), other files
  and all files together, with file counts, plus how much in caches and backups is left out. The
  media choice shows how much **Download it all** downloads. Backup files are counted the same way.
- **See how much room each site takes.** A site's own tab shows its size: the database, WordPress
  core, themes, plugins, media, other files, caches and backups, and the total. It is measured in
  the background when the tab opens (again after an hour, or with **Measure again**), and the Sites
  list shows each site's last measured total.
- **Local domains stay set up.** On Windows, new sites (and clones and stagings) used to bring back
  "Finish setting up local domains" with a password prompt each time. Setup now lets Stackyard keep
  its own lines in the hosts file up to date, so it is asked for once. Click **Set up** one more
  time after this update, and new sites get their address by themselves from then on.
- **Clones go on after an update.** A clone from a live server that was running when Stackyard
  updated or quit starts again by itself and continues where it stopped. Stopped or failed clones
  are shown again too, with their Try again button. The update notice says so while a clone runs.
- **Clone from a live server: tidier server form.** Connect with, Server address and Port share
  the first row (20/60/20), User name and Password the second, Folder on the server and Private
  key the third.
- **The live login is saved once.** The server typed into Clone from a live server is kept with
  the new site under Manage › Servers & Git as Live (password in the system's password store),
  ready for live pull and push and editable there. Cloning the same site again from the same
  server, user and folder updates that connection instead of adding a second one.
- **Clone progress stays put.** The downloaded amount and percent (e.g. 50.2 MB / 562.4 MB · 8%)
  now sit at a fixed spot above the bar, and the file being copied goes on its own line below it,
  so a long path no longer pushes the numbers around.
- **More in the tray menu.** Start and stop each service on its own (a tick shows which run), open a
  site in the browser, make a new site or app, open the sites folder, open a terminal with
  Stackyard Shell, open phpMyAdmin, or go straight to a screen. The menu and the icon's tooltip
  follow the services as they start, stop or crash, without checking on a timer, and Stop all no
  longer holds up the menu while services shut down.
- **AI tools can do more through the MCP server.** New tools that only look: `wordpress_overview`
  (versions, plugins, themes, debug settings, snapshots and live backups of a WordPress site),
  `read_wordpress_log` (the site's `debug.log`), `list_backups` and `list_tasks` (scheduled tasks,
  so their logs can be read). New tools you allow first: `set_wordpress_debug` (only the local
  `wp-config.php`), `install_runtime`, `backup_database` and `backup_site`. `list_databases` and
  `backup_database` work with PostgreSQL too. Pushing to and pulling from a live site stays yours.
- **Fixed: the WordPress MCP tools didn't see WordPress sites in a parked folder** (such as the
  sites folder), so `run_wp_cli` and the staging tools said there was no such site.
- **Clear connection errors.** When connecting to a server fails, Stackyard now says what failed,
  whose side the problem is on (internet or firewall, the server, the login or settings, or
  Stackyard), the server's own reason when it gave one, and what to try. "could not open a
  session: Disconnected" now reads, for example, that the server accepted the login and then closed
  the connection, and to check that SSH/SFTP is on for the user and that no other program is
  connected. **Check connection** under a failed clone or Test goes through each step (find the
  server, reach the port, handshake, log in, open a session, open the files, run commands) and shows
  where it stops.

## 0.15.0

- **Clone several live sites at once.** Clone from a live server no longer waits for the clone
  already running: start as many as you like. **Minimize** puts a clone away while it keeps going,
  and the status bar shows every clone with its stage and how many MB are downloaded. A clone that
  stops or fails can be tried again from where it stopped.

## 0.14.0

- **Push and pull moved to Servers & Git.** A copy of a live WordPress site has a **Live site** tab
  in Manage › Servers & Git with Pull from live, Push to live and the live backups. The Pull and Push
  buttons at the top of the WordPress view stay, and open that tab.
- **WordPress › Environments is now Staging**: make a staging copy of the local site to try something
  out, then **Merge into** the site (a button beside each staging) or **Delete** it. Stagings are made
  from the local site or another staging.
- **Debug settings stay on this computer.** The Debug tab of a copy of a live site shows beside each
  setting whether it is on or off on the live site, and **Set back to how live has it** copies the live
  ones. Switching them changes only the copy's `wp-config.php`, which a push never sends. The live
  `wp-config.php` kept beside the copy is read again on each compare, push and pull.
- **Fixed: Pull from live and Push to live stopped with "This Stackyard helper has expired and
  removed itself."** The helper Stackyard puts on the live server counted its 30 minutes from this
  computer's clock, so a clock running behind the server's made it expire before it started. It now
  counts from when it lands on the server, by the server's own clock.
- **Fixed: Meilisearch couldn't be installed while a new release was still uploading its builds.**
  Stackyard now takes the newest Meilisearch release that has a download for your system.
- **Remote servers: Upload changes and Download changes show how far the compare is.** Instead of
  only "… files so far", you now see which side is being listed, files so far of about how many,
  a filled progress bar, the time left at the pace so far, and the folder being looked at. The
  total comes from the last compare of that server, or from the site folder's own file count the
  first time.

## 0.13.0

- **Fixed: downloading a WordPress site's files from its server broke the site** with "Error
  establishing a database connection". Download copied the live `wp-config.php` over the copy's
  own, so the copy tried the live database login. Upload and Download (Remote servers) now leave
  each side's own files alone on WordPress sites: `wp-config.php`, the live copy Stackyard keeps
  as `wp-config.live.php`, `.user.ini`, the object cache (`object-cache.php`), maintenance mode
  and Stackyard's own files. The list of files says which ones were left alone, and a download
  points to WordPress › Live › Pull for posts, pages and settings, which live in the database.
- **Pull from live and Push to live are on the WordPress view's top row** of a copy of a live site,
  next to WP Admin, and open the live panel ready to go (it now starts on Pull). When posts, pages
  or comments changed only on the live site, a pull picks Everything, so the live database comes
  too; when they changed on both sides, it says so and leaves the choice to you.
- **Pulling from the live site no longer brings back the live object cache**, which a copy keeps
  switched off, and a copy of a site that uses Redis gets `WP_REDIS_DISABLED` so a returning
  cache file stays inactive.

## 0.12.0

- **Site pictures that show something.** A site's picture on Sites is now the site's home page, or,
  until there is one, its logo: the framework's (WordPress, Laravel, Astro…) or else the runtime's
  (PHP, Node.js, Python…), not a plain white box. A page that comes out blank is tried again with
  more time to load and otherwise dropped, and blank pictures kept by earlier versions are thrown
  away. A camera button over each picture takes a new one and, when it can't, says why (the web
  server isn't running, the site's database is stopped, or there is no Edge or Chrome to take it).
- **Fixed: a removed site stayed in the side bar** under Your sites (and its tab stayed open).
  Adding, removing or changing a site now updates every list of sites in the window at once.
- **Site buttons are square icons** (Terminal, Open folder, Open in your code editor, Manage, Remove),
  each named in a tooltip when you point at it.
- **A site's tab has the same buttons** next to Open in browser, and shows the site's picture with a
  button to take a new one.
- **A site's sections are real tabs**: WordPress, Settings, Backups, Public link and Servers & Git sit
  in a row of tabs with icons above the section, so they no longer look like the buttons inside it.
- **Create site shows what is happening**: a progress bar with each step (downloading WordPress with
  how far it has got, unpacking, making the database), then a note that the site is ready with
  Open and Manage.

## 0.11.0

- **Fixed: after making a new site, every site in Sites showed its `.localhost` address**, though
  clicking still opened the `.stack` one. On Windows a new site has to be added to the hosts file
  first, and until then the app showed `.localhost` for all sites. Now each site is checked on its
  own: sites whose `.stack` name already works keep showing it, and only the new site shows
  `.localhost` until you run setup.
- **Sites rows have Open folder and Open in VS Code.** Open folder shows the site's folder in the
  file manager; Open in … opens it in your code editor.
- **Settings has a Code editor choice**: VS Code, Cursor, Windsurf, PhpStorm, WebStorm, Sublime Text,
  Zed and, on Windows, Notepad++. Stackyard says which are installed and remembers the one you pick
  (`editor` in settings.toml). VS Code stays the default; without it, the first installed one is used.
- **Fixed: making a WordPress staging site stopped at the database** with "Unknown table
  'column_statistics' in information_schema", and **changing the table prefix** stopped with
  "Could not set terminology mode for outputting events" before any table was renamed. With both
  MySQL and MariaDB installed on Windows, Stackyard used MySQL's tools on a MariaDB server. It now
  asks the server on the port which one it is and uses that server's own tools. MySQL's dump tool
  also skips column statistics, which MariaDB and older servers don't have, and copies from a live
  server do the same when the server's mysqldump is MySQL's.
- Database errors show what actually went wrong, not a warning printed before it.
- **Fixed: Remove on Sites did nothing** for sites in a parked folder, which is where new sites go
  (C:\stackyard\www on Windows). The site was taken off the list and then came straight back as a
  parked site. Removed sites now stay off the list; their folder and files are kept. Parked sites
  have a Remove button too, and an error from Remove or Open now scrolls into view.
- **Clone from a live server finds the site's address by itself.** When the server can't run
  Stackyard's PHP helper from the command line (FTP, file-transfer-only SFTP, or SSH without a
  usable `php`), Stackyard reads the address from the live database or tries the domains in the
  WordPress folder's path and the server's name. Only when none answers does it ask, as a note
  under the **Live site's address** box instead of an error.
- SSH servers are no longer taken for shell-less ones when they print a greeting before each
  command, or when `php` is missing from a non-interactive login's PATH or is the CGI build
  (Stackyard also looks in /usr/local/bin, /usr/bin and the cPanel, CloudLinux and Plesk PHP folders).
- **Cloning onto a name that is taken** no longer stops with "a site named … already exists".
  Stackyard offers to **replace** that site (it backs up its files and database first, restorable
  under Manage › Backups) or to use the next free name. `stackyard wp clone --replace` does the same.

## 0.10.0

- The **Problems, Logs and Terminal** panel now sits under the open screen only, so the side bar
  runs the whole height of the window. The new button beside the panel's ✕ stretches it across
  the whole window again; Stackyard remembers which you picked.
- The panel's tabs have more room between their underline and what the panel shows.
- **Tabs can be dragged** along the row to put them in another order, or moved with
  **Ctrl+Shift+Left/Right**. A tab's round ✕ only lights up under the pointer, and stays dark
  while a tab is dragged.
- On **Sites**, the New site and Parked folders forms fold away under one heading, closed at first.
  Stackyard remembers whether you left it open; **New site** in the side bar and on Home opens it.
- On **AI tools**, the **Choose folder…** button lines up with the Site or folder list.
- **Long jobs show how far along they are.** Site backups, exports, restores and imports show a
  bar with the step they are on (saving the database, packing files 1,234 of 5,000, writing files,
  loading the database). Downloads of services, runtimes, phpMyAdmin and cloudflared (for public
  links) show the megabytes so far, the total and a percent.
- **After a backup you see where it went.** A site backup says the file it saved, with **Show in
  folder**. Database backups say the file name and have **Show backups folder**.
- **Turned-off services show on the Services screen** as dashed **Disabled** cards with what they
  do and a **Turn on** button, so you can see everything Stackyard offers. They still download
  nothing and run nothing until turned on.
- WordPress **Plugins** no longer stop with "WP-CLI answered in a way Stackyard doesn't understand"
  when a plugin or drop-in has no version or update (WP-CLI sends `false` for those).
- WordPress **Themes**: **Activate** now comes before **Delete** under each theme.
- WordPress **Database**: **Change prefix…** gives the site's tables a new prefix, e.g. `wp_` to
  `shop_`. It backs up the database, renames every table, the user roles and user settings that
  carry the prefix, and sets `$table_prefix` in wp-config.php.
- A site shows **one address**: its own (`https://blog.stack`) once local domains are set up, and
  its `.localhost` address only until then.

## 0.9.0

- The **Problems, Logs and Terminal** panel now runs along the bottom of the whole window, under
  the side bar too. The setting that put it on the right is gone; a `panelPosition` line in
  settings.toml is skipped.
- The **light and dark switch** (sun or moon) moved from the status bar to the top right of the
  window.
- Every text box, list, check box, radio button and slider has the same look in every screen, in
  the theme's colors, and buttons light up under the pointer. Lists draw their own arrow.
- **Scroll bars** are slim and rounded, like the system's own, and brighten under the pointer.
- **Services and sites open in tabs.** Click a service's name on Home or Services to open it in a
  tab of its own, with start, stop, restart, remove, its ports, how to connect and its log. Click a
  site in the side bar, or **Manage** on the Sites screen, to open the site in a tab with its
  addresses, versions, Terminal, Open in browser and all its tools across the whole window. Close
  any of them with the tab's ×.
- The **Terminal, Manage and Remove** buttons on the Sites screen have room between them.
- A tab's **close button** sits a little apart from the tab's name and lights up as a round button
  when you point at it, instead of a cramped square against the tab's edge.
- **A new WordPress view**, laid out like Local and WordPress Studio: a picture of the site's home
  page with **Open site** and **WP Admin** on top, tabs underlined instead of boxed, site details
  next to Customize shortcuts into wp-admin (Site editor and Styles for block themes, Customizer,
  Menus and Widgets for classic ones, Pages, Posts, Media, Settings).
- **Plugins and themes as in wp-admin**: **Activate** and **Deactivate** instead of switching on
  and off, active plugins marked, All / Active / Inactive / Update available filters and a search
  box, themes as picture cards, and **Install and activate**.
- **Plugins and Themes open at once**: the lists are kept while the app runs and the slow update
  check with wordpress.org runs once per list (and on **Check for updates**), instead of every time
  the tab opened. The "the first time, WP-CLI is downloaded" message only shows when it really is
  the first time.
- **Pictures of sites** in the Sites list and the WordPress view: a screenshot of the home page
  taken with the Edge, Chrome, Chromium or Brave already on the computer, or the theme's own
  picture without one.
- **The database server a site needs**: Stackyard finds which of MySQL and MariaDB holds a site's
  database. When that server is not running, the Sites list and the WordPress view say so, with
  **Start MariaDB** (or **Stop MySQL and start MariaDB**), and a site can have its database server
  start whenever the web server starts.
- The Manage button shows for WordPress sites when only the WordPress tools are switched on.

## 0.8.0

- A **WordPress** tab in the tools of every WordPress site (Sites › Manage): an overview with disk
  use, plugins and themes (switch on and off, update, install from wordpress.org), users (log in as
  anyone with a one-time link, new passwords, new administrators), debug switches with `debug.log`
  followed live, search and replace that keeps serialized data intact, database export, import and
  upkeep, and WooCommerce tools (scheduled actions, payment methods in test mode, test orders and
  customers) for stores with and without High-Performance Order Storage. WP-CLI is downloaded the
  first time it is needed. A module you can switch off. See [docs/wordpress.md](docs/wordpress.md).
- **Clone from a live server** (Sites): copy a live WordPress or WooCommerce site to this computer
  over SSH, SFTP or FTP, or from a hosting panel backup (cPanel, Plesk, DirectAdmin) or a zip and
  `.sql` file. The database password is read from the live `wp-config.php`; every live address is
  changed to the local one; mail goes to Mailpit, search engines are kept away, scheduled tasks
  can't call outside services and payment methods go into test mode. Media can be downloaded or
  left on the live site and loaded when a page needs it. Large sites can be stopped and continued,
  and a broken-off download continues where it stopped (in the Servers tab too).
- **Stagings** (WordPress › Environments): make a full copy of a WordPress site next to it, such as
  `checkout.shop.stack`, from the main site, another staging or the live site. Files are copied
  instantly on disks that can share blocks; media can be left out and copied from the main site as
  pages need it. Destroyed stagings stay in a trash for 7 days. Snapshots taken before Stackyard
  changes a copy can be undone from the same tab.
- **Merge a staging** into the main site or another staging: a list of changed files, plugin and
  theme switches and settings since the staging was made, with anything the other copy also changed
  marked for you to decide, merged at three levels: code only, code and picked settings, or
  everything with the other copy's users, orders and customers kept. **Move single posts and pages**
  between copies with their images, categories and custom fields, every post number changed to
  match, and a preview that says what can't be changed safely. Both can be undone.
- **Push to live and pull from live** for copies cloned from a live server (WordPress ›
  Environments): what changed on each side since they were last in step, at the same three levels
  as a merge, or single posts and pages in either direction. A push backs up the live database and
  every file it changes first, shows the maintenance page while it works, checks the site answers
  afterwards and puts the backup back by itself if a step fails. Pushing everything needs the site's
  name typed, keeps live orders, customers and payment settings, and stops if posts changed on the
  live site meanwhile. Backups of the live site can be put back from the same tab.
- **`stackyard wp`** in the terminal: clone live sites, make, list, merge and destroy stagings,
  push and pull (listing the changes until you add `--yes`), undo from snapshots, and any WP-CLI
  command in a site. AI tools get new MCP tools to list WordPress environments, and once allowed,
  make a staging, run WP-CLI, merge and destroy stagings; pushing to live stays in the app.

- Stackyard is now licensed under the GNU Affero General Public License v3.0 (AGPL-3.0) instead of
  MIT. Earlier releases stay under MIT.
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
- Pick any accent color: the rainbow swatch in Settings › Appearance opens a color picker and a box
  for a hex code like `#3fa7d6`. Stackyard darkens or lightens it where needed so text stays readable.
- **Show the status bar** switch in Settings › Appearance.
- New app icon: the Stackyard bricks stepping down like an S.
- A **Terminal** tab in the panel (**Ctrl+`**). Open it from a site's row on the Sites screen and it
  starts in that site's folder with its PHP and Node.js versions. A module you can switch off.
- Installing SeaweedFS works again when its newest release doesn't have a file for your computer yet:
  Stackyard takes the newest release that does.

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
