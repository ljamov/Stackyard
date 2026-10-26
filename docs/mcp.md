# AI coding tools (MCP)

Let Claude Code, Cursor and other AI tools see and manage your local stack, with you deciding what they may do.

`stackyard mcp` is an [MCP](https://modelcontextprotocol.io) server, so Claude Code, Cursor,
Claude Desktop, Windsurf and other AI tools can see and manage your local stack. Switch on the
**MCP server** module in Settings (or `stackyard modules on mcp`), then add it to your AI tool;
the MCP server screen and `stackyard mcp setup` show the exact line, for example:

```sh
claude mcp add --scope user stackyard -- stackyard mcp
```

With the module off, AI tools get no tools at all and nothing runs.

## What AI tools may do

These are always allowed, because they only look:

| Tool | What it does |
| --- | --- |
| `list_sites` | Sites with their address, folder, PHP or Node.js version, framework and any problem |
| `list_services` | Services, whether they are installed and running, and their ports |
| `list_runtimes` | Installed PHP, Node.js, Python, Go and Java versions and the defaults |
| `list_databases` | MySQL, MariaDB or PostgreSQL databases with sizes, or one database's tables (names only, never the data) |
| `read_logs` | The last lines of a service's, a PHP version's or a scheduled task's log |
| `list_tasks` | Scheduled tasks with their schedule, next run and how the last run went (needs the *Task scheduler* module) |
| `list_backups` | Database backups and site backups, with size and date |
| `list_wordpress_environments` | WordPress sites with their stagings and the live site each was cloned from |
| `wordpress_overview` | A WordPress site's versions, database, theme, plugins and themes, debug settings (the live site's too, for a copy), snapshots and live backups |
| `read_wordpress_log` | The last lines of a WordPress site's `debug.log` |

These are off until you allow them, in **Settings › AI tools**, on the **MCP server** screen, or
with `stackyard mcp allow <tool>` (`stackyard mcp allow all` for every one, `deny` to turn off):

| Tool | What it does |
| --- | --- |
| `start_service`, `stop_service`, `restart_service` | Start, stop or restart an installed service |
| `create_site` | Add a site for a project folder |
| `set_site_runtime` | Switch the PHP, Node.js, Python, Go or Java version a site runs on |
| `install_runtime` | Download and install a runtime version, e.g. PHP 8.3 before switching a site to it |
| `backup_database` | Save a MySQL, MariaDB or PostgreSQL database to a new backup file |
| `backup_site` | Pack a site's files and database into a new backup zip (needs the *Site backups* module) |
| `query_database` | Run one `SELECT`, `SHOW`, `DESCRIBE` or `EXPLAIN` statement and return the rows |
| `change_database` | Run SQL that changes data or tables (`INSERT`, `UPDATE`, `DELETE`, `CREATE TABLE`...) |
| `create_wordpress_staging` | Make a staging of a WordPress site, from the main site, another staging or the live site |
| `set_wordpress_debug` | Switch `WP_DEBUG`, `WP_DEBUG_LOG`, `WP_DEBUG_DISPLAY`, `SCRIPT_DEBUG` and `SAVEQUERIES` in a local copy's `wp-config.php` |
| `run_wp_cli` | Run a WP-CLI command in a local WordPress site |
| `merge_wordpress_staging` | List a staging's changes, or merge them into the main site or another staging |
| `destroy_wordpress_staging` | Move a staging to the trash, where it stays 7 days |

The WordPress tools appear only while the *WordPress tools* module is on, and work for sites in
parked folders too. Pushing to or pulling from a live site, cloning a live site and uploading to
or downloading from your servers are never offered to AI tools: you do those yourself in the app.
`set_wordpress_debug` changes only this computer's `wp-config.php`, which a push never sends.
Restoring a backup or a snapshot is also left to you.

An AI tool that is already connected sees tools appear and disappear as you switch them, without
reconnecting.

## Databases

The database tools work with MySQL or MariaDB, whichever is installed and switched on (the running
one when both are). `list_databases` and `backup_database` also take `server: "postgres"` for
PostgreSQL; querying and changing stay with MySQL and MariaDB. `query_database` runs in a read-only transaction, so even a query that tries
to write is refused by the server. Results return 100 rows by default, up to 1000 with `max_rows`,
and long values are cut off.

`change_database` can change and delete rows and tables, so allow it only for databases holding
nothing you can't lose; make a backup on the Databases screen first. It never drops a whole
database. Both tools refuse SQL that reads or writes files on your computer (`INTO OUTFILE`,
`LOAD DATA INFILE`, `LOAD_FILE`) and the `mysql` client's own commands, such as `\!` or `system`.

## The activity log

Every call is recorded with the AI tool's name, what it asked for and whether it worked. See it on
the MCP server screen or with `stackyard mcp log`.

Deleting sites, whole databases or files is never offered.

---

[← Back to the README](../README.md)
