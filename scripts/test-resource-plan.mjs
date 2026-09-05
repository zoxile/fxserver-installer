import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";

const compile = (path) => ts.transpile(readFileSync(new URL(path, import.meta.url), "utf8"), { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 });
const url = (source) => `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`;
const modelUrl = url(compile("../src/lib/modules/resourcePlan.ts"));
const { addReviewedUpdate, newResourcePlan, parseResourcePreferences, resourcePreferenceKey, protectedResourcePaths, runReviewedUpdates } = await import(modelUrl);
const target = (name = "alpha", workspaceId = "default") => ({ workspaceId, txDataPath: "C:/fixture/txData", profile: "test", resourcePath: `C:/fixture/resources/${name}` });
const preference = { pinnedVersion: null, ignored: false };
const preview = (id) => ({ id, resourceName: id, repository: "https://github.com/example/fixture", branch: "main", archiveSha256: "a".repeat(64), archiveBytes: 1234, createdAt: Math.floor(Date.now() / 1000), changes: [
  { path: "config.lua", kind: "modified", canPreserve: true, preserve: true },
  { path: "local-only.txt", kind: "removed", canPreserve: true, preserve: true },
  { path: "server.lua", kind: "modified", canPreserve: true, preserve: false },
  { path: "new.lua", kind: "added", canPreserve: false, preserve: false },
] });
const queue = (plan, id, workspaceId = "default") => addReviewedUpdate(plan, target(id, workspaceId), id, preview(id), [], preference);
const applied = [];
const discarded = [];
const dependencies = {
  preference: () => preference,
  apply: async (entry, id, protectedPaths) => { applied.push({ entry, id, protectedPaths }); return { id: `snapshot-${id}` }; },
  discard: async (id) => { discarded.push(id); },
};

assert.deepEqual(protectedResourcePaths(preview("fixture"), ["server.lua", "new.lua", "../escape"]), ["config.lua", "local-only.txt", "server.lua"]);
const saved = { [resourcePreferenceKey(target())]: { ignored: true, pinnedVersion: "1.2.3" } };
assert.deepEqual({ ...parseResourcePreferences(JSON.stringify(saved)) }, saved);
assert.throws(() => parseResourcePreferences("broken JSON"));
assert.throws(() => parseResourcePreferences('{"fixture":{"ignored":"yes","pinnedVersion":null}}'));
assert.equal(resourcePreferenceKey(target()), resourcePreferenceKey({ ...target(), txDataPath: "c:\\FIXTURE\\txdata\\", profile: "TEST", resourcePath: "c:\\fixture\\resources\\ALPHA" }));
assert.notEqual(resourcePreferenceKey(target()), resourcePreferenceKey({ ...target(), profile: "other" }));

const plan = newResourcePlan();
queue(plan, "alpha"); queue(plan, "beta");
assert.throws(() => queue(plan, "alpha"), /already/);
assert.throws(() => addReviewedUpdate(plan, target("pinned"), "pinned", preview("pinned"), [], { ...preference, pinnedVersion: "1" }), /Unpin/);
assert.throws(() => addReviewedUpdate(plan, target("ignored"), "ignored", preview("ignored"), [], { ...preference, ignored: true }), /Ignore/);
assert.equal(applied.length, 0, "Reviewing must not apply any files");
await runReviewedUpdates(plan, "default", dependencies);
assert.deepEqual(applied.map((entry) => entry.id), ["alpha", "beta"]);
assert.ok(applied.every((entry) => entry.protectedPaths.includes("config.lua") && entry.protectedPaths.includes("local-only.txt")));
assert.equal(plan.status, "completed");
assert.equal(plan.revision, 2, "Every completed apply must refresh mounted inventory views");
await runReviewedUpdates(plan, "default", dependencies);
assert.equal(applied.length, 2, "Completed reviews must never reapply");

const failure = newResourcePlan();
queue(failure, "fails"); queue(failure, "later");
let count = 0;
await runReviewedUpdates(failure, "default", { ...dependencies, apply: async () => { count++; throw new Error("fixture apply failure"); } });
assert.equal(count, 1);
assert.equal(failure.status, "paused");
assert.equal(failure.entries[0].status, "failed");
assert.equal(failure.entries[1].status, "ready");
assert.match(failure.error, /fixture apply failure/);
await runReviewedUpdates(failure, "default", dependencies);
assert.equal(failure.entries[1].status, "completed");
assert.equal(failure.entries[0].status, "failed", "Continue remaining cannot retry an unreviewed failure");

