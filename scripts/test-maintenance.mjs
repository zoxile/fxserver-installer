import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import ts from "typescript";

const source = (path) => readFileSync(new URL(path, import.meta.url), "utf8");
const compile = (path) => ts.transpile(source(path), { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 });
const url = (text) => `data:text/javascript;base64,${Buffer.from(text).toString("base64")}`;
const { createMaintenanceRun, executeMaintenance, sameMaintenancePath } = await import(url(compile("../src/lib/modules/maintenance.ts")));

test("maintenance orders stopped-server backups before updates and never performs implicit rollback", async () => {
  const run = createMaintenanceRun(true, true), calls = [];
  const actions = Object.fromEntries(run.steps.map(({ id }) => [id, async () => { calls.push(id); return id; }]));
  await executeMaintenance(run, actions, () => {});
  assert.deepEqual(calls, ["validate", "announce", "players", "stop", "backup", "updates", "preflight", "start", "health"]);
  assert.equal(run.status, "completed");
  assert.ok(run.steps.every((step) => step.status === "passed"));
  await assert.rejects(executeMaintenance(run, actions, () => {}), /Review a new plan/);
});

test("every failing step pauses remaining work and cannot replay completed writes", async () => {
  for (const failure of createMaintenanceRun(true, true).steps.map((step) => step.id)) {
    const run = createMaintenanceRun(true, true), calls = [];
    const actions = Object.fromEntries(run.steps.map(({ id }) => [id, async () => { calls.push(id); if (id === failure) throw new Error("fixture failure"); return id; }]));
    await assert.rejects(executeMaintenance(run, actions, () => {}), /fixture failure/);
    assert.equal(calls.at(-1), failure);
    assert.equal(run.status, "paused");
    assert.equal(run.steps.find((step) => step.id === failure).status, "failed");
    assert.ok(run.finishedAt);
    await assert.rejects(executeMaintenance(run, actions, () => {}), /Review a new plan/);
  }
});

test("cancellation waits for the current operation and context changes stop the next step", async () => {
  const run = createMaintenanceRun(false, false), calls = [];
  let release;
  const actions = Object.fromEntries(run.steps.map(({ id }) => [id, async () => { calls.push(id); if (id === "backup") await new Promise((resolve) => { release = resolve; }); return id; }]));
  const pending = executeMaintenance(run, actions, () => {});
  while (!release) await new Promise((resolve) => setTimeout(resolve, 0));
  run.cancelRequested = true;
  assert.equal(run.status, "running");
  release();
  await assert.rejects(pending, /safe boundary/);
  assert.equal(run.steps.find((step) => step.id === "backup").status, "passed");
  assert.equal(calls.includes("updates"), false);
  const changed = createMaintenanceRun(false, false);
  let checks = 0;
  await assert.rejects(executeMaintenance(changed, actions, () => { if (++checks === 2) throw new Error("workspace changed"); }), /workspace changed/);
  assert.equal(changed.steps[1].status, "pending");
});

test("Windows launch paths compare consistently without accepting different servers", () => {
  assert.ok(sameMaintenancePath("C:\\FXServer\\", "c:/fxserver"));
  assert.equal(sameMaintenancePath("C:/server", "C:/server-other"), false);
});

test("exclusive maintenance blocks mutations but keeps reads and navigation usable, then releases on failure", async () => {
  const backend = url("export async function invoke(command) { return globalThis.maintenancePendingInvoke ? globalThis.maintenancePendingInvoke(command) : command; }");
  const logger = url("export function log() {} export function appendTaskIncident() {} export function invalidateDatabaseBrowserCommand() {}");
  const code = "const $state = (x) => x;\n" + compile("../src/lib/core/tasks.svelte.ts")
    .replace('"@tauri-apps/api/core"', JSON.stringify(backend))
    .replace('"./logger.svelte"', JSON.stringify(logger)).replace('"./incidents.svelte"', JSON.stringify(logger)).replace('"./databaseBrowserCache"', JSON.stringify(logger));
  const { trackExclusiveTask, trackTask, taskInvoke, taskSession, hasRunningTasks, acceptBackupProgress } = await import(url(code));
  let finish;
  const pending = trackExclusiveTask("run_maintenance_plan", "Maintenance", () => new Promise((resolve) => { finish = resolve; }));
  assert.equal(taskSession.exclusiveOwner, "run_maintenance_plan");
  acceptBackupProgress({ workspaceId: "default", scheduleId: "daily", stage: "running", timestamp: Date.now() });
  assert.equal(taskSession.items.length, 1, "Maintenance backup progress must not create a duplicate task");
  for (const command of ["start_fxserver", "stop_fxserver", "send_fxserver_command", "apply_resource_update", "execute_mariadb_query", "configure_health", "save_server_config"]) {
    await assert.rejects(taskInvoke(command), /maintenance plan/);
  }
  assert.equal(await taskInvoke("get_fxserver_status"), "get_fxserver_status");
  assert.equal(await taskInvoke("read_txdata_log"), "read_txdata_log");
  await assert.rejects(trackExclusiveTask("second", "second", async () => {}), /current tasks/);
  finish(); await pending;
  assert.equal(taskSession.exclusiveOwner, "");
  await assert.rejects(trackExclusiveTask("run_maintenance_plan", "Maintenance", async () => { throw new Error("failed"); }), /failed/);
  assert.equal(taskSession.exclusiveOwner, "");
  const running = trackTask("start_fxserver", "Start", () => new Promise((resolve) => { finish = resolve; }));
  await assert.rejects(trackExclusiveTask("run_maintenance_plan", "Maintenance", async () => {}), /current tasks/);
  finish(); await running;
  globalThis.maintenancePendingInvoke = () => new Promise((resolve) => { finish = resolve; });
  const unlabelled = taskInvoke("configure_live_bridge");
  assert.equal(taskSession.pendingMutations, 1);
  assert.equal(hasRunningTasks(), true);
  await assert.rejects(trackExclusiveTask("run_maintenance_plan", "Maintenance", async () => {}), /current tasks/);
  finish(); await unlabelled;
  assert.equal(hasRunningTasks(), false);
  assert.equal(taskSession.pendingMutations, 0);
  globalThis.maintenancePendingInvoke = async () => { throw new Error("unlabelled failure"); };
  await assert.rejects(taskInvoke("configure_live_bridge"), /unlabelled failure/);
  assert.equal(taskSession.pendingMutations, 0);
  delete globalThis.maintenancePendingInvoke;
});
