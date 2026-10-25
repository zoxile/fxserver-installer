# Signed In-App Updates

Released Windows x64 builds can download a signed update inside the application, show progress, and hand installation to the bundled NSIS updater. The application shuts down its managed work before the installer replaces it; NSIS shows a passive progress window and restarts the updated app. This is not a live replacement of the running executable.

Existing releases without the updater still need one manual installer update. Tauri updater signatures are separate from Windows Authenticode signing and do not remove SmartScreen reputation warnings.

## Channels And Platforms

`update-channel.json` is compiled into the app:

- `development`: all in-app checks, downloads, and installations are disabled on every operating system, including Windows development builds.
- `beta`: published Windows x64 builds may install newer beta or stable releases.
- `stable`: published Windows x64 builds accept stable releases only.

Debug builds are always disabled. Linux packages and AppImages currently require manual updates even if a release channel is accidentally selected. An eventual Linux updater needs its own signed AppImage publication and installation validation; it must not consume the Windows installer or replace this development branch with the main branch.

Do not change the development branch's channel to make the Update button work. Set the main branch's channel to `beta` when publishing beta installers, and keep its version in `release-policy.json`.

## Backend Contract

Register `tauri_plugin_updater::Builder::new().build()`, manage `AppUpdateManager::new(prepare_install)`, and register these app commands:

| Command | Purpose |
| --- | --- |
| `get_app_update_status` | Return current in-memory progress without a network request. |
| `check_app_update` | Discover a compatible release and validate its exact-tag updater feed. Pass `{ force: true }` for explicit refresh, `false` for automatic checks. |
| `download_app_update` | Download and cryptographically verify the selected installer. |
| `install_app_update` | Re-verify the prepared bytes, quiesce the app, and launch NSIS. |

Only the check command accepts a frontend parameter: the cache-refresh flag. Commands return `{ phase, currentVersion, availableVersion, releaseUrl, downloadedBytes, totalBytes, message, supported }`. Optional values serialize as `null`. The event `app-update-progress` carries the same object, with download progress limited to roughly ten emissions per second plus the initial and final notification.

Phases are `disabled`, `idle`, `checking`, `available`, `downloading`, `ready`, `stopping`, `installing`, and `error`. Operation failures reject the command and set the status message; concurrent operations are rejected without replacing current progress. The status command remains available during work. Downloads are kept only in app-owned memory, not in frontend state or a persistent cache.

Do not grant the frontend generic `updater:*` capabilities or use the plugin's JavaScript updater commands. Only the app-owned Rust commands choose endpoints, versions, signatures, and installer arguments.

### Shutdown Integration

`prepare_install: fn(&tauri::AppHandle) -> Result<(), String>` runs on a blocking worker after signature verification, never on the UI thread. It must:

1. Atomically prevent new background work and refuse installation if database, restore, backup, or other blocking tasks are active.
2. Stop scheduled work, health/recovery, and bridge workers so they cannot restart FXServer or launch new jobs.
3. Gracefully stop the managed FXServer, with the existing bounded forced-stop fallback only when necessary.
4. Drain worker shutdown and return only when safe, without calling `app.exit` itself.

The updater separately verifies the managed server is no longer running before installing. Never call installation from inside a shared background-work permit if the shutdown hook waits for those permits; that would deadlock. Installer work instead uses its own exclusive semaphore and a blocking worker.

The updater plugin re-verifies bytes and launches NSIS before exiting the process. A failed installer launch must leave the window able to report the error and quit. The backend reads `commands::background_work_closing()` on installation failures. Once the shutdown gate has closed it disables further updater actions and tells the user to restart; it never silently reopens background work after partial shutdown.

## Download Safeguards

The updater reuses `app_release` discovery, including the public GitHub HTML/Atom route and API fallback. Beta discovery does not rely on GitHub's stable-only `latest` endpoint. It reads `latest.json` from the selected exact release tag, not a mutable global URL.

Only the official repository's exact Windows x64 NSIS asset is accepted. Redirects are limited to HTTPS GitHub release downloads and `release-assets.githubusercontent.com`; credentials in URLs and arbitrary frontend URLs are not accepted. Metadata is limited to 256 KiB and the installer to 256 MiB, even without a `Content-Length` header. Requests have deadlines, and only one update operation can run at a time.

Installer bytes and the signed version are verified with the embedded public key before shutdown and again before installation. Equal versions, downgrades, mismatched tags, other-platform assets, unsigned packages, and invalid signatures are rejected. A missing updater feed on an older release produces a manual-install message rather than an unsigned fallback.

## Release Signing

The private key belongs outside this repository. GitHub Actions uses the `TAURI_SIGNING_PRIVATE_KEY` secret and optional `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` secret. Only the corresponding public key is committed in `src-tauri/tauri.conf.json`. Keep an independently secured backup of the private key; losing it prevents updates to installations trusting that key.

This implementation pins updater 2.14, which requires Tauri 2.12 and verifies version-bound signatures. The CLI must support `tauri signer sign --app-version`; the workflow checks that before building. Signing a custom package manually also requires its version to be supplied. Never disable `requireSignedVersion` to work around an outdated CLI.

The Windows release workflow:

1. Runs application and updater-validator tests.
2. Checks that the source is on `main`, the release channel agrees with the beta policy, and unsafe updater settings are off.
3. Builds NSIS with `createUpdaterArtifacts: true` and signing secrets. This build-only override avoids requiring a release signing key for Linux development bundles.
4. Creates or reuses a draft for that exact commit and channel, uploading the installer, `.sig`, and `latest.json` through the pinned Tauri action.
5. Downloads the uploaded assets using authenticated GitHub API requests, without forwarding the token to the asset CDN.
6. Checks names, limits, version, platform, matching signature files, the installer signature, and its signed version.
7. Publishes the draft only after every check passes. Beta releases remain prereleases.

A failed validation leaves the draft unpublished. Re-running the same commit can reuse its draft. A published version is skipped, never overwritten; use a new version. A draft or pre-existing tag pointing at a different commit requires manual inspection before retrying.

Local validation without installing anything:

```powershell
node --test scripts/test-updater-release.mjs
cargo test --manifest-path src-tauri/Cargo.toml app_update --lib
```

The test suite uses public upstream signature vectors, not the release private key. It tests modified payloads, signature tampering, version mismatches, missing signatures, unversioned legacy signatures, unsafe redirects, limits, and draft publication requirements. It does not run an installer or alter a live server.

References: [Tauri updater guide](https://v2.tauri.app/plugin/updater/), [updater 2.14 Rust API](https://docs.rs/tauri-plugin-updater/2.14.0/tauri_plugin_updater/), and the [pinned Tauri action inputs](https://github.com/tauri-apps/tauri-action/blob/84b9d35b5fc46c1e45415bdb6144030364f7ebc5/action.yml).
