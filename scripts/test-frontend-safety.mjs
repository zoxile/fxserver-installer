import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";
import { compileModule } from "svelte/compiler";

const read = (path) => readFileSync(new URL(`../${path}`, import.meta.url), "utf8");
const dataUrl = (code) => `data:text/javascript;base64,${Buffer.from(code).toString("base64")}`;
const transpile = (code) => ts.transpile(code, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS });
function loadLogic(path) {
  const context = vm.createContext({ exports: {} });
  vm.runInContext(transpile(read(path)), context);
  return context.exports;
}
function component(path, names, bindings) {
  const source = read(path).match(/<script lang="ts">([\s\S]*?)<\/script>/)[1];
  const ast = ts.createSourceFile(path, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const functions = ast.statements.filter((node) => ts.isFunctionDeclaration(node) && names.includes(node.name?.text));
  assert.equal(functions.length, names.length);
  const context = vm.createContext({ DOMException, ...bindings });
  vm.runInContext(transpile(functions.map((node) => node.getText(ast)).join("\n")), context);
  return context;
}
const deferred = () => {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
};

// Use the real task registry; replace only native I/O and persistent logging.
let taskCode = ts.transpile(read("src/lib/core/tasks.svelte.ts"), { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 });
for (const [name, code] of Object.entries({
  "@tauri-apps/api/core": "export const invoke = () => { throw new Error('Native I/O is forbidden in these fixtures'); };",
  "./logger.svelte": "export const log = () => {};",
  "./incidents.svelte": "export const appendTaskIncident = () => {};",
})) taskCode = taskCode.replaceAll(JSON.stringify(name), JSON.stringify(dataUrl(code)));
taskCode = compileModule(taskCode, { filename: "tasks.svelte.js", generate: "client" }).js.code;
taskCode = taskCode.replace(/(from\s+|import\s+)["'](svelte\/[^"']+)["']/g, (_, prefix, name) => `${prefix}${JSON.stringify(import.meta.resolve(name))}`);
const tasks = await import(dataUrl(taskCode));
const resourcePath = "src/lib/features/fxserver/ResourceManagerPage.svelte";
const managerPath = "src/lib/features/fxserver/ManageServerPage.svelte";
function workspaceBindings() {
  tasks.taskSession.items = [];
  tasks.taskSession.workspaceId = "A";
  tasks.taskSession.switching = false;
  return { workspaceId: "A", currentWorkspace: "A", workspaceRevision: 1, workspaceSession: { revision: 1 },
    taskSession: tasks.taskSession, trackTask: tasks.trackTask, error: "", message: "" };
}
function resourceFixture(overrides = {}) {
  const context = component(resourcePath, ["workspaceIsCurrent", "pageIsCurrent", "initialize", "runResourceCommand"], {
    ...workspaceBindings(), active: true, busyCommand: "", recentCommands: [],
    rcon: { host: "original", port: 30120, password: "original-secret" }, ...overrides,
  });
  context.getWorkspaceId = () => context.currentWorkspace;
  return context;
}
function managerFixture(overrides = {}) {
  const context = component(managerPath, ["workspaceIsCurrent", "pageIsCurrent", "actionIsCurrent", "requireCurrentWorkspace", "refreshStatus", "refreshAll",
    "startServer", "restartServer", "stopServer", "checkReadiness", "submitTerminalCommand"], {
    ...workspaceBindings(), pageActive: true, statusRefresh: null, terminalRevision: 0, actionRevision: 0, status: { running: false },
    nowSeconds: 0, recordResourceSample: () => {}, canStart: true, canRestart: true, busy: false,
    starting: false, stopping: false, restarting: false, checkingPreflight: false, rconSending: false,
    terminalCommand: "status", rconConfig: { host: "original", port: 30120, password: "secret" },
    preflight: null, databaseSession: { credentials: null }, log: () => {}, saveEnvironment: () => {},
    launchRequest: () => ({ artifactPath: "A/server", environment: [], serverProfile: "default" }),
    getFxserverStatus: async () => ({ running: true }), runPreflight: async () => ({ blocking: false }),
    startFxserver: async () => {}, restartFxserver: async () => {}, stopFxserver: async () => {},
    persistRconPassword: async () => {}, refreshTerminal: async () => {}, refreshArtifact: async () => {}, tick: async () => {}, ...overrides,
  });
  context.getWorkspaceId = () => context.currentWorkspace;
  return context;
}

test("resource commands snapshot the full target and remain tracked through save/send/navigation", async () => {
  const saved = deferred(), sent = deferred(), calls = [];
  const c = resourceFixture({ saveFxserverRconPassword: (password, workspace) => { calls.push({ password, workspace }); return saved.promise; },
    sendFxserverRconCommand: (command, config) => { calls.push({ command, ...config }); return sent.promise; } });
  const action = c.runResourceCommand("stop", { name: "test", path: "A/test" });
  assert.equal(tasks.hasRunningTasks(), true);
  c.rcon.host = "changed"; c.rcon.port = 30121; c.rcon.password = "changed-secret";
  await c.runResourceCommand("restart", { name: "duplicate", path: "A/duplicate" });
  assert.equal(calls.length, 1);
  c.active = false;
  saved.resolve(); await new Promise(setImmediate);
  assert.deepEqual(calls[1], { command: "stop test", host: "original", port: 30120, password: "original-secret" });
  assert.equal(tasks.hasRunningTasks(), true);
  sent.resolve(); await action;
  assert.equal(tasks.hasRunningTasks(), false);
  assert.equal(c.message, "");
  assert.equal(c.recentCommands.length, 0);
});

test("resource commands refuse workspace changes and do not send after password-store failure", async () => {
  for (const scenario of ["id", "revision", "switching", "failure"]) {
    const saved = deferred(); let sends = 0;
    const c = resourceFixture({ saveFxserverRconPassword: () => saved.promise, sendFxserverRconCommand: async () => sends++ });
    const action = c.runResourceCommand("stop", { name: "test", path: "A/test" });
    assert.equal(tasks.hasRunningTasks(), true);
    if (scenario === "id") c.currentWorkspace = "B";
    if (scenario === "revision") c.workspaceSession.revision++;
    if (scenario === "switching") c.taskSession.switching = true;
    if (scenario === "failure") saved.reject(new Error("Store unavailable")); else saved.resolve();
    await action;
    assert.equal(sends, 0);
    assert.equal(tasks.hasRunningTasks(), false);
  }
});

test("resource initialization reports password-load failures only to the current page", async () => {
  for (const stale of [false, true]) {
    const password = deferred();
    const c = resourceFixture({ loadFxserverSettings: () => {}, readSavedEnvironment: () => ({}),
      getSavedFxserverRconPassword: () => password.promise });
    const pending = c.initialize();
    if (stale) c.active = false;
    password.reject(new Error("Password store unavailable"));
    await pending;
    assert.equal(c.rcon.host, "original");
    assert.equal(c.error.includes("Password store unavailable"), !stale);
  }
});

test("late status successes/errors cannot overwrite a remount or another workspace", async () => {
  for (const failure of [false, true]) {
    for (const invalidation of ["unmount", "workspace"]) {
      const pending = deferred(); let samples = 0;
      const c = managerFixture({ getFxserverStatus: () => pending.promise, recordResourceSample: () => samples++ });
      const refresh = c.refreshStatus();
      if (invalidation === "unmount") c.pageActive = false; else c.workspaceSession.revision++;
      const current = { running: true, pid: 222 };
      c.status = current; c.error = "new page error"; c.message = "new page message";
      if (failure) pending.reject(new Error("old failure")); else pending.resolve({ running: false, pid: 111 });
      await refresh;
      assert.equal(c.status, current);
      assert.equal(c.error, "new page error"); assert.equal(c.message, "new page message");
      assert.equal(samples, 0);
    }
  }
});

test("a status poll spanning a server action is discarded and refreshed once", async () => {
  const pending = deferred(); let calls = 0;
  const c = managerFixture({ getFxserverStatus: () => ++calls === 1 ? pending.promise : Promise.resolve({ running: true, pid: 222 }) });
  const first = c.refreshStatus(false), joined = c.refreshStatus(false);
  assert.equal(calls, 1);
  c.terminalRevision++;
  pending.resolve({ running: false });
  await Promise.all([first, joined]);
  assert.equal(calls, 2); assert.equal(c.status.pid, 222);
});

test("start/restart hold a workspace task across preflight and use the captured launch request", async () => {
  for (const action of ["startServer", "restartServer"]) {
    const preflight = deferred(), mutation = deferred(); const requests = [];
    const c = managerFixture({ runPreflight: () => preflight.promise,
      startFxserver: (request) => { requests.push(request); return mutation.promise; },
      restartFxserver: (request) => { requests.push(request); return mutation.promise; } });
    const pending = c[action]();
    assert.equal(tasks.hasRunningTasks(), true);
    c.launchRequest = () => ({ artifactPath: "changed" });
    c.pageActive = false;
    preflight.resolve({ blocking: false }); await new Promise(setImmediate);
    assert.equal(requests[0].artifactPath, "A/server");
    assert.equal(tasks.hasRunningTasks(), true);
    mutation.resolve(); await pending;
    assert.equal(tasks.hasRunningTasks(), false);
    assert.equal(c.preflight.blocking, false); assert.match(c.message, /FXServer (started|restarted)/);
    assert.equal(c.starting || c.restarting || c.checkingPreflight, false);
  }
});

test("stale or blocking preflight never continues to a server mutation", async () => {
  for (const scenario of ["workspace", "blocking"]) {
    const preflight = deferred(); let mutations = 0;
    const c = managerFixture({ runPreflight: () => preflight.promise, startFxserver: async () => mutations++ });
    const action = c.startServer();
    if (scenario === "workspace") { c.workspaceSession.revision++; c.preflight = { newPage: true }; }
    preflight.resolve({ blocking: scenario === "blocking" }); await action;
    assert.equal(mutations, 0); assert.equal(tasks.hasRunningTasks(), false);
    if (scenario === "workspace") assert.equal(c.preflight.newPage, true);
    else assert.match(c.error, /blocking/);
  }
});

test("stop stays tracked through the status refresh", async () => {
  const mutation = deferred(), status = deferred();
  const c = managerFixture({ stopFxserver: () => mutation.promise, getFxserverStatus: () => status.promise });
  const action = c.stopServer();
  assert.equal(tasks.hasRunningTasks(), true);
  mutation.resolve(); await new Promise(setImmediate);
  assert.equal(tasks.hasRunningTasks(), true); assert.equal(c.stopping, true);
  status.resolve({ running: false }); await action;
  assert.equal(tasks.hasRunningTasks(), false); assert.equal(c.stopping, false);
  assert.equal(c.status.running, false);
});

test("console sends without waiting for DPAPI and stays tracked after an early command failure", async () => {
  const tick = deferred(), saved = deferred(), sent = deferred(); const calls = [];
  const c = managerFixture({ tick: () => tick.promise, persistRconPassword: (password) => { calls.push(password); return saved.promise; },
    sendFxserverCommand: (command, config) => { calls.push({ command, ...config }); return sent.promise; } });
  const action = c.submitTerminalCommand();
  assert.equal(tasks.hasRunningTasks(), true);
  c.rconConfig.host = "changed"; c.rconConfig.password = "changed";
  tick.resolve(); await new Promise(setImmediate);
  assert.equal(calls[0], "secret"); assert.equal(tasks.hasRunningTasks(), true);
  assert.deepEqual(calls[1], { command: "status", host: "original", port: 30120, password: "secret" });
  sent.reject(new Error("old error")); await new Promise(setImmediate);
  assert.equal(tasks.hasRunningTasks(), true); assert.equal(c.rconSending, true);
  c.pageActive = false; c.actionRevision++; c.terminalCommand = "new action command"; c.error = "new action error";
  saved.resolve(); await action;
  assert.equal(c.terminalCommand, "new action command"); assert.equal(c.error, "new action error");
  assert.equal(c.rconSending, false); assert.equal(tasks.hasRunningTasks(), false);
});

test("console stays tracked when DPAPI finishes first and guards workspace changes before dispatch", async () => {
  const sent = deferred(); let sends = 0;
  const c = managerFixture({ sendFxserverCommand: () => { sends++; return sent.promise; } });
  const action = c.submitTerminalCommand(); await new Promise(setImmediate);
  assert.equal(sends, 1); assert.equal(tasks.hasRunningTasks(), true);
  sent.resolve(); await action;
  assert.equal(tasks.hasRunningTasks(), false);

  for (const invalidation of ["id", "revision", "switching"]) {
    const tick = deferred(); let mutations = 0;
    const stale = managerFixture({ tick: () => tick.promise, persistRconPassword: async () => mutations++,
      sendFxserverCommand: async () => mutations++ });
    const pending = stale.submitTerminalCommand();
    if (invalidation === "id") stale.currentWorkspace = "B";
    if (invalidation === "revision") stale.workspaceSession.revision++;
    if (invalidation === "switching") stale.taskSession.switching = true;
    tick.resolve(); await pending;
    assert.equal(mutations, 0); assert.equal(tasks.hasRunningTasks(), false);
  }
});

test("same-workspace background failures and command restoration survive navigation", async () => {
  for (const action of ["startServer", "restartServer", "stopServer", "submitTerminalCommand"]) {
    const mutation = deferred();
    const c = managerFixture({ startFxserver: () => mutation.promise, restartFxserver: () => mutation.promise,
      stopFxserver: () => mutation.promise, sendFxserverCommand: () => mutation.promise });
    const pending = c[action](); await new Promise(setImmediate);
    c.pageActive = false;
    mutation.reject(new Error("Background failure")); await pending;
    assert.match(c.error, /Background failure/);
    if (action === "submitTerminalCommand") assert.equal(c.terminalCommand, "status");
    c.pageActive = true;
    await c.refreshAll(false);
    assert.match(c.error, /Background failure/);
    assert.equal(tasks.hasRunningTasks(), false);
  }
});

test("background actions cannot publish errors or restore commands into another workspace", async () => {
  for (const action of ["startServer", "restartServer", "stopServer", "submitTerminalCommand"]) {
    const mutation = deferred();
    const c = managerFixture({ startFxserver: () => mutation.promise, restartFxserver: () => mutation.promise,
      stopFxserver: () => mutation.promise, sendFxserverCommand: () => mutation.promise });
    const pending = c[action](); await new Promise(setImmediate);
    c.workspaceSession.revision++;
    c.error = "new workspace error"; c.terminalCommand = "";
    mutation.reject(new Error("old workspace failure")); await pending;
    assert.equal(c.error, "new workspace error"); assert.equal(c.terminalCommand, "");
    assert.equal(tasks.hasRunningTasks(), false);
  }
});

const lua = loadLogic("src/lib/features/configurator/configLua.ts");
test("Lua rejects expressions, control flow, dynamic keys, and malformed syntax without partial output", () => {
  for (const source of [
    "Config = {}\nConfig.Timeout = 60 * 1000", "Config = {Timeout = 60 * 1000}",
    "Config = {}\nConfig.Debug = false\nif false then Config.Debug = true end",
    "local x = 10\nConfig = {value = x}", "Config = {value = GetConvar('x', 'y')}",
    "Config = {}\nreturn Config", "return {}", "Config = {}\nConfig[key] = 1",
    "Config = {[key] = 1}", "Config = {a = 1 b = 2}", "Config = {a = 1", 'Config = {1 "}"}',
    "Config = {a = 1, a = 2}", "Config = {[1] = 1, 2}", "Config = {}\nConfig.Missing.Value = 1",
    "Config = {Value = 1}\nConfig.Value.Nested = 2", "Config = {end = 1}", "Config.end = 1",
    "Config.V = vector3(1, 2, 3", "Config.V = vector3(1 2 3)", "Config.V = vector3(1, 2, 3,)",
    'Config.X = "\\x41"', 'Config.X = "\\123"', "Config.X = 1e999", "Config.X = 9007199254740993",
    "Config = {} --[[ unfinished", "Config = {}\n--[=[ unsupported ]=]",
  ]) assert.throws(() => lua.parseConfigLua(source), undefined, source);
});

test("supported literal Lua, nested tables, vectors, comments, and small decimals round-trip", () => {
  for (const source of [lua.sampleConfigLua, 'Config = {tiny = 0.0000001, text = "a\\n\\t\\\\b", nested = {["key"] = false}, values = {1, 2, 3}}',
    "Config = {}\r-- comment\rConfig.Value = 42;", "Config = 42", 'Config["end"] = 1',
    "Config = {Nested = {}}\nConfig.Nested.Value = 2"]) {
    const first = lua.parseConfigLua(source), second = lua.parseConfigLua(first.output);
    assert.equal(first.output, second.output);
    assert.equal(first.warnings.length, 0);
  }
});

test("Configurator clears existing generated output after unsupported input", () => {
  const c = component("src/lib/features/configurator/ConfiguratorPage.svelte", ["parseSource"], {
    source: "Config.Value = 60 * 1000", config: { previous: true }, selectedId: "old", selectedGroupId: "old",
    parseWarnings: [], commentsByPath: {}, unassignedComments: [], notice: null,
    parseConfigLua: lua.parseConfigLua, log: () => {},
  });
  c.parseSource();
  assert.equal(c.config, null); assert.equal(c.notice.type, "error");
});

const profiler = loadLogic("src/lib/features/profiler/profilerAnalyzer.ts");
const span = (name, start, duration, tid = 1) => ({ name, ph: "X", pid: 1, tid, ts: start * 1000, dur: duration * 1000 });
test("profiler counts nested intervals once while retaining inclusive entry timings", () => {
  const result = profiler.analyzeProfilerJson({ traceEvents: [span("Resource Manager Tick", 0, 20), span("tick (test)", 1, 18), span("thread @test/file.lua", 2, 16)] });
  assert.equal(result.stats.worstFrameMs, 20); assert.equal(result.stats.totalScriptMs, 20);
  assert.equal(result.stats.hitchCount, 0); assert.equal(result.frameTimeline[0].state, "excellent");
  assert.equal(result.resources.find((entry) => entry.name === "tick (test)").totalMs, 18);
});

test("explicit frames stay separate, ignore other threads, and still identify real hitches", () => {
  const result = profiler.analyzeProfilerJson({ traceEvents: [span("Resource Manager Tick", 0, 20), span("tick (test)", 1, 18),
    span("Resource Manager Tick", 21, 30), span("tick (test)", 22, 28), span("browser work", 0, 1000, 2)] });
  assert.equal(result.stats.frameCount, 2); assert.equal(result.stats.worstFrameMs, 30);
  assert.equal(result.stats.totalScriptMs, 50); assert.equal(result.stats.hitchCount, 1);
});

test("fallback traces and B/E traces also avoid nested duration inflation", () => {
  const fallback = profiler.analyzeProfilerJson({ traceEvents: [span("tick (test)", 0, 20), span("thread @test/file.lua", 1, 18)] });
  assert.equal(fallback.stats.worstFrameMs, 20); assert.equal(fallback.stats.totalScriptMs, 20);
  const paired = profiler.analyzeProfilerJson({ traceEvents: [
    { name: "Resource Manager Tick", ph: "B", ts: 0 }, { name: "tick (test)", ph: "B", ts: 1000 },
    { name: "tick (test)", ph: "E", ts: 19000 }, { name: "Resource Manager Tick", ph: "E", ts: 20000 },
  ] });
  assert.equal(paired.stats.worstFrameMs, 20); assert.equal(paired.stats.hitchCount, 0);
});
