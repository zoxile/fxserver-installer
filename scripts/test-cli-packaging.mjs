import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const root = fileURLToPath(new URL('../', import.meta.url));
const read = (path) => readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');

test('Cargo exposes both terminal commands behind the same feature', () => {
  const metadata = JSON.parse(execFileSync('cargo', ['metadata', '--manifest-path', 'src-tauri/Cargo.toml', '--no-deps', '--format-version', '1', '--offline'], { cwd: root, encoding: 'utf8', windowsHide: true }));
  const app = metadata.packages.find((entry) => entry.name === 'app');
  for (const name of ['fxins', 'fxserver-manager']) {
    const binary = app.targets.find((target) => target.name === name);
    assert.deepEqual(binary.kind, ['bin']);
    assert.deepEqual(binary['required-features'], ['terminal']);
    const source = readFileSync(binary.src_path, 'utf8');
    assert.ok(source.indexOf('run_elevated_helper_from_args()') < source.indexOf('terminal::run()'));
    assert.ok(source.indexOf('run_fxserver_watchdog_from_args()') < source.indexOf('terminal::run()'));
  }
  assert.equal(app.default_run, 'app');
});

test('bundles retain the settings identifier and enable both terminal binaries', () => {
  const main = JSON.parse(read('src-tauri/tauri.conf.json'));
  const linux = JSON.parse(read('src-tauri/tauri.linux.conf.json'));
  assert.equal(main.identifier, 'com.fxserver.installer');
  assert.equal(main.bundle.windows.nsis.installMode, 'currentUser');
  assert.equal(main.bundle.windows.nsis.installerHooks, 'packaging/windows/hooks.nsh');
  for (const config of [main, linux]) assert.ok(config.build.features.includes('terminal'));
  assert.deepEqual(linux.bundle.targets, ['deb', 'rpm', 'appimage']);
  assert.match(read('src-tauri/src/terminal/mod.rs'), /name = "fxins"/);
  assert.match(read('src-tauri/src/terminal/settings.rs'), /platform::data_dir\(\)\?\.join\("terminal\.json"\)/);
});

test('NSIS PATH hooks do not move PATH through bounded NSIS strings', () => {
  const hook = read('src-tauri/packaging/windows/hooks.nsh');
  assert.match(hook, /!define FXINS_HOOK_DIR "\$\{__FILEDIR__\}"/);
  assert.match(hook, /!macro NSIS_HOOK_POSTINSTALL/);
  assert.match(hook, /!macro NSIS_HOOK_PREUNINSTALL/);
  assert.match(hook, /WM_SETTINGCHANGE/);
  assert.doesNotMatch(hook, /ReadRegStr|WriteRegExpandStr|WriteRegStr|setx|runas/i);
});

test('Windows PATH lifecycle preserves values with in-memory registry keys', { skip: process.platform !== 'win32' }, () => {
  const result = execFileSync('powershell.exe', ['-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', 'scripts/test-windows-path.ps1'], { cwd: root, encoding: 'utf8', windowsHide: true, timeout: 30000 });
  assert.match(result, /Windows PATH tests passed/);
});
