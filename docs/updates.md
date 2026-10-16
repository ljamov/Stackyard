# Updates and releases

How the app updates itself, and how maintainers publish a new version.

Stackyard checks for a newer version when it starts and every four hours while it runs (switch
the **Updates** module off in Settings to stop that). When it finds one, it shows a system
notification (unless the window is in front already), adds **Update to …** to the tray menu and
puts a notice at the top of the window with the version's notes and an **Update now** button. A
version that came out while Stackyard was closed is found at the next start. It never installs on
its own: **Update now** (or **Install and restart** in Settings) downloads it, stops the running
services, installs it and starts them again in the new version, which then shows what changed. Every update is signed with a
key only the maintainer holds, and the app refuses one whose signature doesn't match the public
key built into it.

The public key is in `src-tauri/tauri.conf.json` under `plugins.updater.pubkey`; the private key
lives only in the `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` repository
secrets, and the Installers workflow checks that the two match. To publish an update, raise the
version, rename the `## Unreleased` heading in `CHANGELOG.md` to that version (its section becomes
the release notes the app shows, so write it for people using Stackyard), then either push a `v*` tag or run the Installers workflow on master with "release" ticked,
and publish the draft release it makes (with "Set as the latest release" ticked). The app downloads updates from
`https://github.com/<repo>/releases/latest`, so that repository must be public; set the
`STACKYARD_RELEASES_REPO` variable (and a `STACKYARD_RELEASES_TOKEN` secret that can write to it)
to publish releases somewhere other than this repository.

---

[← Back to the README](../README.md)
