import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";
import { compileModule } from "svelte/compiler";

const read = (path) => readFileSync(new URL(`../${path}`, import.meta.url), "utf8");
const transpile = (source, module = ts.ModuleKind.ES2022) => ts.transpile(source, { target: ts.ScriptTarget.ES2022, module });
const dataUrl = (source) => `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`;
const analysisContext = vm.createContext({ exports: {} });
vm.runInContext(transpile(read("src/lib/features/profiler/profilerAnalyzer.ts"), ts.ModuleKind.CommonJS), analysisContext);
const { analyzeProfilerJson, compareProfilerCaptures } = analysisContext.exports;
const protocolContext = vm.createContext({ exports: {} });
vm.runInContext(transpile(read("src/lib/features/profiler/profilerAnalysisProtocol.ts"), ts.ModuleKind.CommonJS), protocolContext);
const protocol = protocolContext.exports;
const workerCode = transpile(read("src/lib/features/profiler/profiler.worker.ts"), ts.ModuleKind.CommonJS);
async function workerResult(request) {
  let response;
  const context = vm.createContext({ exports: {}, TextEncoder, Blob, self: { postMessage: (value) => response = value }, require: (name) => {
    if (name === "./profilerAnalyzer") return analysisContext.exports;
    if (name === "./profilerAnalysisProtocol") return protocol;
    throw new Error(`Unexpected worker import ${name}`);
  } });
  vm.runInContext(workerCode, context);
  await context.self.onmessage({ data: request });
  return response;
}
const span = (name, start, duration) => ({ name, ph: "X", ts: start * 1000, dur: duration * 1000, pid: 1, tid: 1 });
const trace = (count = 2, duration = 10, resource = 6) => ({ traceEvents: Array.from({ length: count }, (_, index) => [
  span("Resource Manager Tick", index * 50, duration), span("tick (fixture)", index * 50 + 1, resource),
]).flat() });

test("capture comparisons normalize unequal frame counts without adding nested CPU", () => {
  const before = analyzeProfilerJson(trace(2));
  const after = analyzeProfilerJson(trace(4));
  const comparison = compareProfilerCaptures(before, after);
  assert.equal(comparison.averageFrameDeltaMs, 0);
  assert.equal(comparison.entries[0].deltaMsPerFrame, 0);
  assert.equal(comparison.entries[0].beforeMsPerFrame, 6);
  assert.equal(comparison.entries.length, 1, "Frame container is not a resource");
  assert.match(comparison.warnings.join(" "), /lengths differ/);
  assert.equal(before.stats.totalScriptMs, 20);
});

test("comparison detects changes and uses percentage points for heavy frames", () => {
  const result = compareProfilerCaptures(analyzeProfilerJson(trace(2)), analyzeProfilerJson(trace(2, 30, 12)));
  assert.equal(result.averageFrameDeltaMs, 20);
  assert.equal(result.worstFrameDeltaMs, 20);
  assert.equal(result.heavyFrameDeltaPoints, 100);
  assert.equal(result.entries[0].deltaMsPerFrame, 6);
});

test("zero-frame and inferred-frame comparisons are explicitly qualified", () => {
  const result = compareProfilerCaptures(analyzeProfilerJson({ traceEvents: [] }), analyzeProfilerJson({ traceEvents: [span("tick (fixture)", 0, 5)] }));
  assert.equal(result.entries.length, 0);
  assert.match(result.warnings.join(" "), /no measured frames/);
  assert.match(result.warnings.join(" "), /inferred frame/);
});

test("Cfx instant markers and nameless end events are understood", () => {
  const result = analyzeProfilerJson({ traceEvents: [
    { name: "BeginFrame", ph: "I", ts: 0 }, { name: "Resource Manager Tick", ph: "B", ts: 0 },
    { ph: "E", ts: 5000 }, { name: "Screenshot", ph: "O", ts: 5000 },
  ] });
  assert.equal(result.stats.frameCount, 1);
  assert.equal(result.stats.worstFrameMs, 5);
  assert.equal(result.stats.browserFrames, 1);
  assert.equal(result.stats.screenshots, 1);
});

test("malformed and nonfinite events cannot become invented timing data", () => {
  const result = analyzeProfilerJson({ traceEvents: [null, 42, [], {}, { name: "bridge delay", ph: "X", ts: NaN, dur: Infinity },
    span("tick (fixture)", 0, -2), { name: "tick (bad)", ph: "X", ts: 0, dur: Infinity }, ...trace().traceEvents] });
  assert.equal(result.stats.totalScriptMs, 20);
  assert.ok(result.resources.every((entry) => Number.isFinite(entry.totalMs)));
  assert.throws(() => analyzeProfilerJson({ traceEvents: new Array(500_001) }), /event limit/);
});

