async (page) => {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.route("https://raw.githubusercontent.com/**", (route) => route.fulfill({ json: { version: "0.3.2" } }));
  await page.route("**/jsonv2", (route) => route.fulfill({ json: { recommendedArtifact: "10000", windowsDownloadLink: "https://example.invalid/artifact.zip", brokenArtifacts: [] } }));
  await page.addInitScript(() => {
    localStorage.removeItem("fxserver-installer.workspaces.v1");
    localStorage.setItem("installPath", "C:/mock/artifacts");
    const callbacks = new Map();
    const events = new Map();
    const state = window.testDesktop = { running: false, pending: {}, calls: [], logs: [], unknown: [], counter: 0 };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: (name) => events.delete(name) };
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
      transformCallback: (callback) => { callbacks.set(++state.counter, callback); return state.counter; },
      invoke: async (command, args = {}) => {
        state.calls.push(command);
        switch (command) {
          case "plugin:window|title": return "FXServer Installer";
          case "plugin:app|version": return "0.3.2";
          case "plugin:event|listen": events.set(args.event, args.handler); return args.handler;
          case "plugin:event|unlisten": return;
          case "read_app_logs": return { path: "mock.log", entries: state.logs };
          case "append_app_log": state.logs.push(args.entry); return;
          case "get_fxserver_rcon_password": return "mock-password";
          case "save_fxserver_rcon_password": return;
          case "initialize_health_workspace": return;
          case "configure_live_bridge": return { workspaceId: args.target.workspaceId, enabled: false, connected: false, snapshot: null };
          case "run_fxserver_preflight": return { checkedAt: Date.now(), blocking: false, errorCount: 0, warningCount: 0, resourceCount: 0, configCount: 0, checks: [] };
          case "get_windows_artifact_metadata": return { recommendedArtifact: "10000", windowsDownloadLink: "https://example.invalid/artifact.zip", brokenArtifacts: [] };
          case "get_installed_windows_artifact_info": return { installed: true, version: "10000", destination: "C:/mock/artifacts", markerPath: "", hasFxserverExecutable: true, detectionSource: "marker" };
          case "get_fxserver_status": return { running: state.running, pid: 123, startedAt: "2026-09-05T10:00:00Z", resources: { cpuPercent: 15, memoryBytes: 104857600, totalMemoryBytes: 1073741824, memoryPercent: 10, threadCount: 30, handleCount: 150 } };
          case "get_fxserver_terminal": return { entries: args.afterId == null ? Array.from({ length: 1000 }, (_, id) => ({ id, stream: "stdout", timestamp: "12:00:00", line: `[script:test] Console output ${id}`, plainLine: `[script:test] Console output ${id}`, segments: [{ text: "[script:test]", color: "#22c55e", emphasis: true }, { text: ` Console output ${id}` }] })) : [] };
          case "get_mariadb_status": return { installed: false, running: false, version: null, serviceName: null, installPath: null };
          case "get_mariadb_package_info": return { latestVersion: "12.3.3", installedPackageVersion: null, updateAvailable: false };
          case "start_fxserver":
          case "stop_fxserver":
          case "restart_fxserver":
          case "send_fxserver_command":
          case "install_mariadb":
            return new Promise((resolve, reject) => { state.pending[command] = { resolve, reject }; });
          default: state.unknown.push(command); throw new Error(`Unmocked desktop command: ${command}`);
        }
      },
    };
    state.complete = (command, failure) => {
      if (!failure && command !== "send_fxserver_command" && command !== "install_mariadb") state.running = command !== "stop_fxserver";
      const pending = state.pending[command];
      delete state.pending[command];
      if (failure) pending.reject(failure);
      else pending.resolve(command === "send_fxserver_command" ? "command ran" : command === "install_mariadb" ? "Installation completed." : { pid: 123, artifactPath: "C:/mock/artifacts", startedAt: "2026-09-05T10:00:00Z" });
    };
    state.progress = (message) => callbacks.get(events.get("mariadb-progress"))({ payload: message });
  });
  await page.reload({ waitUntil: "domcontentloaded", timeout: 120000 });
  const nav = page.getByRole("navigation", { name: "Workspace navigation" });
  const manage = async () => {
    const parent = nav.getByTitle("FXServer", { exact: true });
    if (await parent.getAttribute("aria-expanded") !== "true") await parent.click();
    await nav.getByTitle("Manage Server", { exact: true }).click();
    await page.getByText("Performance", { exact: true }).waitFor();
  };
  const home = async () => {
    await nav.getByTitle("Home", { exact: true }).click();
    await page.getByText("Project", { exact: true }).waitFor();
  };
  const db = async () => {
    const parent = nav.getByTitle("MariaDB", { exact: true });
    if (await parent.getAttribute("aria-expanded") !== "true") await parent.click();
    await nav.getByTitle("Manage MariaDB", { exact: true }).click();
    await page.getByText("Install Configuration", { exact: true }).waitFor();
  };
  const waitPending = (command) => page.waitForFunction((name) => !!window.testDesktop.pending[name], command);
  const complete = (command, failure) => page.evaluate(([name, error]) => window.testDesktop.complete(name, error), [command, failure]);
  const assertDisabled = async (name) => {
    if (!await page.getByRole("button", { name, exact: true }).isDisabled()) throw new Error(`${name} was not disabled during the pending action`);
  };

  await manage();
  await page.evaluate(() => {
    window.uiLongTasks = [];
    new PerformanceObserver((list) => window.uiLongTasks.push(...list.getEntries().map((entry) => entry.duration))).observe({ type: "longtask" });
  });
  for (const [label, command] of [["Start", "start_fxserver"], ["Restart", "restart_fxserver"], ["Stop", "stop_fxserver"]]) {
    await page.getByRole("button", { name: label, exact: true }).click();
    await waitPending(command);
    const started = Date.now();
    await home();
    await db();
    await manage();
    await assertDisabled("Start");
    await assertDisabled("Restart");
    await assertDisabled("Stop");
    if (await page.evaluate((name) => window.testDesktop.calls.filter((call) => call === name).length, command) !== 1) throw new Error("Duplicate lifecycle request");
    const navigationTime = Date.now() - started;
    if (navigationTime > 5000) throw new Error(`${label}: navigation took ${navigationTime} ms during background work`);
    console.log(`${label}: navigation remained available during pending work (${navigationTime} ms round trip)`);
    await complete(command);
    await page.waitForFunction(() => !Object.keys(window.testDesktop.pending).length);
  }
  await page.getByRole("button", { name: "Start", exact: true }).click();
  await waitPending("start_fxserver");
  await complete("start_fxserver");
  const input = page.getByTitle("Command to send to FXServer through RCON", { exact: true });
  await input.fill("status");
  await input.press("Enter");
  await waitPending("send_fxserver_command");
  await home();
  await manage();
  await page.getByRole("button", { name: "Sending", exact: true }).waitFor();
  await complete("send_fxserver_command", "Mock RCON failure");
  await page.getByText("Mock RCON failure", { exact: true }).waitFor();
  if (await input.inputValue() !== "status") throw new Error("Failed RCON command was not restored");
  await input.press("Enter");
  await waitPending("send_fxserver_command");
  await complete("send_fxserver_command");
  await page.getByRole("button", { name: "Send", exact: true }).waitFor();
  await page.screenshot({ path: "output/playwright/manage-server.png", fullPage: true });

  await db();
  await page.getByPlaceholder("Required root password").fill("mock-only");
  await page.getByRole("button", { name: "Install", exact: true }).click();
  await waitPending("install_mariadb");
  await home();
  await page.evaluate(() => window.testDesktop.progress("Verifying the test installer checksum."));
  await db();
  await page.getByText("Verifying the test installer checksum.", { exact: true }).first().waitFor();
  await assertDisabled("Install");
  await complete("install_mariadb");
  await page.waitForFunction(() => window.testDesktop.logs.some((line) => line.includes("Verifying the test installer checksum.")));
  if (errors.length) throw new Error(errors.join("\n"));
  const unknown = await page.evaluate(() => window.testDesktop.unknown);
  if (unknown.length) throw new Error(`Missing desktop mocks: ${unknown.join(", ")}`);
  const longestTask = await page.evaluate(() => Math.max(0, ...window.uiLongTasks));
  if (longestTask >= 500) throw new Error(`UI stalled for ${longestTask} ms`);
  console.log(`Longest UI task: ${longestTask} ms (500 ms regression ceiling)`);
  console.log("PASS: start/restart/stop, RCON success/failure, 1000 console entries, MariaDB progress across navigation; no page errors.");
}
