# WordPress and WooCommerce

Every WordPress site gets a **WordPress** tab in its tools: open **Sites**, click **Manage** on the
site's row, then **WordPress**. It covers the everyday jobs that would otherwise need wp-admin or a
terminal. It is a module (Settings › Modules › *WordPress tools*); switched off, the tab is gone and
nothing is downloaded.

Plugins, themes, users and the WooCommerce actions use [WP-CLI](https://wp-cli.org), WordPress's
own command-line tool. Stackyard downloads it the first time one of them is used (one file, about
7 MB, into `tools/wp-cli` in Stackyard's folder), checked against the SHA-512 checksum WP-CLI
publishes next to it, and runs it with the site's own PHP version. MySQL or MariaDB must be running.

## Clone a live site

**Sites › Clone from a live server…** copies a live WordPress or WooCommerce site to this computer
as a new site: files, database and settings. Nothing on the live site is changed. It needs the
*WordPress tools* and *Remote servers* modules.

1. Say where the live site is:
   - **A server (SFTP or FTP)**: the address, user name and password (or a private key for SFTP)
     your host gives you for SFTP or FTP. Optionally the folder the site is in; otherwise Stackyard
     looks for WordPress in the folder the server starts in and up to three levels below it.
   - **A saved server**: a connection you already saved for another site.
   - **Backup files**: a full backup downloaded from the hosting panel (cPanel, Plesk, DirectAdmin
     and others; `.tar.gz`, `.tar` or `.zip`, with archives inside archives opened too), or a zip
     of the site's files plus the database export (`.sql` or `.sql.gz`). Stackyard finds WordPress
     and the right `.sql` file in them.
2. Click **Look for WordPress**. Stackyard reads the live `wp-config.php` (you never type the
   database password) and shows the WordPress folder, the site's address, the WordPress, PHP and
   database versions, the database size, and how it will copy the site.
3. Pick a name for the copy (`shop` becomes `shop.stack`) and what to do with media:
   - **Download it all**, or
   - **Leave it on the live site**: nothing in `wp-content/uploads` is downloaded now. The first
     time a page asks for a file, the copy fetches it from the live site once and keeps it. Good for
     sites with many gigabytes of images.
4. Click **Clone** and follow the progress. **Stop** stops it; clicking **Clone** again with the
   same name continues where it stopped.

### How Stackyard reaches the live site

| The host offers | Files | Database |
|---|---|---|
| SSH (SFTP with a shell: most VPS, many managed hosts, cPanel with SSH on) | SFTP | `mysqldump` run on the server, table by table, compressed |
| SFTP or FTP only | SFTP, FTPS or FTP | Stackyard's helper, over the site's own address |
| Nothing usable | A backup from the hosting panel | From the backup |

The **helper** is a small PHP file Stackyard writes for this one copy and puts in the site's folder
on the server. It only reads the database, answers only requests that carry its random key, sends
the tables in chunks, and deletes itself when the clone ends, or after 30 minutes at the latest even
if the clone stopped halfway. When the server has SSH but no `mysqldump`, the helper is run with the
server's own PHP over SSH instead of the site's address. Without SSH, enter the **Live site's
address** so Stackyard can reach the helper there.

Big sites take time, so everything can continue after a dropped connection or **Stop**: files already
copied (same size and date) are skipped, a file that broke off is continued from where it stopped,
and the database comes in chunks whose progress is kept. Caches and the backups that backup plugins
keep inside the site (UpdraftPlus, All-in-One WP Migration, Duplicator and others) are not copied.

### What the copy gets

- Its own database, with every live address changed to the local one, in plain text, JSON and PHP
  serialized data (see Search and replace below), and the live server's folder path changed to the
  local folder.
- A `wp-config.php` for this computer: Stackyard's database, `WP_ENVIRONMENT_TYPE` set to `local`,
  `WP_HOME` and `WP_SITEURL` changed when they were set, the page cache (`WP_CACHE`) and
  `COOKIE_DOMAIN` switched off. The live file is kept as `wp-config.live.php`.
- The live site's PHP version, when Stackyard offers it (installed if needed); otherwise the
  default, and a note says so.
- An object cache drop-in (`wp-content/object-cache.php`, e.g. for Redis) is switched off, since its
  cache server is not on this computer.
- Safety settings, in the must-use plugin `wp-content/mu-plugins/stackyard.php`:
  - all mail goes to Mailpit (or, with Mailpit off, is dropped), never to real customers;
  - search engines are told not to index the copy;
  - scheduled tasks (WP-Cron) can't call outside services such as payment providers, CRMs or mail
    lists; the copy itself stays reachable;
  - WooCommerce webhooks are held back;
  - WooCommerce payment methods that have a test or sandbox mode are switched to it; those that
    have none are listed, so you can check them before testing checkout.
- The live server's connection, saved with the site as **Live** (Manage › Servers & Git), and the
  live address shown on the WordPress tab's Overview.

Premium plugins whose licence is tied to the live domain may show as unlicensed on the copy;
Stackyard can't change that. A multisite network's main site works; subsites on their own domains
may need their addresses changed under Network Admin › Sites.

## Overview

The WordPress, PHP and database versions, WooCommerce's version, the environment
(`WP_ENVIRONMENT_TYPE`), and how much disk the site, its media, plugins, themes and database take.
Warnings appear when `wp-config.php` points at another database server than Stackyard's, or shows
errors on the pages. A cloned site also shows its live site's address and when it was copied. **Check for a WordPress update** asks wordpress.org and offers the update.

## Plugins and themes

Every plugin and theme with its version, whether it is on, and whether an update is available.
Switch them on or off, update one or **Update all**, delete those that are off, or install one from
wordpress.org by its slug (the last part of its wordpress.org address, e.g. `query-monitor`).

WP-CLI loads WordPress without the other plugins for these jobs, so a broken plugin can't stop
you from switching it off. Switching a plugin on or off loads them all, as wp-admin does, so its
own setup runs.

## Users

- **Log in as** opens the site in the browser already logged in as that user. The link works once,
  for five minutes. It goes through a small must-use plugin Stackyard writes into the site
  (`wp-content/mu-plugins/stackyard.php`), which also shows the copy's name (Local, Staging) in the
  admin bar.
- **New password** sets a new random password and shows it.
- **Add an administrator** makes a new administrator account.

## Debug

Switches for `WP_DEBUG`, `WP_DEBUG_LOG`, `WP_DEBUG_DISPLAY`, `SCRIPT_DEBUG` and `SAVEQUERIES`,
written straight into `wp-config.php`, and `debug.log` underneath, followed live (every two
seconds) while **Follow** is ticked. **Clear** empties the log.

## Search and replace

Finds text everywhere in the site's database and replaces it, for example an old address after a
move. It works on PHP serialized data too (widgets, theme settings, page builders such as Elementor
and Divi), whose string lengths it counts again; a plain text replace would break those. JSON
(`https:\/\/example.com`) and URL-encoded (`https%3A%2F%2Fexample.com`) forms are changed as well.

With **These are site addresses** ticked, `http://` and `https://`, with and without `www.`, and
`//example.com` all become the new address, while other domains that only start the same way
(`example.com.au`, `example.community`) and e-mail addresses are left alone.

**Preview** counts the places per table without changing anything. **Replace** backs the database
up first (Databases screen › Backups), then makes the change. The preview also lists values it
could not change safely: serialized data that was already damaged, and base64-encoded values that
hold the text.

## Database

- **Export…** saves the site's database as `.sql` or, with a name ending `.sql.gz`, compressed.
- **Import…** loads a `.sql` or `.sql.gz` file. Its tables replace the ones of the same name; a
  backup is taken first.
- **Optimize tables** runs `OPTIMIZE TABLE` on every table.
- **Clear transients** removes WordPress's cached values (they are made again when needed).
- **Open in phpMyAdmin** when the phpMyAdmin module is on.

## WooCommerce

Shown when WooCommerce is installed.

- How orders are stored: in WooCommerce's own tables (High-Performance Order Storage, the default
  since WooCommerce 8.2) or as posts, read from the store's settings. Every tool works with both.
- Counts of orders, products and customers, and scheduled actions that are due or failed.
- **Run due actions** runs the scheduled actions that are due (e-mails, webhooks, renewals), as
  the site's cron would.
- **Make 3 test orders** and **Make 3 test customers**. Customers get `@example.test` addresses,
  which never receive mail.
- **Payment methods**: each one that is set up, whether it is on, and whether it is in test mode.
  **Switch to test mode** works for gateways that keep a test switch in their settings (Stripe,
  PayPal and most others); for the rest, use their own settings page.

---

[← Back to the README](../README.md)