let unique = 0;
async function loadSession() {
  const key = `__profilerFixture${++unique}`;
  const fixture = globalThis[key] = {
    calls: [], timers: [], tracks: [], workspace: "fixture", password: "fixture-secret", status: null,
    invoke: async () => null, taskSession: { switching: false },
    workers: [], workerTimers: [], workerMode: "success", workerResult,
  };
  const fixtureRef = `globalThis[${JSON.stringify(key)}]`;
  fixture.Worker = class {
    constructor() {
      if (fixture.workerMode === "constructor-error") throw new Error("Constructor failure");
      fixture.workers.push(this); this.terminated = false;
    }
    postMessage(request) {
      this.request = request;
      this.lateMessage = this.onmessage;
      if (fixture.workerMode === "post-error") throw new Error("Post failure");
      if (fixture.workerMode !== "success") return;
      void fixture.workerResult(request).then((data) => {
        this.result = data.result;
        this.onmessage?.({ data });
      });
    }
    terminate() { this.terminated = true; }
  };
  let workerClient = transpile(read("src/lib/features/profiler/profilerAnalysis.ts"));
  workerClient = workerClient.replaceAll(JSON.stringify("./profilerAnalysisProtocol"), JSON.stringify(dataUrl(transpile(read("src/lib/features/profiler/profilerAnalysisProtocol.ts")))));
  workerClient = workerClient.replaceAll("import.meta.url", JSON.stringify(new URL("../src/lib/features/profiler/profilerAnalysis.ts", import.meta.url).href));
  workerClient = `const Worker = ${fixtureRef}.Worker; const setTimeout = (callback, delay) => { const timer = {callback,delay,active:true}; ${fixtureRef}.workerTimers.push(timer); return timer; }; const clearTimeout = timer => { if(timer) timer.active = false; };\n${workerClient}`;
  const replacements = {
    "@tauri-apps/api/core": `export const invoke = (command, args) => { const f = ${fixtureRef}; f.calls.push({command,args}); return f.invoke(command,args); };`,
    "$lib/core/tasks.svelte": `const f = ${fixtureRef}; export const taskSession = f.taskSession; export async function trackTask(command,label,action) { const task = {command,label,status:'running'}; f.tracks.push(task); try { const value = await action(); task.status = 'completed'; return value; } catch(error) { task.status = error.name === 'AbortError' ? 'cancelled' : 'failed'; throw error; } }`,
    "$lib/core/workspaces.svelte": `export const getWorkspaceId = () => ${fixtureRef}.workspace;`,
    "$lib/modules/fxserver": `export const getSavedFxserverRconPassword = async () => ${fixtureRef}.password;`,
    "$lib/features/fxserver/fxserverSettings.svelte": `export const readSavedEnvironment = () => ({TXHOST_RCON_HOST:'127.0.0.1',TXHOST_RCON_PORT:'30120'});`,
    "./profilerAnalysis": workerClient,
  };
  let source = transpile(read("src/lib/features/profiler/profilerCapture.svelte.ts"));
  source = `const window = {__TAURI_INTERNALS__:{}}; const setTimeout = callback => ${fixtureRef}.timers.push(callback);\n${source}`;
  for (const [name, code] of Object.entries(replacements)) source = source.replaceAll(JSON.stringify(name), JSON.stringify(dataUrl(code)));
  source = compileModule(source, { filename: "profilerCapture.svelte.js", generate: "client" }).js.code;
  source = source.replace(/(from\s+|import\s+)["'](svelte\/[^"']+)["']/g, (_, prefix, name) => `${prefix}${JSON.stringify(import.meta.resolve(name))}`);
  return { fixture, session: await import(dataUrl(source)), workerClient: await import(dataUrl(workerClient)) };
}
const settle = async () => { for (let i = 0; i < 12; i++) await Promise.resolve(); };

