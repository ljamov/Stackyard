# Services

Web server, databases, cache, search, mail, file storage and queues, all running natively on your
computer.

## Services

Services install into `services/<id>/<line>`, keep their data in `data/<id>` (kept when a service
is removed, so reinstalling brings databases back) and their config in `config/<id>`. Everything
listens on `127.0.0.1` only. MySQL and MariaDB start with user `root` and PostgreSQL with user
`postgres`, all without a password.

| Service | Ports | Source | Checked against |
| --- | --- | --- | --- |
| Caddy 2 (web server) | 80, 443 (8080, 8443 when taken) | Official GitHub releases | Caddy's SHA-512 list |
| MySQL 8.4 | 3306 | Oracle's official downloads | SHA-256 from `.github/workflows/mysql-manifest.yml`, which first checks Oracle's GnuPG signature |
| MariaDB 11.8 (Windows, Linux) | 3306 | MariaDB Foundation downloads | The Foundation's SHA-256 |
| PostgreSQL 18 | 5432 | Built from the official source (macOS, Linux); the official EDB binaries without pgAdmin (Windows) | SHA-256 from `.github/workflows/postgres-builds.yml`, which first checks postgresql.org's SHA-256 |
| Redis 8 (macOS, Linux) | 6379 | Built from the official source | SHA-256 from `.github/workflows/redis-builds.yml`, which first checks Redis's published SHA-256 |
| Mailpit (mail catcher) | 1025, 8025 | Official GitHub releases | GitHub's SHA-256 digest |
| Meilisearch 1 (search engine) | 7700 | Official GitHub releases | GitHub's SHA-256 digest |
| MongoDB 9.0 (document database) | 27017 | MongoDB's official downloads (Community Server) | The SHA-256 in MongoDB's download list |
| SeaweedFS 4 (S3-compatible storage) | 8333 (S3), 23646 (manager), 9333, 9340, 8888 | Official GitHub releases | GitHub's SHA-256 digest |
| NATS 2 (message queue) | 4222, 8222 (monitoring) | Official GitHub releases | GitHub's SHA-256 digest |

Each service is a module: MySQL and Caddy are on by default, MariaDB, PostgreSQL, Redis, Mailpit,
Meilisearch, MongoDB, SeaweedFS and NATS can be switched on in Settings. `STACKYARD_MYSQL_MANIFEST`,
`STACKYARD_POSTGRES_MANIFEST` and `STACKYARD_REDIS_MANIFEST` point those services at another
manifest, and
`STACKYARD_MONGODB_LIST` points MongoDB at another download list, and
`STACKYARD_GITHUB_TOKEN` raises the GitHub API limit for Caddy, Mailpit, Meilisearch, SeaweedFS
and NATS lookups.

Each service card shows how apps connect (for example `mongodb://127.0.0.1:27017`), with the port
it really uses.

Meilisearch runs in development mode with no master key, so apps connect to
`http://127.0.0.1:7700` without one and the address opens its search preview page. Its indexes
live in `data/meilisearch` and are carried over when Stackyard updates it to a newer 1.x release.

PostgreSQL keeps its databases in `data/postgres/db`, created the first time it starts with the C
locale and UTF-8, so sorting is the same on every system. It only accepts connections over TCP
(`127.0.0.1:5432`), never a socket file, so every client connects the same way everywhere. Redis
saves to `data/redis/dump.rdb` every few minutes while keys change, so queued jobs survive a
restart. Redis publishes no Windows version, so it is not offered there.

MongoDB is MongoDB Community Server, which is under the SSPL licence. Stackyard downloads it from
mongodb.com for you when you install it and never ships it itself. Linux builds are made per
distribution: Stackyard picks the one for yours from `/etc/os-release` (Ubuntu, Debian, RHEL and
its relatives, Fedora, Amazon Linux, SUSE), or the oldest Ubuntu build on other distributions.
Databases live in `data/mongodb/db`; connect with `mongodb://127.0.0.1:27017` and no password, or
open it in MongoDB Compass.

