import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import ts from "typescript";

const compile = (path) => ts.transpile(readFileSync(new URL(path, import.meta.url), "utf8"), { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 });
const url = (code) => `data:text/javascript;base64,${Buffer.from(code).toString("base64")}`;
const modelUrl = url(compile("../src/lib/modules/maintenance.ts"));
const planUrl = url(compile("../src/lib/modules/resourcePlan.ts"));
const { newResourcePlan, addReviewedUpdate } = await import(planUrl);
let sequence = 0;

async function fixture() {
  const key = `maintenanceFixture${++sequence}`;
  const credentials = { host: "localhost", port: 3306, username: "fixture", password: "private-fixture", database: "qbox" };
  const schedule = { id: "daily", workspaceId: "default", name: "Daily", database: "qbox", outputDir: "C:/backups", intervalMinutes: 60, retainCount: 5 };
  const state = globalThis[key] = {
    databaseSession: { credentials, validated: { credentials: { ...credentials }, revision: 0 }, revision: 0 },
    workspaceSession: { activeId: "default", revision: 0 }, taskSession: { workspaceId: "default", switching: false, exclusiveOwner: "" },
    fxserverSettings: { txDataPath: "C:/txData", profile: "default" }, artifactPath: "C:/artifacts", environment: { TXHOST_DATA_PATH: "C:/txData" },
    schedule, plan: newResourcePlan(), calls: [], running: true, pid: 101, fail: "", clock: Date.now(),
  };
  addReviewedUpdate(state.plan, { workspaceId: "default", txDataPath: "C:/txData", profile: "default", resourcePath: "C:/resources/example" }, "example",
    { id: "review", resourceName: "example", createdAt: Date.now() / 1000, changes: [], branch: "main", repository: "https://github.com/example/repo" }, [], { pinnedVersion: null, ignored: false });
  state.invoke = async (command, args) => {
    state.calls.push({ command, args });
    if (command === state.fail) throw new Error("Fixture failure");
    switch (command) {
      case "get_fxserver_status": await state.onStatus?.(); return { running: state.running, pid: state.pid, artifactPath: state.artifactPath };
      case "get_backup_manager": return { schedules: [{ config: state.schedule }], snapshots: [], busy: false };
      case "get_live_bridge_status": return { workspaceId: "default", enabled: false, connected: false, snapshot: null };
      case "prepare_fxserver_maintenance_launch": return "fixture-launch-ticket";
      case "reserve_maintenance_backups": state.onReserve?.(); return { token: "fixture-backup-reservation", expiresAt: state.clock + 120_000 };
      case "renew_maintenance_backup_reservation": return { token: "fixture-backup-reservation", expiresAt: state.clock + 120_000 };
      case "release_maintenance_backup_reservation": return;
      case "validate_fxserver_maintenance_launch": case "discard_fxserver_maintenance_launch": case "validate_mariadb_credentials": case "discard_resource_preview": return;
      case "get_fxserver_rcon_password": return "fixture-rcon";
      case "verify_managed_rcon_endpoint": case "send_fxserver_command": return;
      case "stop_fxserver_for_maintenance": state.running = false; return;
      case "run_scheduled_backup_now": assert.equal(args.reservationToken, "fixture-backup-reservation"); state.onBackup?.(); return { id: "backup", database: "qbox", workspaceId: "default" };
      case "apply_resource_update": state.onUpdate?.(); return { id: "resource-snapshot" };
      case "run_fxserver_preflight": return { blocking: state.blocking === true, errorCount: state.blocking ? 1 : 0, warningCount: 0 };
      case "start_fxserver_after_maintenance": state.running = true; state.pid = 202; return { pid: state.pid, artifactPath: state.artifactPath };
      default: throw new Error(`Unexpected command ${command}`);
    }
  };
  const mocks = url(`const state = globalThis[${JSON.stringify(key)}];
    export const databaseSession = state.databaseSession, workspaceSession = state.workspaceSession, taskSession = state.taskSession, fxserverSettings = state.fxserverSettings;
    export const invoke = (...args) => state.invoke(...args);
    export const isDatabaseSessionValidated = () => !!state.databaseSession.validated;
    export const getInstallPath = () => state.artifactPath, readSavedEnvironment = () => state.environment;
    export const txHostFields = [{key: 'TXHOST_DATA_PATH'}, {key: 'TXHOST_GAME_NAME'}];
    export const log = () => {}, loadResourcePreferences = () => {};
    export const getResourcePlan = () => state.plan, getResourcePreference = () => ({pinnedVersion:null,ignored:false});
    export async function trackExclusiveTask(command, _label, action) { state.taskSession.exclusiveOwner = command; try { return await action(); } finally { state.taskSession.exclusiveOwner = ''; } }
  `);
  const reservationUrl = url(compile("../src/lib/modules/maintenanceReservation.ts").replace('"@tauri-apps/api/core"', JSON.stringify(mocks)));
  let code = compile("../src/lib/modules/maintenance.svelte.ts").replace('"./maintenanceReservation"', JSON.stringify(reservationUrl))
    .replaceAll('"./maintenance"', JSON.stringify(modelUrl)).replaceAll('"./resourcePlan"', JSON.stringify(planUrl))
    .replace(/"(?:@tauri-apps\/api\/core|\$lib\/[^"\n]+|\.\/resourcePlan\.svelte)"/g, JSON.stringify(mocks));
  code = code.replace(/const delay = .*?;/, `const delay = async (ms) => { globalThis[${JSON.stringify(key)}].clock += Math.max(ms, 15000); };`);
  code = code.replaceAll("Date.now()", `globalThis[${JSON.stringify(key)}].clock`);
  const module = await import(url(`const $state = (value) => value;\n${code}`));
  return { state, module, async review() { return module.prepareMaintenance({ scheduleIds: ["daily"], announce: false, waitForPlayers: false }); } };
}

