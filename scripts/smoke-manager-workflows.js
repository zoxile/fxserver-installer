async (page) => {
  const errors = [];
  await page.evaluate(() => sessionStorage.removeItem("manager-seeded"));
  page.on("pageerror", (error) => errors.push(error.message));
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.route("https://raw.githubusercontent.com/**", (route) => route.fulfill({ json: { version: "0.3.2" } }));
  await page.route("**/jsonv2", (route) => route.fulfill({ json: { recommendedArtifact: "10000", windowsDownloadLink: "https://example.invalid/artifact.zip", brokenArtifacts: [] } }));
  await page.addInitScript(() => {
    if (!sessionStorage.getItem("manager-seeded")) {
      localStorage.clear();
      localStorage.setItem("installPath", "C:/mock/artifacts");
      sessionStorage.setItem("manager-seeded", "1");
    }
    const callbacks = new Map();
    const events = new Map();
    const state = window.managerTest = {
      running: false, pending: {}, calls: [], unknown: [], logs: [], schedules: [], counter: 0, blocked: false, healthSample: null,
      passwords: JSON.parse(sessionStorage.getItem("passwords") || '{"default":"default-secret"}'),
      health: { alertsEnabled: false, recoveryEnabled: false, cpuThresholdPercent: 90, memoryThresholdPercent: 80, minimumFreeDiskGb: 5, diskPath: "", sustainedSeconds: 15, alertCooldownSeconds: 300, recoveryBackoffSeconds: 30 },
    };
    const report = () => ({ checkedAt: Math.floor(Date.now() / 1000), blocking: state.blocked, errorCount: Number(state.blocked), warningCount: 1, resourceCount: 3, configCount: 1, checks: [
      { category: "rcon", code: "rcon.missing", severity: "warning", title: "RCON password missing", detail: "Add rcon_password to server.cfg.", resource: null, file: "server.cfg", line: null },
      ...(state.blocked ? [{ category: "paths", code: "artifact.missing", severity: "error", title: "Artifact missing", detail: "Choose an installed artifact.", resource: null, file: null, line: null }] : []),
    ] });
    const healthStatus = () => ({ workspaceId: state.workspaceId || "default", config: state.health, sample: state.healthSample, events: [], recoveryArmed: false, recoveryBlocked: false, recoveryAttempts: 0, nextRecoverySeconds: null });
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
      transformCallback: (callback) => { const id = ++state.counter; callbacks.set(id, callback); return id; },
      invoke: async (command, args = {}) => {
        state.calls.push(command);
        switch (command) {
          case "fetch_latest_app_release": return { version: "0.3.2", tagName: "v0.3.2", htmlUrl: "https://github.com/zoxile/fxserver-installer/releases/tag/v0.3.2", installerUrl: "https://github.com/zoxile/fxserver-installer/releases/download/v0.3.2/FXServer.Installer_0.3.2_windows_x64-setup.exe" };
          case "plugin:window|title": return "FXServer Installer";
          case "plugin:app|version": return "0.3.2";
          case "plugin:event|listen": { const ids = events.get(args.event) || []; ids.push(args.handler); events.set(args.event, ids); return args.handler; }
          case "plugin:event|unlisten": return;
          case "read_app_logs": return { path: "mock.log", entries: state.logs };
          case "append_app_log": state.logs.push(args.entry); return;
          case "get_fxserver_rcon_password": return state.passwords[args.workspaceId] || "";
          case "save_fxserver_rcon_password": state.passwords[args.workspaceId] = args.password; sessionStorage.setItem("passwords", JSON.stringify(state.passwords)); return;
          case "clear_fxserver_rcon_password": delete state.passwords[args.workspaceId]; sessionStorage.setItem("passwords", JSON.stringify(state.passwords)); return;
          case "initialize_health_workspace": state.workspaceId = args.workspaceId; return;
          case "load_database_login": return null;
          case "clear_database_login": return;
          case "configure_live_bridge": return { workspaceId: args.target.workspaceId, enabled: false, connected: false, snapshot: null };
          case "prepare_workspace_switch": if (state.running) throw new Error("Stop FXServer before switching workspaces."); state.workspaceId = args.workspaceId; return;
          case "get_windows_artifact_metadata": return { recommendedArtifact: "10000", windowsDownloadLink: "https://example.invalid/artifact.zip", brokenArtifacts: [] };
          case "get_installed_windows_artifact_info": return { installed: true, version: "10000", destination: args.destination, markerPath: "", hasFxserverExecutable: true, detectionSource: "marker" };
          case "list_txdata_profiles": return { dataPath: args.dataPath, profiles: ["default"], hasRootLogs: false };
          case "get_fxserver_status": return { running: state.running, pid: state.running ? 123 : null, startedAt: String(Math.floor(Date.now() / 1000)), resources: null };
          case "get_fxserver_terminal": return { entries: [] };
          case "run_fxserver_preflight": return report();
          case "start_fxserver": return new Promise((resolve) => { state.pending.start = () => { state.running = true; delete state.pending.start; resolve({ pid: 123, artifactPath: args.request.artifactPath, startedAt: String(Math.floor(Date.now() / 1000)) }); }; });
          case "stop_fxserver": state.running = false; return;
          case "get_health_status":
            if (state.holdHealthStatus) return new Promise((resolve) => {
              state.pending.healthStatus = () => { delete state.pending.healthStatus; resolve(healthStatus()); };
            });
            return healthStatus();
          case "configure_health": {
            const apply = () => { state.health = args.config; state.healthSample = null; return healthStatus(); };
            if (state.holdHealthConfig) return new Promise((resolve) => {
              state.pending.healthConfig = () => { delete state.pending.healthConfig; resolve(apply()); };
            });
            return apply();
          }
          case "get_backup_manager": return { schedules: state.schedules.filter((item) => item.config.workspaceId === args.workspaceId), snapshots: [], restoreTests: [], busy: false };
          case "remove_backup_schedule": state.schedules = state.schedules.filter((item) => item.config.id !== args.scheduleId || item.config.workspaceId !== args.workspaceId); return;
          case "preview_diagnostic_export": return { id: "preview", createdAt: Math.floor(Date.now() / 1000), expiresAt: Math.floor(Date.now() / 1000) + 900, entries: [{ name: "manifest.json", content: '{"appVersion":"0.3.2","rcon_password":"[redacted]"}' }], totalBytes: 64 };
          default: state.unknown.push(command); throw new Error(`Unmocked command: ${command}`);
        }
      },
    };
    state.emit = (name, payload) => (events.get(name) || []).forEach((id) => callbacks.get(id)?.({ payload }));
  });
  await page.reload({ waitUntil: "domcontentloaded", timeout: 120000 });
  const nav = page.getByRole("navigation", { name: "Workspace navigation" });
  const navigate = async (title, parent) => {
    if (parent) {
      const toggle = nav.getByTitle(parent, { exact: true });
      if (await toggle.getAttribute("aria-expanded") !== "true") await toggle.click();
    }
    await nav.getByTitle(title, { exact: true }).click();
  };
  await navigate("Workspaces");
  await page.getByRole("button", { name: "New workspace", exact: true }).click();
  await page.getByRole("textbox", { name: "Name", exact: true }).fill("QA");
  await page.getByRole("textbox", { name: /^Artifact folder/ }).fill("C:/mock/qa");
  await page.getByRole("button", { name: "Save workspace", exact: true }).click();
  await page.getByText("Workspace saved.", { exact: true }).waitFor();
  await page.evaluate(() => { window.managerTest.running = true; });
  await page.getByRole("button", { name: "Switch", exact: true }).last().click();
  await page.getByText("Stop FXServer before switching workspaces.", { exact: true }).waitFor();
  await page.evaluate(() => { window.managerTest.running = false; });
  await page.getByRole("button", { name: "Switch", exact: true }).last().click();
  await page.waitForFunction(() => JSON.parse(localStorage.getItem("fxserver-installer.workspaces.v1")).activeId !== "default");
  const qa = await page.evaluate(() => JSON.parse(localStorage.getItem("fxserver-installer.workspaces.v1")).activeId);
  await navigate("Manage Server", "FXServer");
  const password = page.getByTitle("Value configured with rcon_password in server.cfg.", { exact: true });
  await password.waitFor();
  if (await password.inputValue()) throw new Error("RCON password leaked from Default to QA");
  await password.fill("qa-secret");
  await password.blur();
  await page.waitForFunction((id) => window.managerTest.passwords[id] === "qa-secret", qa);
  await page.evaluate(() => { window.managerTest.blocked = true; });
  await page.getByRole("button", { name: "Start", exact: true }).click();
  await page.getByText("Preflight found blocking issues. Review the checks before starting FXServer.", { exact: false }).waitFor();
  if (await page.evaluate(() => window.managerTest.calls.includes("start_fxserver"))) throw new Error("Preflight did not block launch");
  await page.evaluate(() => { window.managerTest.blocked = false; });
  await page.getByRole("button", { name: "Start", exact: true }).click();
  await page.waitForFunction(() => !!window.managerTest.pending.start);
  await page.getByTitle(/^Task Center/).click();
  await page.getByRole("heading", { name: "Task Center", exact: true }).waitFor();
  await page.getByRole("button", { name: "Filter tasks", exact: true }).click();
  await page.getByRole("option", { name: "Running", exact: true }).click();
  await page.getByText("Start FXServer", { exact: true }).first().waitFor();
  await navigate("Workspaces");
  await page.getByRole("button", { name: "Switch", exact: true }).first().click();
  await page.getByText("Wait for background tasks to finish before switching workspaces.", { exact: true }).waitFor();
  await page.evaluate(() => window.managerTest.pending.start());
  await navigate("Manage Server", "FXServer");
  await page.getByRole("button", { name: "Stop", exact: true }).click();
  await page.waitForFunction(() => !window.managerTest.running);
  await navigate("Workspaces");
  await page.getByRole("button", { name: "Switch", exact: true }).first().click();
  await page.waitForFunction(() => JSON.parse(localStorage.getItem("fxserver-installer.workspaces.v1")).activeId === "default");
  await navigate("Manage Server", "FXServer");
  await page.waitForFunction(() => document.querySelector('[title="Value configured with rcon_password in server.cfg."]')?.value === "default-secret");
  const stored = await page.evaluate(() => localStorage.getItem("fxserver-installer.workspaces.v1"));
  if (stored.includes("secret") || stored.includes("password")) throw new Error("Workspace settings contain a password");
  await navigate("Health & Recovery", "FXServer");
  await page.getByRole("heading", { name: "Health & Recovery", exact: true }).waitFor();
  if (await page.getByRole("checkbox", { checked: true }).count()) throw new Error("Health automation is on by default");
  await page.evaluate(() => {
    window.managerTest.running = true;
    window.managerTest.healthSample = { timestamp: Date.now(), running: true, pid: 123, cpuPercent: 12.5, memoryPercent: 24, freeDiskGb: 42, diskPath: "C:/mock/artifacts" };
  });
  await page.getByText("12.5%", { exact: true }).waitFor();
  await page.getByText("24.0%", { exact: true }).waitFor();
  await page.getByText("42.0 GiB", { exact: true }).waitFor();
  await page.getByText("Running (123)", { exact: true }).waitFor();
  if (await page.getByRole("checkbox", { checked: true }).count()) throw new Error("Passive readings enabled health automation");
  if (await page.evaluate(() => window.managerTest.calls.includes("configure_health"))) throw new Error("Viewing readings silently changed health settings");
  await page.screenshot({ path: "output/playwright/health-desktop.png", fullPage: true });
  await page.setViewportSize({ width: 480, height: 800 });
  await page.screenshot({ path: "output/playwright/health-narrow.png", fullPage: true });
  if (await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)) throw new Error("Health readings overflow at narrow width");
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.evaluate(() => { window.managerTest.healthSample = { timestamp: Date.now(), running: true, pid: 123, cpuPercent: null, memoryPercent: null, freeDiskGb: 42, processError: "Mock process metrics unavailable" }; });
  await page.getByText("Mock process metrics unavailable", { exact: true }).waitFor();
  if (await page.getByText("Unavailable", { exact: true }).count() !== 2) throw new Error("Missing metrics were not distinguished from a stopped server");
  await page.getByText("Running (123)", { exact: true }).waitFor();
  await page.evaluate(() => { window.managerTest.running = false; window.managerTest.healthSample = { timestamp: Date.now(), running: false, pid: null, cpuPercent: null, memoryPercent: null, freeDiskGb: 42 }; });
  await page.getByText("Stopped", { exact: true }).first().waitFor();
  if (await page.getByText("12.5%", { exact: true }).count()) throw new Error("Stopped server retained a stale CPU reading");

  const freshHealthSample = () => page.evaluate(() => {
    window.managerTest.running = true;
    window.managerTest.healthSample = { timestamp: Date.now(), running: true, pid: 123, cpuPercent: 12.5, memoryPercent: 24, freeDiskGb: 42, diskPath: "C:/mock/artifacts" };
  });
  const refreshHealth = page.getByRole("button", { name: "Refresh health status", exact: true });
  const applyHealth = page.getByRole("button", { name: "Apply Settings", exact: true });
  for (const pending of ["healthStatus", "healthConfig"]) {
    await freshHealthSample();
    await refreshHealth.click();
    await page.getByText("12.5%", { exact: true }).waitFor();
    await page.evaluate((pending) => {
      window.managerTest.holdHealthStatus = pending === "healthStatus";
      window.managerTest.holdHealthConfig = pending === "healthConfig";
    }, pending);
    if (pending === "healthStatus") {
      await refreshHealth.click();
    } else {
      await page.getByRole("checkbox", { name: "Enable health alerts", exact: true }).check();
      await applyHealth.click();
    }
    await page.waitForFunction((pending) => !!window.managerTest.pending[pending], pending);
    const polls = await page.evaluate(() => window.managerTest.calls.filter((command) => command === "get_health_status").length);
    await page.getByText("Health sample is out of date.", { exact: true }).waitFor({ timeout: 35_000 });
    if (await page.getByText("Unavailable", { exact: true }).count() !== 4) throw new Error(`${pending}: stale metrics or process status remained visible`);
    if (await page.getByText("12.5%", { exact: true }).count()) throw new Error(`${pending}: stale CPU remained visible`);
    if (await page.evaluate(() => Date.now() - window.managerTest.healthSample.timestamp) <= 20_000) throw new Error("Stale regression did not cross the actual stale window");
    if (await page.evaluate(() => window.managerTest.calls.filter((command) => command === "get_health_status").length) !== polls) throw new Error(`${pending}: overlapping health polls were issued`);
    await page.screenshot({ path: `output/playwright/health-pending-${pending}.png`, fullPage: true });
    await page.evaluate((pending) => {
      window.managerTest.holdHealthStatus = false;
      window.managerTest.holdHealthConfig = false;
      window.managerTest.pending[pending]();
    }, pending);
    await freshHealthSample();
    await refreshHealth.click();
    await page.getByText("12.5%", { exact: true }).waitFor();
    if (await page.getByText("Health sample is out of date.", { exact: true }).count()) throw new Error(`${pending}: freshness did not recover after IPC completed`);
  }

  const vanishedDisk = "C:/mock/vanished-health-folder";
  await page.getByRole("checkbox", { name: "Enable health alerts", exact: true }).check();
  await page.getByRole("checkbox", { name: "Restart after an unexpected exit", exact: true }).check();
  await page.getByRole("textbox", { name: "Disk folder", exact: true }).fill(vanishedDisk);
  await applyHealth.click();
  await page.waitForFunction((path) => window.managerTest.health.alertsEnabled && window.managerTest.health.recoveryEnabled && window.managerTest.health.diskPath === path, vanishedDisk);
  await freshHealthSample();
  await page.evaluate(() => {
    Object.assign(window.managerTest.healthSample, { freeDiskGb: null, diskPath: null, diskError: "Mock disk folder disappeared" });
  });
  await refreshHealth.click();
  await page.getByText("Mock disk folder disappeared", { exact: true }).waitFor();
  await page.getByRole("checkbox", { name: "Enable health alerts", exact: true }).uncheck();
  await page.getByRole("checkbox", { name: "Restart after an unexpected exit", exact: true }).uncheck();
  await applyHealth.click();
  await page.waitForFunction(() => !window.managerTest.health.alertsEnabled && !window.managerTest.health.recoveryEnabled);
  if (await page.getByRole("textbox", { name: "Disk folder", exact: true }).inputValue() !== vanishedDisk) throw new Error("Opt-out required clearing the unavailable disk folder");
  if (await page.evaluate(() => window.managerTest.calls.includes("restart_fxserver"))) throw new Error("Health regressions invoked a server restart");
  await page.screenshot({ path: "output/playwright/health-vanished-disk-optout.png", fullPage: true });
  await page.evaluate(() => { window.managerTest.running = false; });
  await navigate("Backups & Restore", "MariaDB");
  await page.getByRole("heading", { name: /^(Backups & Restore|Backup Manager)$/ }).waitFor();
  await page.screenshot({ path: "output/playwright/backups-desktop.png", fullPage: true });
  await navigate("Diagnostics", "FXServer");
  await page.getByRole("button", { name: "Run checks", exact: true }).click();
  await page.getByText("RCON password missing", { exact: true }).waitFor();
  await page.screenshot({ path: "output/playwright/diagnostics-desktop.png", fullPage: true });
  await page.setViewportSize({ width: 760, height: 900 });
  await page.screenshot({ path: "output/playwright/diagnostics-narrow.png", fullPage: true });
  await navigate("Workspaces");
  await page.getByRole("button", { name: "New workspace", exact: true }).click();
  await page.screenshot({ path: "output/playwright/workspaces-narrow.png", fullPage: true });
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth);
  if (overflow) throw new Error("Page has horizontal overflow");
  await page.evaluate(() => {
    window.managerTest.emit("backup-manager-progress", { workspaceId: "default", scheduleId: "hourly", stage: "running", timestamp: Date.now() });
    window.managerTest.emit("background-app-log", { id: "test-alert", timestamp: Date.now(), level: "error", scope: "mariadb.backup", message: "Mock scheduled backup failure" });
    window.managerTest.emit("backup-manager-progress", { workspaceId: "default", scheduleId: "hourly", stage: "error", timestamp: Date.now() });
  });
  await page.getByText("Mock scheduled backup failure", { exact: true }).waitFor();
  await page.getByLabel("Background notifications").getByTitle("Dismiss notification").click();
  await page.getByTitle(/^Task Center/).click();
  await page.getByText("Scheduled database backup", { exact: true }).waitFor();
  await page.screenshot({ path: "output/playwright/tasks-narrow.png", fullPage: true });
  await page.reload({ waitUntil: "domcontentloaded" });
  await navigate("Workspaces");
  await page.getByRole("heading", { name: "QA", exact: true }).waitFor();
  if (await page.evaluate(() => JSON.parse(localStorage.getItem("fxserver-installer.workspaces.v1")).items.length) !== 2) throw new Error("Saved workspaces did not survive reload");
  await page.evaluate((id) => {
    window.managerTest.schedules.push({ config: { id: "qa-backup", workspaceId: id } });
    window.confirm = (message) => { window.managerTest.confirmation = message; return true; };
  }, qa);
  await page.getByRole("button", { name: "Remove QA", exact: true }).click();
  await page.waitForFunction(() => JSON.parse(localStorage.getItem("fxserver-installer.workspaces.v1")).items.length === 1);
  if (await page.evaluate((id) => Boolean(window.managerTest.passwords[id]), qa)) throw new Error("Removed workspace password was not cleared");
  if (await page.evaluate(() => window.managerTest.schedules.length)) throw new Error("Removed workspace backup schedules were not cleared");
  if (!await page.evaluate(() => window.managerTest.confirmation?.includes('Remove saved workspace "QA"'))) throw new Error("Workspace removal did not request confirmation");
  if (errors.length) throw new Error(errors.join("\n"));
  const unknown = await page.evaluate(() => window.managerTest.unknown);
  if (unknown.length) throw new Error(`Missing mocks: ${unknown.join(", ")}`);
  await page.evaluate(() => { window.managerTest.completed = true; });
  console.log("PASS: workspace isolation, running/task switch guards, preflight gate, nonblocking task navigation, passive health polling with automation off, unavailable/stopped readings, backup/diagnostics pages, dismissible background failure, narrow layouts.");
}
