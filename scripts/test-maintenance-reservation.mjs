import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import ts from "typescript";

const url = (source) => `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`;
let sequence = 0;
async function fixture() {
  const key = `maintenanceReservation${++sequence}`;
  const state = globalThis[key] = { now: 1_000, timers: new Map(), sequence: 0, calls: [], failure: false, pending: null };
  const backend = url(`export async function invoke(command, args) {
    const state = globalThis[${JSON.stringify(key)}]; state.calls.push({command, args});
    if (command === 'renew_maintenance_backup_reservation') {
      if (state.pending) await state.pending;
      if (state.failure) throw new Error('lost lease');
    }
    return {token: 'fixture-reservation', expiresAt: state.now + 120000};
  }`);
  let source = ts.transpile(readFileSync(new URL("../src/lib/modules/maintenanceReservation.ts", import.meta.url), "utf8"), { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 });
  source = source.replace('"@tauri-apps/api/core"', JSON.stringify(backend)).replaceAll("Date.now()", "state.now");
  const module = await import(url(`const state = globalThis[${JSON.stringify(key)}];
    const setTimeout = (callback, delay) => { const id = ++state.sequence; state.timers.set(id, {callback, delay}); return id; };
    const clearTimeout = (id) => state.timers.delete(id);
    ${source}`));
  const reservation = await module.reserveMaintenanceBackups("default", ["daily"]);
  return { state, reservation, fire() { const [id, timer] = state.timers.entries().next().value; state.timers.delete(id); state.now += timer.delay; timer.callback(); } };
}
const settle = () => new Promise((resolve) => setImmediate(resolve));

test("reservations renew serially and release waits for in-flight renewal before cleanup", async () => {
  const { state, reservation, fire } = await fixture();
  assert.equal(reservation.token, "fixture-reservation");
  assert.doesNotThrow(reservation.assertActive);
  let complete;
  state.pending = new Promise((resolve) => { complete = resolve; });
  fire();
  assert.equal(state.timers.size, 0, "Renewal must not overlap another request");
  const closing = reservation.release();
  assert.equal(state.calls.some((call) => call.command === "release_maintenance_backup_reservation"), false);
  complete(); await closing;
  assert.equal(state.timers.size, 0);
  assert.equal(state.calls.at(-1).command, "release_maintenance_backup_reservation");
  assert.deepEqual(state.calls.at(-1).args, { workspaceId: "default", token: "fixture-reservation" });
  assert.throws(reservation.assertActive, /reservation/);
});

test("renewal failure and expiry both block further work and cannot resurrect a reservation", async () => {
  for (const failure of ["renewal", "expiry"]) {
    const { state, reservation, fire } = await fixture();
    if (failure === "expiry") state.now += 120001;
    else { state.failure = true; fire(); await settle(); assert.equal(state.timers.size, 0); }
    assert.throws(reservation.assertActive, /reservation/);
    await reservation.release();
    assert.equal(state.timers.size, 0);
  }
});

test("successful renewal extends validity and schedules just one next renewal", async () => {
  const { state, reservation, fire } = await fixture();
  for (let count = 0; count < 6; count++) {
    fire(); await settle();
    assert.equal(state.timers.size, 1);
    assert.doesNotThrow(reservation.assertActive);
  }
  await reservation.release();
  assert.equal(state.timers.size, 0);
});
