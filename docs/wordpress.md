# WordPress and WooCommerce

Every WordPress site gets a **WordPress** tab in its tools: open **Sites**, click **Manage** on the
site's row, then **WordPress**. It covers the everyday jobs that would otherwise need wp-admin or a
terminal. It is a module (Settings › Modules › *WordPress tools*); switched off, the tab is gone and
nothing is downloaded.

Plugins, themes, users and the WooCommerce actions use [WP-CLI](https://wp-cli.org), WordPress's
own command-line tool. Stackyard downloads it the first time one of them is used (one file, about
7 MB, into `tools/wp-cli` in Stackyard's folder), checked against the SHA-512 checksum WP-CLI
publishes next to it, and runs it with the site's own PHP version. MySQL or MariaDB must be running.

## Overview

The WordPress, PHP and database versions, WooCommerce's version, the environment
(`WP_ENVIRONMENT_TYPE`), and how much disk the site, its media, plugins, themes and database take.
Warnings appear when `wp-config.php` points at another database server than Stackyard's, or shows
errors on the pages. **Check for a WordPress update** asks wordpress.org and offers the update.

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