test("recording is opt-in, tracked across page subscribers, and opens real JSON on completion", async () => {
  const { fixture, session } = await loadSession();
  assert.equal(fixture.calls.length, 0);
  let complete;
  fixture.invoke = (command) => command === "capture_fxserver_profile" ? new Promise((resolve) => complete = resolve) : Promise.resolve({ id: "capture-one", frames: 100, phase: "recording", capturedFrames: 10 });
  const operation = session.startProfilerCapture(100);
  await settle();
  assert.equal(fixture.tracks[0].status, "running");
  assert.equal(session.profilerCaptureSession.busy, true);
  await session.startProfilerCapture(100);
  assert.equal(fixture.calls.filter((call) => call.command === "capture_fxserver_profile").length, 1);
  const request = fixture.calls.find((call) => call.command === "capture_fxserver_profile").args.request;
  assert.deepEqual(Object.keys(request).sort(), ["frames", "rcon"]);
  assert.equal(request.frames, 100);
  complete({ id: "capture-one", fileName: "fxi-profiler-one.json", frames: 100, json: JSON.stringify(trace()) });
  await operation;
  assert.equal(fixture.tracks[0].status, "completed");
  assert.equal(session.profilerCaptureSession.openRequest, 1);
  assert.equal(session.profilerCaptureSession.selectedId, "capture-one");
  assert.equal(session.profilerCaptureSession.captures[0].analysis.stats.totalScriptMs, 20);
  const count = fixture.calls.length;
  fixture.timers.splice(0).forEach((callback) => callback());
  await settle();
  assert.equal(fixture.calls.length, count, "No progress polling after completion");
});

test("cancellation requested before the status ID arrives is delivered once", async () => {
  const { fixture, session } = await loadSession();
  let fail, releaseStatus;
  fixture.invoke = (command) => {
    if (command === "capture_fxserver_profile") return new Promise((_, reject) => fail = reject);
    if (command === "get_profiler_capture_status") return new Promise((resolve) => releaseStatus = resolve);
    if (command === "cancel_profiler_capture") { fail("Profiler capture cancelled."); return Promise.resolve(); }
  };
  const operation = session.startProfilerCapture(250);
  await settle();
  await session.cancelProfilerCapture();
  assert.equal(fixture.calls.filter((call) => call.command === "cancel_profiler_capture").length, 0);
  releaseStatus({ id: "cancel-id", frames: 250, phase: "recording", capturedFrames: 2 });
  await operation;
  assert.equal(fixture.calls.filter((call) => call.command === "cancel_profiler_capture").length, 1);
  assert.equal(fixture.tracks[0].status, "cancelled");
  assert.equal(session.profilerCaptureSession.openRequest, 0);
});

test("invalid requests, missing passwords, and native failures preserve the viewer", async () => {
  const { fixture, session } = await loadSession();
  await session.importProfilerCapture(JSON.stringify(trace()), "existing.json");
  const selected = session.profilerCaptureSession.selectedId;
  await session.startProfilerCapture(9999);
  assert.equal(fixture.calls.length, 0);
  fixture.password = "";
  await session.startProfilerCapture(100);
  assert.equal(fixture.calls.length, 0);
  fixture.password = "secret";
  fixture.invoke = async () => { throw "A manual profiler recording is already active."; };
  await session.startProfilerCapture(100);
  assert.equal(session.profilerCaptureSession.selectedId, selected);
  assert.equal(session.profilerCaptureSession.phase, "failed");
  assert.match(session.profilerCaptureSession.error, /already active/);
  assert.equal(session.profilerCaptureSession.busy, false);
});

test("imports retain only two captures and reject oversized or empty traces", async () => {
  const { session } = await loadSession();
  for (let index = 0; index < 3; index++) await session.importProfilerCapture(JSON.stringify(trace()), `${index}.json`);
  assert.equal(session.profilerCaptureSession.captures.length, 2);
  assert.equal(session.profilerCaptureSession.captures[0].name, "1.json");
  const selected = session.profilerCaptureSession.selectedId;
  await assert.rejects(session.importProfilerCapture("x".repeat(session.MAX_PROFILER_BYTES + 1), "large.json"), /16 MiB/);
  await assert.rejects(session.importProfilerCapture('{"traceEvents":[]}', "empty.json"), /No completed timing/);
  assert.equal(session.profilerCaptureSession.selectedId, selected);
});

test("worker accepts Blob imports and compares complete analyses, not just displayed rankings", async () => {
  const { fixture, session } = await loadSession();
  const input = { traceEvents: [span("Resource Manager Tick", 0, 20), ...Array.from({ length: 120 }, (_, i) => span(`tick (resource-${i})`, 1, 2))] };
  await session.importProfilerCapture(new Blob([JSON.stringify(input)]), "before.json");
  const result = await session.importProfilerCapture(JSON.stringify(input), "after.json");
  assert.equal(session.profilerCaptureSession.comparison.entries.length, 120);
  assert.equal(session.profilerCaptureSession.comparison.entries.at(-1).deltaMsPerFrame, 0);
  assert.equal(result.analysis.resources.length, 121);
  assert.equal(session.profilerCaptureSession.captures[1].analysis, fixture.workers[1].result.analysis, "Large analysis must not be deep-proxied");
  assert.equal(session.profilerCaptureSession.comparison, fixture.workers[1].result.comparison);
  assert.ok(fixture.workers.every((worker) => worker.terminated && worker.onmessage === null));
  assert.ok(fixture.workerTimers.every((timer) => !timer.active));
});

