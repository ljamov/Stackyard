# Updates and releases

How the app updates itself, and how maintainers publish a new version.

Stackyard checks for a newer version when it starts and twice a day while it runs (switch the
**Updates** module off in Settings to stop that). It never installs on its own: **Settings** shows
the new version and its notes, and **Install and restart** downloads it, stops the running
services, installs it and starts them again in the new version. Every update is signed with a
key only the maintainer holds, and the app refuses one whose signature doesn't match the public
key built into it.

The public key is in `src-tauri/tauri.conf.json` under `plugins.updater.pubkey`; the private key
lives only in the `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` repository
secrets, and the Installers workflow checks that the two match. To publish an update, raise the
version, then either push a `v*` tag or run the Installers workflow on master with "release" ticked,
and publish the draft release it makes (with "Set as the latest release" ticked). The app downloads updates from
`https://github.com/<repo>/releases/latest`, so that repository must be public; set the
`STACKYARD_RELEASES_REPO` variable (and a `STACKYARD_RELEASES_TOKEN` secret that can write to it)
to publish releases somewhere other than this repository.

---

[← Back to the README](../README.md)
