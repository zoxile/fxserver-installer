import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import ts from "typescript";
import { compileModule } from "svelte/compiler";

const url = (code) => `data:text/javascript;base64,${Buffer.from(code).toString("base64")}`;
const fixture = globalThis.updateFixture = { calls: [], listener: null, exclusive: false, blocked: false };
const transport = url("export const invoke = (...args) => globalThis.updateFixture.invoke(...args);");
const events = url("export const listen = async (name, callback) => { globalThis.updateFixture.listener = callback; return () => {}; };");
const tasks = url("export const trackTask = (command, label, action) => action(); export const trackExclusiveTask = async (command, label, action) => { if (globalThis.updateFixture.blocked) throw new Error('Background tasks running'); globalThis.updateFixture.exclusive = true; try { return await action(); } finally { globalThis.updateFixture.exclusive = false; } };");
const pkg = url('export default { version: "0.6.0" };');
let code = ts.transpile(readFileSync(new URL("../src/lib/core/appUpdate.svelte.ts", import.meta.url), "utf8"), { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 })
  .replace('"@tauri-apps/api/core"', JSON.stringify(transport))
  .replace('"@tauri-apps/api/event"', JSON.stringify(events))
  .replace('"./tasks.svelte"', JSON.stringify(tasks))
  .replace('"../../../package.json"', JSON.stringify(pkg));
code = compileModule(code, { filename: "appUpdate.svelte.ts", generate: "client" }).js.code.replace(/(from\s+)["'](svelte\/[^"']+)["']/g, (_, prefix, name) => `${prefix}${JSON.stringify(import.meta.resolve(name))}`);
globalThis.window = { __TAURI_INTERNALS__: {} };
const session = await import(url(code));
const status = (phase, fields = {}) => ({ phase, currentVersion: "0.6.0", availableVersion: "0.6.1", releaseUrl: null, downloadedBytes: 0, totalBytes: null, message: null, supported: true, ...fields });
fixture.invoke = async (command) => { fixture.calls.push(command); return status(command === "check_app_update" ? "available" : "idle"); };

test("home remounts share one listener and initial release check", async () => {
  await Promise.all([session.initializeAppUpdate(), session.initializeAppUpdate()]);
  assert.deepEqual(fixture.calls, ["get_app_update_status", "check_app_update"]);
  assert.equal(session.appUpdate.status.phase, "available");
  assert.equal(session.appUpdate.loading, false);
});

test("duplicate clicks do not overlap downloads or bypass the exclusive install guard", async () => {
  fixture.calls.length = 0;
  let finish;
  fixture.invoke = async (command) => {
    fixture.calls.push(command);
    if (command === "download_app_update") return new Promise((resolve) => { finish = resolve; });
    assert.equal(fixture.exclusive, true);
    return status("installing");
  };
  const pending = session.updateAndRestart();
  await session.updateAndRestart();
  await session.checkForAppUpdate();
  fixture.listener({ payload: status("downloading", { downloadedBytes: 1024, totalBytes: 4096 }) });
  assert.equal(session.appUpdate.status.downloadedBytes, 1024);
  assert.deepEqual(fixture.calls, ["download_app_update"]);
  finish(status("ready"));
  await pending;
  assert.deepEqual(fixture.calls, ["download_app_update", "install_app_update"]);
  assert.equal(session.appUpdate.working, false);
});

test("a busy application keeps the verified download for a later explicit install", async () => {
  fixture.calls.length = 0;
  fixture.blocked = true;
  session.appUpdate.status = status("ready");
  await session.updateAndRestart();
  assert.match(session.appUpdate.error, /Background tasks/);
  assert.deepEqual(fixture.calls, []);
  assert.equal(session.appUpdate.status.phase, "ready");
  fixture.blocked = false;
  await session.updateAndRestart();
  assert.deepEqual(fixture.calls, ["install_app_update"]);
});

test("download failure cannot advance to installation and development builds do not update", async () => {
  fixture.calls.length = 0;
  session.appUpdate.status = status("available");
  fixture.invoke = async (command) => { fixture.calls.push(command); throw new Error("Signature rejected"); };
  await session.updateAndRestart();
  assert.deepEqual(fixture.calls, ["download_app_update"]);
  assert.match(session.appUpdate.error, /Signature rejected/);
  session.appUpdate.status = status("disabled", { supported: false });
  await session.updateAndRestart();
  await session.checkForAppUpdate();
  assert.deepEqual(fixture.calls, ["download_app_update"]);
});