for (const mode of ["pauseRequested", "stopRequested"]) {
  const paused = newResourcePlan(); queue(paused, "first"); queue(paused, "second");
  let complete;
  const running = runReviewedUpdates(paused, "default", { ...dependencies, apply: () => new Promise((resolve) => { complete = resolve; }) });
  assert.equal(paused.entries[0].status, "applying");
  paused[mode] = true;
  complete({ id: "snapshot-first" });
  await running;
  assert.equal(paused.entries[0].status, "completed");
  assert.equal(paused.entries[1].status, mode === "stopRequested" ? "cancelled" : "ready");
  assert.equal(paused.status, mode === "stopRequested" ? "stopped" : "paused");
}

for (const scenario of ["expired", "workspace", "new-pin"]) {
  const invalid = newResourcePlan(); queue(invalid, scenario, scenario === "workspace" ? "other" : "default");
  if (scenario === "expired") invalid.entries[0].preview.createdAt -= 1801;
  let calls = 0;
  await runReviewedUpdates(invalid, "default", { ...dependencies, preference: () => scenario === "new-pin" ? { ignored: false, pinnedVersion: "1" } : preference, apply: async () => { calls++; return { id: "unexpected" }; } });
  assert.equal(calls, 0); assert.equal(invalid.status, "paused");
}

// Exercise the real session module with in-memory storage and no Tauri transport.
const storage = new Map();
globalThis.localStorage = { getItem: (key) => storage.get(key) ?? null, setItem: (key, value) => storage.set(key, value) };
const taskUrl = url('export const taskSession = { workspaceId: "default", switching: false }; export const trackTask = async (_c, _l, action) => action();');
const backendUrl = url('export const applyResourceUpdate = async () => ({id:"fixture"}); export const discardResourcePreview = async () => {};');
const settingsUrl = url('export const fxserverSettings = { txDataPath: "C:/fixture/txData", profile: "test" };');
const { fxserverSettings } = await import(settingsUrl);
const sessionSource = 'const $state = (value) => value;\n' + compile("../src/lib/modules/resourcePlan.svelte.ts")
  .replace('"$lib/core/tasks.svelte"', JSON.stringify(taskUrl)).replace('"./resourceUpdates"', JSON.stringify(backendUrl)).replace('"./resourcePlan"', JSON.stringify(modelUrl))
  .replace('"$lib/features/fxserver/fxserverSettings.svelte"', JSON.stringify(settingsUrl));
const session = await import(url(sessionSource));
session.loadResourcePreferences("default"); session.loadResourcePreferences("other");
session.saveResourcePreference(target(), { ignored: false, pinnedVersion: "2.0" });
assert.equal(session.getResourcePreference(target()).pinnedVersion, "2.0");
assert.equal(session.getResourcePreference(target("alpha", "other")).pinnedVersion, null);
delete session.resourcePlanSession.preferences.default;
session.loadResourcePreferences("default");
assert.equal(session.getResourcePreference(target()).pinnedVersion, "2.0", "Pin must survive preferences reload");
session.saveResourcePreference(target(), preference);
session.queueResourceUpdate(target(), "alpha", preview("retained"), []);
assert.equal(session.getResourcePlan("default"), session.getResourcePlan("default"), "Navigation must retrieve the same queue state");
session.saveResourcePreference(target(), { ignored: true, pinnedVersion: null });
assert.equal(session.getResourcePlan("default").entries[0].status, "cancelled");
storage.set("fxserver-installer.resource-preferences.v1.broken", "invalid");
session.loadResourcePreferences("broken");
assert.throws(() => session.getResourcePreference(target("alpha", "broken")), /blocked/);
session.queueResourceUpdate(target("beta"), "beta", preview("changed-path"), []);
fxserverSettings.txDataPath = "C:/different/txData";
await assert.rejects(session.runResourcePlan("default"), /Server paths changed/);
assert.equal(session.getResourcePlan("default").entries.at(-1).status, "ready");
fxserverSettings.txDataPath = "c:/FIXTURE/txData/";
await session.runResourcePlan("default");
assert.equal(session.getResourcePlan("default").entries.at(-1).status, "completed");
console.log("Resource plan fixtures passed: review-only queue, protection, pin/ignore persistence and isolation, failure/pause/stop, expiry, workspace guard, navigation state.");