test("reviewed session completes exact queued updates with stopped-server backups and startup checks", async () => {
  const { state, module, review } = await fixture();
  const prepared = await review();
  assert.deepEqual(prepared.databaseTarget, { host: "localhost", port: 3306, username: "fixture" });
  assert.equal(JSON.stringify(prepared).includes("private-fixture"), false);
  await module.runMaintenance(prepared);
  assert.equal(module.maintenanceSession.run.status, "completed");
  const writes = state.calls.filter(({ command }) => ["stop_fxserver_for_maintenance", "run_scheduled_backup_now", "apply_resource_update", "start_fxserver_after_maintenance"].includes(command));
  assert.deepEqual(writes.map(({ command }) => command), ["stop_fxserver_for_maintenance", "run_scheduled_backup_now", "apply_resource_update", "start_fxserver_after_maintenance"]);
  assert.equal(state.plan.entries[0].snapshotId, "resource-snapshot");
  assert.equal(module.maintenanceSession.snapshots.length, 1);
  assert.equal(state.taskSession.exclusiveOwner, "");
  assert.equal(state.calls.filter((call) => call.command === "release_maintenance_backup_reservation").length, 1);
  await assert.rejects(module.runMaintenance(prepared), /Review/);
});

test("changes to identity, authentication, settings, queue or schedule invalidate review before shutdown", async () => {
  for (const change of ["workspace", "profile", "path", "environment", "authentication", "pid", "queue", "schedule", "expired"]) {
    const { state, module, review } = await fixture();
    const prepared = await review();
    if (change === "workspace") state.workspaceSession.activeId = "other";
    if (change === "profile") state.fxserverSettings.profile = "other";
    if (change === "path") state.artifactPath = "C:/other";
    if (change === "environment") state.environment.TXHOST_GAME_NAME = "redm";
    if (change === "authentication") state.databaseSession.validated = { ...state.databaseSession.validated };
    if (change === "pid") state.pid++;
    if (change === "queue") state.plan.entries[0].protectedPaths.push("config.lua");
    if (change === "schedule") state.schedule.outputDir = "C:/other-backups";
    if (change === "expired") state.clock += 301_000;
    await assert.rejects(module.runMaintenance(prepared), undefined, change);
    assert.equal(state.calls.some(({ command }) => command === "stop_fxserver_for_maintenance"), false, change);
  }
});

test("backup, apply and preflight failures never restart the server or repeat completed writes", async () => {
  for (const failure of ["run_scheduled_backup_now", "apply_resource_update", "preflight"]) {
    const { state, module, review } = await fixture();
    const prepared = await review();
    if (failure === "preflight") state.blocking = true;
    else state.fail = failure;
    await assert.rejects(module.runMaintenance(prepared));
    assert.equal(module.maintenanceSession.run.status, "paused");
    assert.equal(state.running, false);
    assert.equal(state.calls.some(({ command }) => command === "start_fxserver_after_maintenance"), false);
    assert.equal(state.taskSession.exclusiveOwner, "");
    if (failure !== "run_scheduled_backup_now") assert.equal(module.maintenanceSession.snapshots.length, 1);
  }
});