test("worker enforces byte/event budgets and does not leak malformed JSON contents", async () => {
  assert.match((await workerResult({ input: '"' + "\u20ac".repeat(Math.floor(protocol.MAX_PROFILER_BYTES / 3) + 1) + '"' })).error, /16 MiB/);
  assert.match((await workerResult({ input: new Blob(["x".repeat(protocol.MAX_PROFILER_BYTES + 1)]) })).error, /16 MiB/);
  assert.match((await workerResult({ input: JSON.stringify({ traceEvents: new Array(500_001).fill(null) }) })).error, /event limit/);
  const invalid = await workerResult({ input: '{private-trace-secret:' });
  assert.ok(invalid.error);
  assert.ok(!invalid.error.includes("private-trace-secret"));
});

test("worker deadline terminates the job and late replies cannot replace previous analysis", async () => {
  const { fixture, session } = await loadSession();
  await session.importProfilerCapture(JSON.stringify(trace()), "baseline.json");
  const baseline = session.profilerCaptureSession.selectedId;
  fixture.workerMode = "pending";
  const pending = session.importProfilerCapture(JSON.stringify(trace()), "slow.json");
  const rejected = assert.rejects(pending, /15-second limit/);
  const worker = fixture.workers.at(-1);
  const timer = fixture.workerTimers.at(-1);
  assert.equal(timer.delay, 15_000);
  timer.callback();
  await rejected;
  worker.lateMessage({ data: await workerResult({ input: JSON.stringify(trace()) }) });
  assert.equal(worker.terminated, true);
  assert.equal(timer.active, false);
  assert.equal(session.profilerCaptureSession.analyzing, false);
  assert.equal(session.profilerCaptureSession.selectedId, baseline);
});

test("manual import abort and disposal preserve the viewer and allow a new job", async () => {
  const { fixture, session } = await loadSession();
  fixture.workerMode = "pending";
  const abort = new AbortController();
  const pending = session.importProfilerCapture(new Blob([JSON.stringify(trace())]), "cancel.json", { signal: abort.signal });
  const rejected = assert.rejects(pending, { name: "AbortError" });
  await assert.rejects(session.importProfilerCapture("{}", "duplicate.json"), /current profiler/);
  abort.abort();
  await rejected;
  assert.equal(fixture.workers[0].terminated, true);
  assert.equal(session.profilerCaptureSession.captures.length, 0);
  fixture.workerMode = "success";
  await session.importProfilerCapture(JSON.stringify(trace()), "next.json");
  assert.equal(session.profilerCaptureSession.captures.length, 1);
});

test("worker load, clone, and message errors always dispose the job", async () => {
  for (const mode of ["constructor-error", "post-error", "pending"]) {
    const { fixture, workerClient } = await loadSession();
    fixture.workerMode = mode;
    const pending = workerClient.analyzeProfilerCapture("{}");
    const rejected = assert.rejects(pending, /analysis failed/);
    if (mode === "pending") fixture.workers[0].onmessageerror();
    await rejected;
    assert.ok(fixture.workers.every((worker) => worker.terminated));
    assert.ok(fixture.workerTimers.every((timer) => !timer.active));
  }
  const { fixture, workerClient } = await loadSession();
  fixture.workerMode = "pending";
  const rejected = assert.rejects(workerClient.analyzeProfilerCapture("{}"), /analysis failed/);
  let prevented = false;
  fixture.workers[0].onerror({ preventDefault() { prevented = true; } });
  await rejected;
  assert.equal(prevented, true);
  assert.equal(fixture.workers[0].terminated, true);
});

test("guided analysis stays in Task Center and cancellation no longer sends server commands", async () => {
  const { fixture, session } = await loadSession();
  fixture.workerMode = "pending";
  fixture.invoke = async (command) => command === "capture_fxserver_profile" ? { id: "guided", fileName: "guided.json", frames: 100, json: JSON.stringify(trace()) } : null;
  const operation = session.startProfilerCapture(100);
  await settle();
  assert.equal(session.profilerCaptureSession.phase, "analyzing");
  assert.equal(fixture.tracks[0].status, "running");
  await session.cancelProfilerCapture();
  await operation;
  assert.equal(fixture.tracks[0].status, "cancelled");
  assert.equal(fixture.workers[0].terminated, true);
  assert.equal(session.profilerCaptureSession.openRequest, 0);
  assert.equal(fixture.calls.some((call) => call.command === "cancel_profiler_capture"), false);
});
