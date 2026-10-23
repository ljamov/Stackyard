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
| `list_runtimes` | Installed PHP and Node.js versions and the defaults |
| `list_databases` | MySQL or MariaDB databases with sizes, or one database's tables (names only, never the data) |
| `read_logs` | The last lines of a service's, a PHP version's or a scheduled task's log |
| `list_wordpress_environments` | WordPress sites with their stagings and the live site each was cloned from |

These are off until you allow them, in **Settings › AI tools**, on the **MCP server** screen, or
with `stackyard mcp allow <tool>` (`stackyard mcp allow all` for every one, `deny` to turn off):

| Tool | What it does |
| --- | --- |
| `start_service`, `stop_service`, `restart_service` | Start, stop or restart an installed service |
| `create_site` | Add a site for a project folder |
| `set_site_runtime` | Switch the PHP or Node.js version a site runs on |
| `query_database` | Run one `SELECT`, `SHOW`, `DESCRIBE` or `EXPLAIN` statement and return the rows |
| `change_database` | Run SQL that changes data or tables (`INSERT`, `UPDATE`, `DELETE`, `CREATE TABLE`...) |
| `create_wordpress_staging` | Make a staging of a WordPress site, from the main site, another staging or the live site |
| `run_wp_cli` | Run a WP-CLI command in a local WordPress site |
| `merge_wordpress_staging` | List a staging's changes, or merge them into the main site or another staging |
| `destroy_wordpress_staging` | Move a staging to the trash, where it stays 7 days |

The WordPress tools appear only while the *WordPress tools* module is on. Pushing to or pulling
from a live site is never offered to AI tools: you do that yourself in the app.

An AI tool that is already connected sees tools appear and disappear as you switch them, without
reconnecting.

## Databases

The database tools work with MySQL or MariaDB, whichever is installed and switched on (the running
one when both are). `query_database` runs in a read-only transaction, so even a query that tries
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
