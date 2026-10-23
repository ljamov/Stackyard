# Site settings, backups, moving sites and public links

Open a site's tools with **Manage** on its row on the Sites screen, or by clicking the site in the
side bar: the site opens in a tab of its own, with its addresses, folder and versions at the top and
its tools under them. Each part is a module of its own, so any of them can be switched off in
Settings › Modules.

## Settings

| Setting | What it does |
| --- | --- |
| Domain ending | This site's own ending, e.g. `.test`, while other sites keep theirs. Its servers and Git connections move with it. |
| Folder served | The folder inside the project the web server serves, e.g. `public` or `web`. Empty lets Stackyard pick it from the framework. Not used by sites whose own server Stackyard runs (Node, Python, Go, Java). |
| HTTPS | Serve the site over HTTPS with the local certificate, or plain HTTP. |
| Database | The database that goes into the site's backups and exports. Empty means the one named in the project's `.env` (`DB_DATABASE` with `DB_CONNECTION`, or `DATABASE_URL`) or `wp-config.php` (`DB_NAME`). |
| Start command | A command Stackyard runs in the project folder while the web server runs, e.g. `npm run dev` for a Laravel site's Vite, or `php artisan queue:work`. It runs with the site's runtime versions first on `PATH`. For Node, Python, Go and Java sites it replaces the usual server command and gets the site's port in `PORT`. Its output is in the log viewer as `dev-<site>`. |
| Environment variables | `NAME=value`, one per line. PHP sees them in `getenv()` and `$_SERVER`; the site's servers and start command get them too. |
| PHP settings | `php.ini` lines for this site only, e.g. `memory_limit = 1G`. |

PHP keeps a setting made for one request for the next requests the same PHP process serves,
whatever site they are for. So a site with PHP settings of its own gets a PHP process of its own
(`php-<version>-<domain>` in the log viewer), which reads the version's `php.ini` and then the
site's lines. Other sites share the version's process as before. Extensions can't be turned on per
site; turn them on in the version's `php.ini` (`config/php/<version>/php.ini`).

Sites found in a parked folder use the usual settings. To change them, add the folder as a site.

## Backups

**Back up now** writes one zip with:

- the project folder, without `node_modules` (`npm install` makes it again) and without links;
- a dump of the site's database, when it has one and the database server is running (a database
  picked in the settings must work; one found in the project is left out with a note when it
  can't be dumped);
- the site's settings.

Backups are kept in `backups/sites/<domain>/` in Stackyard's data folder (**Show backups folder**).
**Restore** puts the site's files and database back as they were in the backup. Files in the
backup replace the site's files of the same name; files added since are kept. Tables in the
backup replace tables of the same name. Before restoring, Stackyard backs the site up as it is
now, so a restore can be undone by restoring that newest backup.

## Moving a site to another computer

**Export to a file…** saves the same zip wherever you pick. On the other computer, click
**Import site…** at the top of the Sites screen, pick the file, check the name and folder (a new
or empty folder; the sites folder by default) and click **Import site**. The site gets its
settings, runtime version and domain ending back, and its database is loaded. Start MySQL, MariaDB
or PostgreSQL first so the database can be loaded; if it can't be, the files are still put in
place and the file is kept with the site's backups, so the database can be restored from there
later. Install the site's runtime version on the Runtimes screen if that computer doesn't have it.

## Public links

**Public link › Make a public link** gives the site a temporary address like
`https://some-random-words.trycloudflare.com` that anyone can open, to show work to a client or
test on a phone. It is off by default; switch on **Public links** in Settings › Modules.

- It uses Cloudflare's free [quick tunnels](https://try.cloudflare.com/).
  No account is needed. They are meant for testing: Cloudflare promises no uptime and its terms of
  use apply. Don't use them to host a live site.
- The first link downloads `cloudflared`, Cloudflare's own program (Apache 2.0 licence), from
  [its GitHub releases](https://github.com/cloudflare/cloudflared/releases) into `tools/cloudflared`,
  checked against the SHA-256 GitHub records for it. **Remove it** takes it away again.
- Visits go to the local web server with the site's own name, so they are served exactly like
  visits on this computer. The web server must be running.
- A link lasts until you click **Stop link**, remove the site or quit Stackyard. Each new link
  gets a new address. Anyone with the address can open the site while the link is on.
- Apps that write their own address into pages (WordPress's Site Address, Laravel's `APP_URL`)
  may still link to `<name>.stack` here and there; those links only work on this computer.

## Command line

```sh
stackyard sites backup blog            # back up blog.stack
stackyard sites backups blog           # list its backups
stackyard sites restore blog blog.stack-20261017-101500.zip
stackyard sites export blog ~/blog.zip # one file for another computer
stackyard sites import ~/blog.zip --name blog2 --path ~/Code/blog2
```

---

[← Back to the README](../README.md)
