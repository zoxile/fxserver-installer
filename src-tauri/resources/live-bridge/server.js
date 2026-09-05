"use strict";

const { randomBytes, timingSafeEqual } = require("crypto");
const { performance } = require("perf_hooks");
const resourceName = GetCurrentResourceName();
const token = (LoadResourceFile(resourceName, "bridge-token.txt") || "").trim();
const instanceId = randomBytes(16).toString("hex");
const started = performance.now();
const events = [];
let sequence = 0;
let cachedSnapshot;
let cacheTime = -Infinity;
let lastRequest = -Infinity;
let lastTick = performance.now();
let schedulerDelayMs = 0;

function record(kind, name) {
  events.push({ id: ++sequence, timestamp: Date.now(), kind, resource: String(name).slice(0, 128) });
  if (events.length > 100) events.shift();
  cacheTime = -Infinity;
}

on("onResourceStart", (name) => record("resource-started", name));
on("onResourceStop", (name) => record("resource-stopped", name));
setInterval(() => {
  const now = performance.now();
  schedulerDelayMs = Math.max(0, now - lastTick - 1000);
  lastTick = now;
}, 1000);

function isLoopback(address) {
  if (typeof address !== "string" || /[\r\n]/.test(address)) return false;
  return /^(127\.0\.0\.1|::1|::ffff:127\.0\.0\.1)$/.test(address) ||
    /^127\.0\.0\.1:\d+$/.test(address) ||
    /^\[(::1|::ffff:127\.0\.0\.1)\]:\d+$/.test(address);
}

function authorized(request) {
  if (!isLoopback(request.address) || !/^[a-f0-9]{64}$/.test(token)) return false;
  const header = request.headers?.authorization || request.headers?.Authorization || "";
  if (typeof header !== "string" || header.length !== 71) return false;
  const expected = Buffer.from(`Bearer ${token}`);
  const received = Buffer.from(String(header));
  return expected.length === received.length && timingSafeEqual(expected, received);
}

function send(response, status, value) {
  response.writeHead(status, { "Content-Type": "application/json", "Cache-Control": "no-store" });
  response.send(JSON.stringify(value));
}

function snapshot() {
  const now = Date.now();
  if (cachedSnapshot && performance.now() - cacheTime < 1000) return cachedSnapshot;
  const resources = [];
  const resourceCount = GetNumResources();
  for (let index = 0; index < Math.min(resourceCount, 5000); index++) {
    const name = GetResourceByFindIndex(index);
    if (!name) continue;
    resources.push({ name: String(name).slice(0, 128), state: GetResourceState(name), version: (GetResourceMetadata(name, "version", 0) || "").slice(0, 80) });
  }
  const playerCount = GetNumPlayerIndices();
  const players = [];
  for (let index = 0; index < Math.min(playerCount, 512); index++) {
    const id = String(GetPlayerFromIndex(index));
    players.push({ id, name: String(GetPlayerName(id) || "Connecting").slice(0, 128), ping: GetPlayerPing(id) });
  }
  cachedSnapshot = {
    protocol: 2, version: "1.1.0", instanceId, timestamp: now,
    uptimeSeconds: Math.floor((performance.now() - started) / 1000), schedulerDelayMs,
    hostname: GetConvar("sv_hostname", "FXServer").slice(0, 256),
    gameBuild: GetConvar("sv_enforceGameBuild", "default").slice(0, 64),
    onesync: GetConvar("onesync", "off").slice(0, 32),
    maxPlayers: GetConvarInt("sv_maxclients", 48), playerCount, resourceCount,
    resources, players, events: events.slice(),
  };
  cacheTime = performance.now();
  return cachedSnapshot;
}

SetHttpHandler((request, response) => {
  if (!authorized(request)) return send(response, 403, { error: "Local bridge authentication required." });
  if (performance.now() - lastRequest < 100) return send(response, 429, { error: "Retry shortly." });
  lastRequest = performance.now();
  if (request.method === "GET" && request.path === "/snapshot") {
    return setImmediate(() => {
      try { send(response, 200, snapshot()); }
      catch { send(response, 500, { error: "Could not collect server status." }); }
    });
  }
  if (request.method !== "POST" || request.path !== "/resource") return send(response, 404, { error: "Not found." });
  let finished = false;
  let bodyReceived = false;
  const deadline = performance.now() + 2000;
  const reply = (status, data) => {
    if (!finished) {
      finished = true;
      clearTimeout(timeout);
      send(response, status, data);
    }
  };
  const expired = () => {
    if (performance.now() >= deadline) reply(408, { error: "Request timed out." });
    return finished;
  };
  const timeout = setTimeout(() => reply(408, { error: "Request timed out." }), 2000);
  request.setCancelHandler(() => { finished = true; clearTimeout(timeout); });
  request.setDataHandler((body) => {
    if (bodyReceived || expired()) return;
    bodyReceived = true;
    if (typeof body !== "string" || Buffer.byteLength(body) > 1024) return reply(413, { error: "Request too large." });
    let command;
    try { command = JSON.parse(body); }
    catch { return reply(400, { error: "Invalid JSON." }); }
    if (!command || !["start", "stop", "restart", "ensure"].includes(command.action) ||
        typeof command.resource !== "string" || command.resource.length < 1 || command.resource.length > 96 ||
        /[^A-Za-z0-9_.-]/.test(command.resource) || command.resource.toLowerCase() === resourceName.toLowerCase()) {
      return reply(400, { error: "Unsupported resource action." });
    }
    if (command.instanceId !== instanceId) return reply(409, { error: "Server instance changed. Refresh bridge status." });
    setImmediate(() => {
      if (expired()) return;
      try {
        if (GetResourceState(command.resource) === "missing") return reply(404, { error: "Resource not found." });
        ExecuteCommand(`${command.action} ${command.resource}`);
        record("resource-command", command.resource);
        reply(200, { accepted: true, resource: command.resource, state: GetResourceState(command.resource) });
      } catch { reply(500, { error: "Resource action failed. Check server logs and bridge ACE permissions." }); }
    });
  });
});