SeaweedFS gives apps an S3 endpoint at `http://127.0.0.1:8333`, for file uploads and anything else
that would go to AWS S3 or MinIO. It takes the place of MinIO, whose free edition stopped
publishing downloads in 2025 and is no longer maintained; SeaweedFS is Apache 2.0 licensed and
actively developed. Sign in with access key `stackyard` and secret key `stackyard-secret`, use any
region (`us-east-1` is fine) and path-style addresses. Buckets are created when an app first
writes to them, or on the bucket and key manager page at `http://127.0.0.1:23646` (the **Open**
button). For Laravel:

```env
FILESYSTEM_DISK=s3
AWS_ACCESS_KEY_ID=stackyard
AWS_SECRET_ACCESS_KEY=stackyard-secret
AWS_DEFAULT_REGION=us-east-1
AWS_BUCKET=uploads
AWS_ENDPOINT=http://127.0.0.1:8333
AWS_USE_PATH_STYLE_ENDPOINT=true
```

Files live in `data/seaweedfs`. Its other ports are used by its own parts and need nothing from
you.

NATS is a small message queue. JetStream is on, so streams and their messages are kept in
`data/nats` and survive a restart. Apps connect to `nats://127.0.0.1:4222`; the **Open** button
shows its monitoring page. It was picked over RabbitMQ because it is one small program with no
runtime to install (RabbitMQ needs Erlang) and runs the same on Windows, macOS and Linux. On
macOS and Linux, Redis also works as a queue for Laravel, Sidekiq and similar tools.

## Databases screen

The **Databases** screen works with whichever of MySQL and MariaDB is running, and with
PostgreSQL: when both kinds are installed, buttons at the top switch between them. List and
create databases, page through a table's rows, run SQL (up to 1,000 rows are shown, and NULL is
told apart from an empty value), and back up or restore. Backups are gzipped SQL files in
`backups/<database>` inside Stackyard's data folder, made with the server's own dump tool, so they
load into any MySQL or MariaDB. PostgreSQL backups are named `<database>-<time>.pg.sql.gz`, made
with `pg_dump` without owners or permissions so they load into any database, and are only offered
for PostgreSQL. A backup can replace the tables of its database or be restored as a copy under
another name. Dropping a database asks you to type its name; `mysql`, `sys` and the other system
databases, and PostgreSQL's `postgres` and template databases, can't be dropped from here.
PostgreSQL tables outside the `public` schema are listed as `schema.table`. In the terminal, add
`--postgres` to any `stackyard db` command, for example `stackyard db --postgres backup shop`. Switch the screen off in Settings if you use
another database tool.

## Logs

The **Logs** screen shows every service's log (the web server, each PHP version, databases,
Mailpit and the background service) in one place, follows it as it grows, and searches it with
plain text or a regular expression. Errors and warnings stand out. Logs live in the `logs`
folder of Stackyard's data folder; one that grows past 5 MB starts over and the previous one is
kept as `.log.1`. Switch the screen off in Settings if you don't need it.

## Disk space

**Settings → Disk space** shows how much Stackyard's own folder takes (runtimes, services, their
data, logs, downloads) and what can go. Safe items are picked for you: downloads and folders left
by interrupted installs, previous logs, versions Stackyard no longer offers. Others are yours to
pick: emptying the current logs, runtime versions that are not the default and that no site uses
or pins, and the kept data of a service you removed, which asks once more because it can't be
undone. Nothing is read until you ask, and project folders are never counted or touched.

## Running in the background

One process at a time runs services, and every other part of Stackyard talks to it:

- **The app.** Closing the window keeps Stackyard in the tray (menu bar on macOS) and frees the
  window's memory; the tray menu opens it again, starts or stops every service, or quits. Quitting
  stops all services. Opening the app a second time brings the first window forward.
- **`stackyard start`** when the app is closed: it starts a small background process
  (`stackyard daemon`) that keeps services running after the terminal closes. `stackyard stop`
  stops everything and ends it; opening the app takes its services over.
- **`stackyard up`**: the same in the foreground of one terminal, until Ctrl+C.

`stackyard start`, `stop`, `status` and site changes from the CLI all reach whichever of these runs
services, so a site added in a terminal is served right away by the app. They talk over
`127.0.0.1` with a secret token kept in `run/control.json`, readable only by you.

---

[← Back to the README](../README.md)
