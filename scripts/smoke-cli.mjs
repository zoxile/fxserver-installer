// Executes only local, non-network commands with a disposable settings namespace.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, delimiter, dirname, resolve, join } from 'node:path';

assert.equal(process.argv.length, 4, 'Usage: node scripts/smoke-cli.mjs /path/to/fxins /path/to/fxserver-manager');
const binaries = process.argv.slice(2).map((path) => resolve(path));
const root = mkdtempSync(join(tmpdir(), 'fxins-cli-'));
const data = join(root, 'data');
const env = { ...process.env, HOME: root, USERPROFILE: root, LOCALAPPDATA: data, APPDATA: data, XDG_DATA_HOME: data, XDG_CONFIG_HOME: root };
// Child-only PATH: discovery is tested without changing this process or the user.
const pathKeys = Object.keys(env).filter((key) => key.toUpperCase() === 'PATH');
const originalPath = env[pathKeys[0]] ?? '';
for (const key of pathKeys) delete env[key];
env.PATH = [...new Set(binaries.map(dirname)), originalPath].join(delimiter);
delete env.DISPLAY;
delete env.WAYLAND_DISPLAY;
delete env.DBUS_SESSION_BUS_ADDRESS;
function run(binary, args, expected = 0) {
  const result = spawnSync(binary, args, { env, cwd: root, encoding: 'utf8', timeout: 20000, windowsHide: true });
  assert.ifError(result.error);
  assert.equal(result.status, expected, `${binary} ${args.join(' ')}: ${result.stderr}`);
  return result;
}
try {
  for (const binary of binaries) {
    assert.match(run(binary, ['--help']).stdout, /run[\s\S]*database/);
    assert.match(run(binary, ['run', '--help']).stdout, /--force/);
    assert.match(run(binary, ['tui', '--help']).stdout, /interactive terminal/);
    assert.match(run(binary, ['--version']).stdout, /^fxins /);
    assert.equal(run(basename(binary), ['--version']).stdout, run(binary, ['--version']).stdout);
    run(binary, ['--not-an-option'], 2);
    // Malformed private dispatch must exit before clap or a TUI can run.
    run(binary, ['--fxi-watch-fxserver'], 2);
  }
  const config = join(data, 'com.fxserver.installer', 'terminal.json');
  run(binaries[1], ['init']);
  const original = readFileSync(config, 'utf8');
  run(binaries[0], ['init'], 1);
  assert.equal(readFileSync(config, 'utf8'), original, 'fxins must not replace compatibility settings');
  for (const binary of binaries) {
    assert.match(run(binary, ['query', 'SELECT 1;']).stdout, /Preview only/);
    assert.equal(readFileSync(config, 'utf8'), original, 'Read-only command changed settings');
  }
  writeFileSync(config, '{broken-fixture', 'utf8');
  for (const binary of binaries) run(binary, ['query', 'SELECT 1;'], 1);
  assert.equal(readFileSync(config, 'utf8'), '{broken-fixture', 'Invalid existing settings were silently reset');
  console.log('Both CLI names, private dispatch, and unchanged shared settings passed.');
} finally {
  rmSync(root, { recursive: true, force: true });
}