test("cancelling during an operation keeps completed backup but prevents the next update", async () => {
  const { state, module, review } = await fixture();
  const prepared = await review();
  state.onBackup = () => { module.maintenanceSession.run.cancelRequested = true; };
  await assert.rejects(module.runMaintenance(prepared), /safe boundary/);
  assert.equal(module.maintenanceSession.snapshots.length, 1);
  assert.equal(state.calls.some(({ command }) => command === "apply_resource_update"), false);
});

test("announcements require verified managed RCON ownership before sending or stopping", async () => {
  for (const fail of [false, true]) {
    const { state, module } = await fixture();
    const prepared = await module.prepareMaintenance({ scheduleIds: ["daily"], announce: true, waitForPlayers: false });
    if (fail) state.fail = "verify_managed_rcon_endpoint";
    if (fail) await assert.rejects(module.runMaintenance(prepared));
    else await module.runMaintenance(prepared);
    const commands = state.calls.map(({ command }) => command);
    assert.ok(commands.includes("verify_managed_rcon_endpoint"));
    assert.equal(commands.includes("send_fxserver_command"), !fail);
    assert.equal(commands.includes("stop_fxserver_for_maintenance"), !fail);
    if (!fail) assert.ok(commands.indexOf("verify_managed_rcon_endpoint") < commands.indexOf("send_fxserver_command"));
  }
});

test("preparation cannot mix profiles, overlap, or publish after invalidation", async () => {
  for (const reason of ["profile", "overlap", "cancel"]) {
    const { state, module, review } = await fixture();
    let release;
    state.onStatus = () => new Promise((resolve) => { release = resolve; });
    const pending = review();
    if (reason === "profile") state.fxserverSettings.profile = "other";
    if (reason === "overlap") await assert.rejects(review(), /already being prepared/);
    if (reason === "cancel") module.invalidateMaintenanceReview();
    release();
    if (reason === "overlap") {
      const prepared = await pending;
      assert.ok(prepared.id);
      assert.equal(prepared.profile, prepared.launch.serverProfile);
    } else {
      await assert.rejects(pending, /changed|cancelled/);
      assert.equal(module.maintenanceSession.review, null);
    }
  }
});

test("cancellation during lifecycle status reads prevents dispatch", async () => {
  for (const step of ["stop", "start"]) {
    const { state, module, review } = await fixture();
    const prepared = await review();
    state.onStatus = () => {
      if (module.maintenanceSession.run?.steps.find((item) => item.id === step)?.status === "running") {
        module.maintenanceSession.run.cancelRequested = true;
      }
    };
    await assert.rejects(module.runMaintenance(prepared), /safe boundary/);
    const command = step === "stop" ? "stop_fxserver_for_maintenance" : "start_fxserver_after_maintenance";
    assert.equal(state.calls.some((call) => call.command === command), false);
  }
});

test("backup admission and settings races fail before shutdown and release acquired reservations", async () => {
  for (const reason of ["busy", "changed"]) {
    const { state, module, review } = await fixture();
    const prepared = await review();
    if (reason === "busy") state.fail = "reserve_maintenance_backups";
    else state.onReserve = () => { state.schedule.database = "different"; };
    await assert.rejects(module.runMaintenance(prepared));
    assert.equal(state.calls.some(({ command }) => command === "stop_fxserver_for_maintenance"), false);
    assert.equal(state.calls.some(({ command }) => command === "release_maintenance_backup_reservation"), reason === "changed");
  }
});

test("cleanup failures are visible and cannot report successful maintenance", async () => {
  const { state, module, review } = await fixture();
  const prepared = await review();
  state.fail = "release_maintenance_backup_reservation";
  await assert.rejects(module.runMaintenance(prepared), /release was not confirmed/);
  assert.equal(module.maintenanceSession.run.status, "paused");
  assert.equal(state.taskSession.exclusiveOwner, "");
  assert.equal(state.calls.some(({ command }) => command === "discard_fxserver_maintenance_launch"), true);
});
