import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";
import { compileModule } from "svelte/compiler";

const url = (code) => `data:text/javascript;base64,${Buffer.from(code).toString("base64")}`;
function build(path, dependencies) {
  let code = ts.transpile(readFileSync(new URL(`../src/lib/${path}`, import.meta.url), "utf8"), { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 });
  for (const [name, replacement] of Object.entries(dependencies)) code = code.replaceAll(JSON.stringify(name), JSON.stringify(replacement));
  if (path.endsWith(".svelte.ts")) code = compileModule(code, { filename: path, generate: "client" }).js.code;
  code = code.replace(/(from\s+|import\s+)["'](svelte\/[^"']+)["']/g, (_, prefix, name) => `${prefix}${JSON.stringify(import.meta.resolve(name))}`);
  return url(code);
}
const storage = new Map();
globalThis.localStorage = { getItem: (key) => storage.get(key) ?? null, setItem: (key, value) => storage.set(key, value), removeItem: (key) => storage.delete(key) };
globalThis.window = new EventTarget();
window.__TAURI_INTERNALS__ = {};
let resolveProfiles;
const requests = [];
globalThis.enhancedTest = { profiles: (path, options) => { requests.push({ path, options }); return new Promise((resolve) => { resolveProfiles = resolve; }); } };
const logger = url("export const log = () => {};");
const settings = await import(build("features/fxserver/fxserverSettings.svelte.ts", {
  "$lib/core/logger.svelte": logger,
  "$lib/core/workspaceSettings": url("export const publicEnvironment = (value) => ({...value});"),
  "$lib/modules/fxserver": url("export const listTxDataProfiles = (...args) => globalThis.enhancedTest.profiles(...args);"),
}));
const nested = "C:/fixture/artifacts/custom-data";
storage.set("fxserver.manage.env", JSON.stringify({ TXHOST_DATA_PATH: nested }));
storage.set("fxserver.manage.serverProfile", "saved-profile");
settings.loadFxserverSettings();
let pending = settings.refreshTxDataProfiles("C:/fixture/artifacts");
resolveProfiles({ dataPath: "C:/unwanted-migration", profiles: [], hasRootLogs: false, exists: false });
await pending;
assert.equal(settings.fxserverSettings.txDataPath, nested);
assert.equal(settings.fxserverSettings.profile, "saved-profile", "Absent profiles must not erase a workspace selection");
assert.equal(requests.at(-1).options.discover, false);

pending = settings.refreshTxDataProfiles("C:/fixture/artifacts", true);
settings.resetTxDataProfiles(); // Same-path workspace switch.
settings.setServerProfile("next-workspace");
resolveProfiles({ dataPath: "C:/wrong", profiles: ["wrong"], hasRootLogs: true, selectedProfile: "wrong" });
await pending;
assert.equal(settings.fxserverSettings.txDataPath, nested);
assert.equal(settings.fxserverSettings.profile, "next-workspace");
assert.equal(settings.fxserverSettings.profiles.length, 0);

settings.setTxDataPath("");
pending = settings.refreshTxDataProfiles("C:/fresh/artifacts");
resolveProfiles({ dataPath: "C:/fresh/txData", profiles: [], hasRootLogs: false, exists: false });
await pending;
assert.equal(settings.fxserverSettings.txDataPath, "C:/fresh/txData");
assert.equal(JSON.parse(storage.get("fxserver.manage.env")).TXHOST_DATA_PATH, "C:/fresh/txData");

settings.setTxDataPath("C:/chosen/txData/default");
pending = settings.refreshTxDataProfiles("C:/fresh/artifacts", true);
resolveProfiles({ dataPath: "C:/chosen/txData", profiles: ["default"], hasRootLogs: false, selectedProfile: "default" });
await pending;
assert.equal(settings.fxserverSettings.txDataPath, "C:/chosen/txData");
assert.equal(settings.fxserverSettings.profile, "default");

pending = settings.refreshTxDataProfiles("C:/fresh/artifacts", true);
settings.setServerProfile("user-picked-during-scan");
resolveProfiles({ dataPath: "C:/chosen/txData", profiles: ["default"], hasRootLogs: false, selectedProfile: "default" });
await pending;
assert.equal(settings.fxserverSettings.profile, "user-picked-during-scan");

const tasks = [];
let finishInstall;
globalThis.enhancedTest.track = async (command, label, action) => {
  const task = { command, label, status: "running" }; tasks.push(task);
  try { const result = await action(); task.status = "completed"; return result; }
  catch (error) { task.status = "failed"; throw error; }
};
globalThis.enhancedTest.invoke = () => new Promise((resolve) => { finishInstall = resolve; });
const artifact = await import(build("modules/artifact.ts", {
  "$lib/core/logger.svelte": logger,
  "$lib/core/paths.svelte": url("export const getInstallPath = () => 'C:/fixture/artifacts';"),
  "$lib/core/tasks.svelte": url("export const taskInvoke = () => {}; export const trackTask = (...args) => globalThis.enhancedTest.track(...args);"),
  "@tauri-apps/api/core": url("export const invoke = (...args) => globalThis.enhancedTest.invoke(...args);"),
}));
const installed = { installed: true, edition: "enhanced", version: "00000000-0000-0000-0000-000000000001" };
const health = artifact.getArtifactHealthStatus({ recommendedArtifact: "10000", brokenArtifacts: [{ artifact: "0-99999", reason: "legacy only" }] }, installed);
assert.equal(health.label, "Enhanced installed");
assert.equal(health.recommendedVersion, null);
const install = artifact.installEnhancedArtifact("https://fixture.invalid", "C:/fixture");
assert.equal(tasks[0].command, "install_enhanced_artifact");
assert.equal(tasks[0].status, "running");
finishInstall({ version: "fixture" });
await install;
assert.equal(tasks[0].status, "completed");
console.log("Enhanced task tracking, edition health, and non-migrating txData state tests passed.");
