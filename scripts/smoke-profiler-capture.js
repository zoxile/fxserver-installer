async (page) => {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.addInitScript(() => {
    localStorage.clear();
    sessionStorage.clear();
    localStorage.setItem("fxserver.manage.env", JSON.stringify({ TXHOST_RCON_HOST: "127.0.0.1", TXHOST_RCON_PORT: "30120" }));
    const state = window.profilerTest = { calls: [], logs: [], sequence: 0, pending: null, active: null, failure: "", password: "fixture-rcon-secret", workerMode: "normal", workersStarted: 0, workersActive: 0 };
    const NativeWorker = window.Worker;
    window.Worker = class extends NativeWorker {
      constructor(url, options) {
        super(url, options);
        this.profiler = String(url).includes("profiler.worker");
        this.closed = false;
        if (this.profiler) { state.workersStarted++; state.workersActive++; }
      }
      postMessage(...args) {
        if (this.profiler && state.workerMode === "stall") return;
        if (this.profiler && state.workerMode === "error") {
          queueMicrotask(() => this.dispatchEvent(new ErrorEvent("error", { message: "fixture worker failure", cancelable: true })));
          return;
        }
        return super.postMessage(...args);
      }
      terminate() {
        if (this.profiler && !this.closed) { state.workersActive--; this.closed = true; }
        return super.terminate();
      }
    };
    const trace = (cost, resource) => ({ traceEvents: Array.from({ length: 4 }, (_, index) => [
      { name: "Resource Manager Tick", ph: "X", ts: index * 50000, dur: cost * 1000, pid: 1, tid: 1 },
      { name: "tick (fixture-resource)", ph: "X", ts: index * 50000 + 1000, dur: resource * 1000, pid: 1, tid: 1 },
    ]).flat() });
    state.complete = (cost = 10, resource = 6) => {
      const { id, frames } = state.active;
      const pending = state.pending;
      state.active = null;
      state.pending = null;
      pending.resolve({ id, frames, fileName: `${id}.json`, json: JSON.stringify(trace(cost, resource)) });
    };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
      transformCallback: () => ++state.sequence,
      invoke: async (command, args = {}) => {
        state.calls.push({ command, args });
        switch (command) {
          case "get_platform_info": return { os: "windows", arch: "x86_64", artifactPlatform: "windows", clientLogsSupported: true, secureStorageSupported: true };
          case "capture_fxserver_profile": {
            const request = args.request;
            if (Object.keys(request).sort().join(",") !== "frames,rcon") throw new Error("Unexpected capture arguments");
            if (![100, 250, 500].includes(request.frames)) throw new Error("Unbounded frame request");
            if (request.rcon.host !== "127.0.0.1" || request.rcon.port !== 30120) throw new Error("Wrong RCON target");
            if (state.failure) throw new Error(state.failure);
            if (state.active) throw new Error("Duplicate capture");
            state.active = { id: `fxi-profiler-fixture-${++state.sequence}`, frames: request.frames, phase: "recording", capturedFrames: 12, cancelRequested: false };
            return new Promise((resolve, reject) => state.pending = { resolve, reject });
          }
          case "get_profiler_capture_status": return state.active;
          case "cancel_profiler_capture": {
            if (args.id !== state.active?.id) throw new Error("Wrong cancellation ID");
            state.active = null;
            state.pending.reject("Profiler capture cancelled.");
            state.pending = null;
            return;
          }
          case "get_fxserver_rcon_password": return state.password;
          case "plugin:app|version": return "0.5.1";
          case "plugin:window|title": return "FXServer Installer";
          case "plugin:event|listen": return ++state.sequence;
          case "plugin:event|unlisten": case "initialize_health_workspace": return;
          case "append_app_log": state.logs.push(args.entry); return;
          case "read_app_logs": return { path: "mock.log", entries: [] };
          case "load_database_login": return null;
          case "configure_live_bridge": return { workspaceId: "default", enabled: false, connected: false, snapshot: null };
          case "get_app_update_status": return { phase: "disabled", currentVersion: "0.5.2", availableVersion: null, releaseUrl: null, downloadedBytes: 0, totalBytes: null, message: "Updates disabled in fixture.", supported: false };
          case "fetch_latest_app_release": return null;
          case "get_windows_artifact_metadata": return { recommendedArtifact: "10000", windowsDownloadLink: "https://example.invalid/artifact.zip", brokenArtifacts: [] };
          case "get_installed_windows_artifact_info": return { installed: false };
          case "start_fxserver": case "restart_fxserver": case "stop_fxserver": case "send_fxserver_command": case "send_fxserver_rcon_command": case "read_text_file":
            throw new Error(`Forbidden server or arbitrary-file command: ${command}`);
          default: return null;
        }
      },
    };
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  const nav = page.getByRole("navigation", { name: "Workspace navigation" });
  const check = (value, message) => { if (!value) throw new Error(message); };
  const openProfiler = async () => {
    const tools = nav.getByTitle("Tools & Utils", { exact: true });
    if (await tools.getAttribute("aria-expanded") !== "true") await tools.click();
    await nav.getByTitle("Profiler", { exact: true }).click();
    await page.getByRole("heading", { name: "Profiler Analyzer", exact: true }).waitFor();
  };
  const openTaskCenter = async () => {
    await page.getByTitle(/^Task Center/).click();
    await page.getByRole("heading", { name: "Task Center", exact: true }).waitFor();
  };
  await openProfiler();
  check(await page.evaluate(() => !window.profilerTest.calls.some((call) => call.command === "capture_fxserver_profile")), "Capture started without explicit consent");
  const controls = page.getByRole("region", { name: "Guided performance recording" });
  const record = () => controls.getByRole("button", { name: "Record capture", exact: true }).click();
  const waitPending = () => page.waitForFunction(() => window.profilerTest.pending !== null);
  const captureSelect = page.getByLabel("Capture", { exact: true });
  const selectedCapture = async () => (await captureSelect.innerText()).trim();
  const checkCaptureCount = async (count, message) => {
    await captureSelect.click();
    const menu = page.getByRole("listbox");
    await menu.waitFor();
    check(await menu.getByRole("option").count() === count, message);
    await page.keyboard.press("Escape");
    await menu.waitFor({ state: "hidden" });
  };
  await controls.getByLabel("Frames", { exact: true }).click();
  await page.getByRole("option", { name: "100 frames", exact: true }).waitFor();
  check(JSON.stringify((await page.getByRole("listbox").getByRole("option").allTextContents()).map((text) => text.trim())) === JSON.stringify(["100 frames", "250 frames", "500 frames"]), "Frame menu permits an unbounded capture");
  await page.getByRole("option", { name: "100 frames", exact: true }).click();
  await record();
  await waitPending();
  await controls.getByText("Recording: 12 / 100 frames", { exact: true }).waitFor();
  check(await controls.getByLabel("Frames", { exact: true }).isDisabled(), "Frame count was editable during capture");
  await openTaskCenter();
  await page.getByText("Record FXServer performance", { exact: true }).waitFor();
  await page.getByText("1 running", { exact: true }).waitFor();
  check(await page.evaluate(() => window.profilerTest.active !== null), "Navigation cancelled the recording");
  await page.evaluate(() => window.profilerTest.complete());
  await page.getByRole("heading", { name: "Profiler Analyzer", exact: true }).waitFor();
  await page.getByText(/Loaded fxi-profiler-fixture-/).waitFor();
  await checkCaptureCount(1, "Completed capture was not retained in viewer");
  check((await selectedCapture()).startsWith("fxi-profiler-fixture-"), "Completed capture was not selected in viewer");

  await record();
  await waitPending();
  await page.evaluate(() => window.profilerTest.complete(30, 12));
  const comparison = page.getByRole("region", { name: "Capture comparison", exact: true });
  await comparison.waitFor();
  await comparison.getByText("+20.00 ms", { exact: true }).first().waitFor();
  await comparison.getByText("+100.00 pp", { exact: true }).waitFor();
  await comparison.getByRole("cell", { name: "6.00", exact: true }).waitFor();
  await checkCaptureCount(2, "Two captures were not retained");
  await captureSelect.click();
  const baselineOption = page.getByRole("listbox").getByRole("option").first();
  const baseline = (await baselineOption.innerText()).trim();
  await baselineOption.click();
  check(await selectedCapture() === baseline, "Capture selection did not change the viewer");
  await openTaskCenter();
  await openProfiler();
  check(await selectedCapture() === baseline, "Navigation lost selected capture");

  for (const failure of ["A manual profiler recording is already active.", "FXServer did not confirm profiler status.", "Capture exceeds the 16 MiB limit."]) {
    await page.evaluate((failure) => window.profilerTest.failure = failure, failure);
    await record();
    await controls.getByRole("alert").filter({ hasText: failure }).waitFor();
    check(await selectedCapture() === baseline, "A failed capture replaced the previous viewer result");
  }
  await page.evaluate(() => { window.profilerTest.failure = ""; });
  await record();
  await waitPending();
  await controls.getByRole("button", { name: "Cancel capture", exact: true }).click();
  await controls.getByRole("alert").filter({ hasText: "Profiler capture cancelled." }).waitFor();
  check(await page.evaluate(() => window.profilerTest.active === null), "Cancellation did not clear the backend fixture");
  await checkCaptureCount(2, "Cancellation added a capture");

  await record();
  await waitPending();
  await page.evaluate(() => window.profilerTest.complete(8, 3));
  await controls.getByRole("button", { name: "Record capture", exact: true }).waitFor();
  await checkCaptureCount(2, "Session capture retention exceeded two");
  for (const width of [1440, 390]) {
    await page.setViewportSize({ width, height: width === 390 ? 844 : 1000 });
    await controls.scrollIntoViewIfNeeded();
    for (const section of [controls, comparison]) {
      const dimensions = await section.evaluate((element) => ({ width: element.clientWidth, scroll: element.scrollWidth }));
      check(dimensions.scroll <= dimensions.width + 1, "Profiler section overflowed its narrow container");
    }
    await page.screenshot({ path: `output/playwright/profiler-capture-${width}.png`, fullPage: true });
    await captureSelect.click();
    const menu = page.getByRole("listbox");
    const bounds = await menu.boundingBox();
    check(bounds && bounds.x >= 0 && bounds.x + bounds.width <= width + 1, "Capture menu overflowed the viewport");
    await page.keyboard.press("Escape");
    await menu.waitFor({ state: "hidden" });
  }
  await page.setViewportSize({ width: 1440, height: 1000 });
  check(await page.evaluate(() => window.profilerTest.workersStarted >= 3 && window.profilerTest.workersActive === 0), "Completed analysis workers were not disposed");
  await page.evaluate(() => window.profilerTest.workerMode = "stall");
  await record();
  await waitPending();
  await page.evaluate(() => window.profilerTest.complete());
  await controls.getByText("Analyzing capture", { exact: true }).waitFor();
  await openTaskCenter();
  await page.getByText("1 running", { exact: true }).waitFor();
  check(await page.evaluate(() => window.profilerTest.workersActive === 1), "Navigation disposed a guided background analysis");
  await openProfiler();
  const nativeCancels = await page.evaluate(() => window.profilerTest.calls.filter((call) => call.command === "cancel_profiler_capture").length);
  await controls.getByRole("button", { name: "Cancel capture", exact: true }).click();
  await controls.getByRole("alert").filter({ hasText: "Profiler analysis cancelled." }).waitFor();
  check(await page.evaluate((count) => window.profilerTest.workersActive === 0 && window.profilerTest.calls.filter((call) => call.command === "cancel_profiler_capture").length === count, nativeCancels), "Analysis cancellation sent a stale native cancellation or leaked its worker");

  const importInput = page.getByLabel("Upload profiler JSON", { exact: true });
  const upload = () => importInput.setInputFiles({ name: "worker-fixture.json", mimeType: "application/json", buffer: Buffer.from('{"traceEvents":[{"name":"Resource Manager Tick","ph":"X","ts":0,"dur":10000}]}') });
  const selectedBeforeImport = await selectedCapture();
  await upload();
  await page.getByRole("button", { name: "Cancel profiler import", exact: true }).click();
  await page.getByRole("button", { name: "Cancel profiler import", exact: true }).waitFor({ state: "detached" });
  check(await page.evaluate(() => window.profilerTest.workersActive === 0), "Cancelled import worker remained active");
  await upload();
  await page.getByRole("button", { name: "Cancel profiler import", exact: true }).waitFor();
  await openTaskCenter();
  await page.waitForFunction(() => window.profilerTest.workersActive === 0);
  await openProfiler();
  check(await selectedCapture() === selectedBeforeImport, "Disposed import changed the viewer");
  await page.evaluate(() => window.profilerTest.workerMode = "error");
  await upload();
  await page.getByRole("alert").filter({ hasText: "Profiler analysis failed." }).waitFor();
  check(await page.evaluate(() => window.profilerTest.workersActive === 0), "Failed worker was not disposed");

  await page.evaluate(() => window.profilerTest.workerMode = "normal");
  const largeTrace = { traceEvents: Array.from({ length: 501 }, (_, index) => [
    { name: "Resource Manager Tick", ph: "X", ts: index * 50000, dur: 10000, pid: 1, tid: 1 },
    { name: `tick (entry-${index})`, ph: "X", ts: index * 50000 + 1000, dur: 1000, pid: 1, tid: 1 },
  ]).flat() };
  await importInput.setInputFiles({ name: "paged-fixture.json", mimeType: "application/json", buffer: Buffer.from(JSON.stringify(largeTrace)) });
  await page.getByText("Loaded paged-fixture.json", { exact: true }).waitFor();
  await page.getByText("Entries 1-100 of 502", { exact: true }).waitFor();
  check(await page.getByRole("button", { name: /^Frame \d+,/ }).count() === 500, "Frame rendering exceeded its page budget");
  await page.getByRole("button", { name: "Next profiler frames", exact: true }).click();
  await page.getByRole("button", { name: /^Frame 501,/ }).waitFor();
  await page.getByRole("button", { name: "Next profiler entries", exact: true }).click();
  await page.getByText("Entries 101-200 of 502", { exact: true }).waitFor();
  const audit = await page.evaluate(() => ({ calls: window.profilerTest.calls.map((call) => call.command), saved: JSON.stringify([window.profilerTest.logs, Object.entries(localStorage), Object.entries(sessionStorage)]) }));
  check(!audit.calls.some((command) => /^(start_fxserver|restart_fxserver|stop_fxserver|send_fxserver_command|send_fxserver_rcon_command|read_text_file)$/.test(command)), "Used unrestricted server/file IPC");
  check(!audit.saved.includes("fixture-rcon-secret") && !audit.saved.includes('"traceEvents"'), "Secret or raw trace was persisted");
  check(errors.length === 0, errors.join("\n"));
  return "PASS (mock IPC): bounded capture, background analysis, auto-open, full comparison, worker cancellation/disposal/errors, paged rendering, two-capture retention, desktop/mobile layout; no live server.";
}
