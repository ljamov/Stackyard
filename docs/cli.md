# Command line

Everything the app does is also available as the `stackyard` command. It ships next to the app;
**Add to PATH** on the Runtimes screen (or `stackyard shims on`) puts it on your PATH. Every command
talks to whichever Stackyard is running services (the app, `stackyard start` or `stackyard up`), so a
change made in a terminal shows up in the app right away.

## Sites

```sh
stackyard sites add myapp --path ~/Code/myapp   # PHP, Node or static, detected
stackyard sites add legacy --php 7.4 --path ~/Code/legacy
stackyard sites add shop --node 22 --port 5173
stackyard sites add api --python 3.13 --path ~/Code/api   # also --go 1.26, --java 21
stackyard sites park ~/Code           # every project folder in ~/Code becomes a site
stackyard sites list
stackyard sites remove shop
stackyard system setup                # local domains and HTTPS, asks for your password once
stackyard tld                         # show the domain ending
stackyard tld .test                   # change it
```

## Services

```sh
stackyard start caddy                 # run the web server in the background (`stackyard start` = everything)
stackyard status                      # what is running, and where
stackyard stop                        # stop everything (`stackyard stop mysql` for one)
stackyard up                          # run the web server (and e.g. `stackyard up mysql`) until Ctrl+C
stackyard services                    # services, with installed and switched-off marked
stackyard services install caddy
stackyard services uninstall mailpit  # keeps its data
stackyard logs                        # every log there is
stackyard logs caddy -n 100 -f        # the end of a service's log, then new lines as they come
```

## Runtimes and the terminal

```sh
stackyard runtimes                    # versions, with installed and default marked
stackyard runtimes install php 8.4
stackyard runtimes default php 8.4
stackyard runtimes uninstall node 20
stackyard shims on                    # php, node, composer… with each project's versions, in every terminal
stackyard shell                       # or just in this terminal
stackyard use php 8.3                 # pin a version for this project
stackyard which                       # which version each command runs here, and why
```

## Databases

```sh
stackyard db                          # databases on the running MySQL or MariaDB
stackyard db create shop
stackyard db query shop "SELECT * FROM orders LIMIT 5"
stackyard db backup shop              # backups/shop/shop-<date>-<time>.sql.gz
stackyard db restore shop             # the newest backup (or name a file; --into another database)
stackyard db drop shop --yes
psql -c "create database shop"        # PostgreSQL, once it runs (`stackyard start postgres`)
pg_dump shop > shop.sql
redis-cli ping                        # Redis (`stackyard start redis`)
```

## Scheduled tasks

```sh
stackyard tasks add "Laravel scheduler" --run "php artisan schedule:run" --every "* * * * *"
stackyard tasks                       # tasks, with their next and last runs
stackyard tasks run "Laravel scheduler"   # run one now, here in the terminal
stackyard tasks off "Laravel scheduler"   # `on` switches it back on; `remove` deletes it
stackyard tasks check "0 9 * * mon-fri"   # when a schedule would run
```

## AI coding tools (MCP)

```sh
stackyard mcp setup                   # how to add Stackyard to Claude Code, Cursor and others
stackyard mcp tools                   # what AI tools may do
stackyard mcp allow start_service     # or `allow all`; `deny` turns it off again
stackyard mcp log                     # what AI tools did
```

## Disk space and modules

```sh
stackyard cleanup                     # what takes space, and what can go
stackyard cleanup --recommended       # remove what is safe to remove
stackyard cleanup runtime-php-8.2     # or name items; service data also needs --yes
stackyard modules                     # every module, and whether it is on
stackyard modules on scheduler        # switch one on (`off` switches it off)
```

---

[← Back to the README](../README.md)
