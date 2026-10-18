# Terminal PATH Integration

Tauri's [NSIS installer hooks](https://v2.tauri.app/distribute/windows-installer/#extending-the-installer) run after copying binaries and before uninstalling them. The installer remains `currentUser`; no PATH operation requests elevation. Both Cargo terminal binaries are bundled normally, not as sidecars.

The hook embeds `path.ps1` in the installer/uninstaller and extracts it into NSIS's private plugin directory. It runs Windows PowerShell directly with `-NoProfile -NonInteractive -File`; paths are arguments, not evaluated PowerShell source. The script edits only `HKCU\Environment\Path`, never the process or machine PATH. Machine PATH is read only to avoid claiming a preexisting entry. Unexpanded strings are read using [RegistryKey.GetValue](https://learn.microsoft.com/en-us/dotnet/api/microsoft.win32.registrykey.getvalue), with the original string kind retained on write. PATH never travels through NSIS's bounded strings or `setx`.

Ownership records live under `HKCU\Software\com.fxserver.installer\TerminalPath`, keyed by the normalized installation directory hash. Reinstalling the same directory retains ownership. Removal restores the exact original value if otherwise unchanged, or removes a single unchanged owned entry while retaining later edits. Preexisting, duplicate, or manually reformatted entries are never removed speculatively. A failed ownership write may leave an unclaimed entry; the warning is intentional and cleanup must then be manual. A per-user mutex serializes these hooks, and a fresh read detects intervening external edits before writing. The registry provides no compare-and-swap here, so other PATH editors should not run concurrently with setup.

The hook broadcasts [WM_SETTINGCHANGE](https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-settingchange) with `Environment`; already-running terminal processes may still need restarting. Restricted PowerShell policy or unsafe install-directory characters produce a visible installer warning without overwriting PATH.

## Non-Installing Checks

From the repository root:

```powershell
node --test scripts/test-cli-packaging.mjs
powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File scripts/test-windows-path.ps1
cargo build --manifest-path src-tauri/Cargo.toml --locked --no-default-features --features terminal --bin fxins --bin fxserver-manager
node scripts/smoke-cli.mjs src-tauri/target/debug/fxins.exe src-tauri/target/debug/fxserver-manager.exe
```

The PATH tests dot-source function definitions and supply in-memory keys: they never call the production registry entrypoint, broadcast environment changes, or launch an installer. CLI smoke tests use a disposable settings root and do not start FXServer or contact a database. Installer compilation and disposable-VM install/upgrade/uninstall still need separate validation; these tests do not prove the NSIS runtime lifecycle.

With NSIS installed, compile the hook fixture using an absolute script path (do not run the output):

```powershell
$fixture = (Resolve-Path scripts/tests/nsis-hooks.nsi).Path
$output = Join-Path $env:TEMP 'fxins-nsis-hooks-test.exe'
makensis.exe /V3 "/DFXINS_TEST_OUT=$output" $fixture
```
