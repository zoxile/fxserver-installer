import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";
import { createRequire } from "node:module";
import test from "node:test";

const script = await readFile(new URL("../src-tauri/resources/live-bridge/server.js", import.meta.url), "utf8");
const secret = "a".repeat(64);

function bridge(token = secret) {
  const state = { commands: [], time: 10000, listeners: {}, cacheReads: 0, deferred: false, queue: [], timers: new Map() };
  const require = createRequire(import.meta.url);
  let nextTimer = 0;
  const context = {
    require: (name) => name === "crypto" ? { ...require(name), randomBytes: (size) => {
      const bytes = require(name).randomBytes(size);
      state.instanceId = bytes.toString("hex");
      return bytes;
    } } : name === "perf_hooks" ? { performance: { now: () => state.time } } : require(name),
    Buffer, Date: { now: () => state.time + (state.clockOffset ?? 0) },
    GetCurrentResourceName: () => "fxserver_installer_bridge", LoadResourceFile: () => token,
    on: (name, fn) => { state.listeners[name] = fn; }, setInterval: () => 1,
    setTimeout: (fn) => { state.timers.set(++nextTimer, fn); return nextTimer; },
    clearTimeout: (id) => state.timers.delete(id),
    setImmediate: (fn) => state.deferred ? state.queue.push(fn) : fn(),
    SetHttpHandler: (fn) => { state.handler = fn; },
    GetNumResources: () => { state.cacheReads++; return 2; },
    GetResourceByFindIndex: (i) => ["qbx_core", "chat"][i],
    GetResourceState: (name) => {
      if (state.failNative) throw new Error("fixture native failure");
      return name === "missing" ? "missing" : "started";
    },
    GetResourceMetadata: () => "1.0", GetNumPlayerIndices: () => 1,
    GetPlayerFromIndex: () => 7, GetPlayerName: () => "Player", GetPlayerPing: () => 25,
    GetConvar: (_, fallback) => fallback, GetConvarInt: (_, fallback) => fallback,
    ExecuteCommand: (command) => state.commands.push(command),
  };
  vm.runInNewContext(script, context);
  state.flush = () => { while (state.queue.length) state.queue.shift()(); };
  state.action = (resource = "chat", extra = {}) => ({ method: "POST", path: "/resource",
    body: JSON.stringify({ action: "ensure", resource, instanceId: state.instanceId, ...extra }) });
  state.request = (options = {}) => {
    state.time += 150;
    const result = {};
    state.handler({ address: "127.0.0.1", headers: { authorization: `Bearer ${secret}` }, method: "GET", path: "/snapshot",
      setDataHandler: (fn) => fn(options.body ?? ""), setCancelHandler: () => {}, ...options,
    }, { writeHead: (status) => { result.status = status; }, send: (body) => { result.body = JSON.parse(body); } });
    return result;
  };
  return state;
}

test("rejects remote, missing, wrong and malformed credentials", () => {
  const state = bridge();
  for (const options of [{ address: "192.168.1.1" }, { address: "127.0.0.1.evil" }, { address: "127.0.0.1\n" }, { address: null }, { headers: {} }, { headers: { authorization: "Bearer wrong" } }]) {
    assert.equal(state.request(options).status, 403);
  }
  assert.equal(bridge("").request().status, 403);
});

test("accepts only exact loopback addresses and returns bounded public metadata", () => {
  const state = bridge();
  for (const address of ["127.0.0.1", "127.0.0.1:1234", "::1", "[::1]:1234", "::ffff:127.0.0.1"]) assert.equal(state.request({ address }).status, 200);
  const result = state.request().body;
  assert.equal(result.protocol, 2);
  assert.equal(result.version, "1.1.0");
  assert.equal(result.playerCount, 1);
  assert.equal(result.resources[0].state, "started");
  assert.equal(JSON.stringify(result).includes(secret), false);
  assert.equal("identifiers" in result.players[0], false);
  assert.equal(state.cacheReads, 1);
});

test("resource commands reject injection, missing resources and self removal", () => {
  const state = bridge();
  for (const command of [{ action: "exec", resource: "server.cfg" }, { action: "ensure", resource: "chat;quit" }, { action: "ensure", resource: "chat\n" }, { action: "stop", resource: "fxserver_installer_bridge" }, { action: "stop", resource: "FXSERVER_INSTALLER_BRIDGE" }, null]) {
    assert.equal(state.request({ method: "POST", path: "/resource", body: JSON.stringify(command) }).status, 400);
  }
  assert.equal(state.request(state.action("missing")).status, 404);
  assert.equal(state.commands.length, 0);
  assert.equal(state.request(state.action()).body.accepted, true);
  assert.deepEqual(state.commands, ["ensure chat"]);
});

test("actions are bound to the observed instance and contain native errors", () => {
  const state = bridge();
  for (const instanceId of [undefined, "old-instance", null]) {
    assert.equal(state.request(state.action("chat", { instanceId })).status, 409);
  }
  state.failNative = true;
  assert.equal(state.request(state.action()).status, 500);
  assert.deepEqual(state.commands, []);
  assert.equal(state.timers.size, 0);
});

test("cancelled, timed out and scheduler-delayed requests never execute", () => {
  for (const reason of ["cancel", "timeout", "delayed-body", "delayed-dispatch"]) {
    const state = bridge();
    state.deferred = true;
    let deliver;
    let cancel;
    const request = state.action();
    const result = state.request({ ...request, setDataHandler: (fn) => { deliver = fn; }, setCancelHandler: (fn) => { cancel = fn; } });
    if (reason !== "delayed-body") deliver(request.body);
    if (reason === "cancel") cancel();
    else if (reason === "timeout") [...state.timers.values()].forEach((fn) => fn());
    else state.time += 2000;
    if (reason === "delayed-body") deliver(request.body);
    state.flush();
    assert.deepEqual(state.commands, [], reason);
    assert.equal(result.status, reason === "cancel" ? undefined : 408);
    assert.equal(state.timers.size, 0);
  }
});

test("duplicate body delivery cannot queue additional resource actions", () => {
  const state = bridge();
  state.deferred = true;
  const request = state.action();
  const result = state.request({ ...request, setDataHandler: (fn) => { for (let i = 0; i < 100; i++) fn(request.body); } });
  assert.equal(state.queue.length, 1);
  state.flush();
  assert.equal(result.status, 200);
  assert.deepEqual(state.commands, ["ensure chat"]);
  assert.equal(state.timers.size, 0);
});

test("wall-clock changes cannot extend action deadlines or throttle valid requests", () => {
  const state = bridge();
  state.deferred = true;
  const result = state.request(state.action());
  state.clockOffset = -100000;
  state.time += 2000;
  state.flush();
  assert.equal(result.status, 408);
  assert.deepEqual(state.commands, []);
  state.deferred = false;
  assert.equal(state.request().status, 200);
});

test("event history remains bounded and rate limits requests", () => {
  const state = bridge();
  for (let i = 0; i < 150; i++) state.listeners.onResourceStart(`resource-${i}`);
  assert.equal(state.request().body.events.length, 100);
  state.time -= 150;
  assert.equal(state.request().status, 429);
  assert.equal(state.request({ method: "POST", path: "/resource", body: "x".repeat(1025) }).status, 413);
});
