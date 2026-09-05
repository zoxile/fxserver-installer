import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import vm from "node:vm";
import { createRequire } from "node:module";
import test from "node:test";

const script = await readFile(new URL("../src-tauri/resources/live-bridge/server.js", import.meta.url), "utf8");
const secret = "a".repeat(64);

function bridge(token = secret) {
  const state = { commands: [], time: 10000, listeners: {}, cacheReads: 0 };
  const context = {
    require: createRequire(import.meta.url), Buffer, Date: { now: () => state.time },
    GetCurrentResourceName: () => "fxserver_installer_bridge", LoadResourceFile: () => token,
    on: (name, fn) => { state.listeners[name] = fn; }, setInterval: () => 1,
    setTimeout: () => 2, clearTimeout: () => {}, setImmediate: (fn) => fn(),
    SetHttpHandler: (fn) => { state.handler = fn; },
    GetNumResources: () => { state.cacheReads++; return 2; },
    GetResourceByFindIndex: (i) => ["qbx_core", "chat"][i],
    GetResourceState: (name) => name === "missing" ? "missing" : "started",
    GetResourceMetadata: () => "1.0", GetNumPlayerIndices: () => 1,
    GetPlayerFromIndex: () => 7, GetPlayerName: () => "Player", GetPlayerPing: () => 25,
    GetConvar: (_, fallback) => fallback, GetConvarInt: (_, fallback) => fallback,
    ExecuteCommand: (command) => state.commands.push(command),
  };
  vm.runInNewContext(script, context);
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
  for (const options of [{ address: "192.168.1.1" }, { address: "127.0.0.1.evil" }, { headers: {} }, { headers: { authorization: "Bearer wrong" } }]) {
    assert.equal(state.request(options).status, 403);
  }
  assert.equal(bridge("").request().status, 403);
});

test("accepts only exact loopback addresses and returns bounded public metadata", () => {
  const state = bridge();
  for (const address of ["127.0.0.1", "127.0.0.1:1234", "::1", "[::1]:1234", "::ffff:127.0.0.1"]) assert.equal(state.request({ address }).status, 200);
  const result = state.request().body;
  assert.equal(result.playerCount, 1);
  assert.equal(result.resources[0].state, "started");
  assert.equal(JSON.stringify(result).includes(secret), false);
  assert.equal("identifiers" in result.players[0], false);
  assert.equal(state.cacheReads, 1);
});

test("resource commands reject injection, missing resources and self removal", () => {
  const state = bridge();
  for (const command of [{ action: "exec", resource: "server.cfg" }, { action: "ensure", resource: "chat;quit" }, { action: "stop", resource: "fxserver_installer_bridge" }, null]) {
    assert.equal(state.request({ method: "POST", path: "/resource", body: JSON.stringify(command) }).status, 400);
  }
  assert.equal(state.request({ method: "POST", path: "/resource", body: JSON.stringify({ action: "start", resource: "missing" }) }).status, 404);
  assert.equal(state.commands.length, 0);
  assert.equal(state.request({ method: "POST", path: "/resource", body: JSON.stringify({ action: "ensure", resource: "chat" }) }).body.accepted, true);
  assert.deepEqual(state.commands, ["ensure chat"]);
});

test("event history remains bounded and rate limits requests", () => {
  const state = bridge();
  for (let i = 0; i < 150; i++) state.listeners.onResourceStart(`resource-${i}`);
  assert.equal(state.request().body.events.length, 100);
  state.time -= 150;
  assert.equal(state.request().status, 429);
  assert.equal(state.request({ method: "POST", path: "/resource", body: "x".repeat(1025) }).status, 413);
});
