# Runtimes and the terminal

How PHP, Node.js, Python, Go and Java versions are installed, and how each project gets the version
it asks for.

## Runtimes

Each version installs into its own folder (`runtimes/php/8.4`, `runtimes/python/3.13`) inside the data
folder, so versions sit side by side. Every download is checked against a published SHA-256
checksum before it is unpacked.

| Runtime | Windows | macOS and Linux |
| --- | --- | --- |
| Node.js 20, 22, 24, 26 | Official builds from nodejs.org | Official builds from nodejs.org |
| PHP 7.4 to 8.5 | Official builds from the PHP project | PHP 8.1 and later, built from the official PHP source by `.github/workflows/php-builds.yml` |
| Python 3.11 to 3.15 | Portable CPython builds from the python-build-standalone project | Same |
| Go 1.25, 1.26, 1.27 | Official builds from go.dev | Official builds from go.dev |
| Java 11, 17, 21, 25 | Eclipse Temurin (OpenJDK) from Eclipse Adoptium | Same |

Python, Go and Java are modules that start switched off: turn them on in Settings › Modules (or
`stackyard modules on python`). python.org has no Python that unpacks into a folder for macOS and
Linux, so Stackyard uses the portable CPython builds that `uv` and other Python tools install, with
pip and venv included. Go and Java checksums come from go.dev's and Adoptium's own release lists,
Python's from the `SHA256SUMS` file of the python-build-standalone release.

The PHP build job uses static-php-cli to produce self-contained `php` and `php-fpm` binaries for
x86_64 and ARM64, and publishes them with their checksums and licenses on the `php-builds` release.
Download sources can be pointed at a mirror with `STACKYARD_NODE_MIRROR`,
`STACKYARD_PHP_WINDOWS_MIRROR`, `STACKYARD_PHP_MANIFEST`, `STACKYARD_PYTHON_MIRROR` (a
`latest-release.json`), `STACKYARD_GO_MIRROR` and `STACKYARD_JAVA_MIRROR` (the Adoptium API).

## Per-project versions

Each site, and each project folder in a terminal, runs the runtime versions it asks for. In
order, Stackyard uses:

1. the nearest version file in the folder or above it:
   - a `.stackyard` file (`stackyard use php 8.3` writes one):
     ```toml
     php = "8.3"
     node = "22"
     ```
   - or a file another tool already uses: `.nvmrc` or `.node-version` (`v22.11.0` means Node 22),
     `.php-version`, or asdf's and mise's `.tool-versions` (`nodejs 22.11.0`, `php 8.3.12`).
     `.python-version` (pyenv), `.go-version`, `.java-version` (jenv) and SDKMAN's `.sdkmanrc`
     (`java=21.0.2-tem`) are read the same way.

   In one folder `.stackyard` wins over the others. A version Stackyard does not offer (such as
   `lts/*`) in another tool's file is skipped, since the file is written for that tool.
2. the version saved with the site added for that folder,
3. the default version (Runtimes screen, or `stackyard runtimes default php 8.4`),
4. the newest installed version.

The web server, the servers Stackyard runs for Node, Python, Go and Java sites, and the terminal all follow this
order, so the browser and the terminal always agree.

### Picking versions on the Sites screen

Each site in the list has a picker for every runtime its project uses: the one it is served with,
Node.js (for build tools such as Vite and Tailwind), and any other its folder pins. Pick a version
and the site switches to it; the web server restarts by itself. A version marked **(install)** is
downloaded first. **Default** follows the default version from the Runtimes screen.

The version a site is served with is saved with the site. Any other runtime, and every runtime of a
parked site, is written to a `.stackyard` file in the project folder, so `npm` in a terminal there
uses it too. When a version file already decides, the picker updates `.stackyard` in the project
folder to match, so the file and the site never disagree.

## The terminal

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

`python`, `python3`, `pip` and `pip3` run the project's Python (`pip` is `python -m pip`), `go` and
`gofmt` its Go, and `java`, `javac` and `jar` its Java, with `JAVA_HOME` set so Maven and Gradle find
the same JDK. Until Stackyard has a version of its own installed (or with the module off), these
commands run the Python, Go or Java already on your computer, so turning the commands on never
hides them. A project that pins a version Stackyard has not installed gets the usual "install it"
message instead. Inside an activated virtual environment, its `python` comes first on PATH as
usual.

---

[← Back to the README](../README.md)
