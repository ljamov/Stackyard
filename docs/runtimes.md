# Runtimes and the terminal

How PHP and Node.js versions are installed, and how each project gets the version it asks for.

## Runtimes

Each version installs into its own folder (`runtimes/php/8.4`, `runtimes/node/22`) inside the data
folder, so versions sit side by side. Every download is checked against a published SHA-256
checksum before it is unpacked.

| Runtime | Windows | macOS and Linux |
| --- | --- | --- |
| Node.js 20, 22, 24, 26 | Official builds from nodejs.org | Official builds from nodejs.org |
| PHP 7.4 to 8.5 | Official builds from the PHP project | PHP 8.1 and later, built from the official PHP source by `.github/workflows/php-builds.yml` |

The PHP build job uses static-php-cli to produce self-contained `php` and `php-fpm` binaries for
x86_64 and ARM64, and publishes them with their checksums and licenses on the `php-builds` release.
Download sources can be pointed at a mirror with `STACKYARD_NODE_MIRROR`,
`STACKYARD_PHP_WINDOWS_MIRROR` and `STACKYARD_PHP_MANIFEST`.

## Per-project versions in the terminal

`php`, `composer`, `node`, `npm`, `npx`, `mysql` and `mysqldump` run the version each project asks
for. In order, Stackyard uses:

1. the nearest `.stackyard` file in the folder or above it (`stackyard use php 8.3` writes one):
   ```toml
   php = "8.3"
   node = "22"
   ```
2. the version of a site added for that folder,
3. the default version (Runtimes screen, or `stackyard runtimes default php 8.4`),
4. the newest installed version.

Parked sites follow `.stackyard` files too, so the browser and the terminal agree.

`stackyard shims on` (or **Add to PATH** on the Runtimes screen) creates the commands in `bin/`
inside Stackyard's folder and puts that folder on your PATH: a marked block in `.zshrc`, `.bashrc`,
`.bash_profile` or `.profile` and a fish `conf.d` file on macOS and Linux, your user `Path` on
Windows. No password is needed, and `stackyard shims off` undoes it. `stackyard shell` opens one
terminal with the commands instead, without touching your PATH. `stackyard which` shows which version
each command runs in the current folder, and why.

The commands lead back to a copy of the `stackyard` program in the same folder (which also puts
`stackyard` itself on your PATH), and it runs the real one from the runtime's folder.
Composer is downloaded from getcomposer.org the first time you run `composer`, checked against its
published SHA-256. `mysql` connects to whichever of MySQL and MariaDB is running. `psql`, `pg_dump`
and `redis-cli` come from Stackyard's PostgreSQL and Redis, and `psql` and `pg_dump` connect as user
`postgres` without a password.

---

[← Back to the README](../README.md)
