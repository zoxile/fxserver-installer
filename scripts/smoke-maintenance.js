async (page) => {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.addInitScript(() => {
    localStorage.clear();
    localStorage.setItem("installPath", "C:/mock/artifacts");
    localStorage.setItem("fxserver.manage.env", JSON.stringify({ TXHOST_DATA_PATH: "C:/mock/txData" }));
    localStorage.setItem("fxserver.manage.serverProfile", "default");
    const state = window.maintenanceTest = { calls: [], running: true, pendingBackup: null, failBackup: true, pid: 101 };
    const config = { id: "daily", workspaceId: "default", name: "Daily Qbox", database: "qbox", outputDir: "C:/mock/backups", intervalMinutes: 60, retainCount: 5 };
    let callback = 0;
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
      transformCallback: () => ++callback,
      invoke: async (command, args = {}) => {
        state.calls.push(command);
        switch (command) {
          case "get_platform_info": return { os: "windows", arch: "x86_64", artifactPlatform: "windows", clientLogsSupported: true, secureStorageSupported: true };
          case "plugin:window|title": return "FXServer Installer";
          case "plugin:app|version": return "0.5.1";
          case "plugin:event|listen": return ++callback;
          case "plugin:event|unlisten": case "append_app_log": case "initialize_health_workspace": case "save_database_login": return;
          case "fetch_latest_app_release": throw new Error("Offline fixture");
          case "read_app_logs": return { path: "mock.log", entries: [] };
          case "get_installed_windows_artifact_info": return { installed: true, version: "10000", destination: "C:/mock/artifacts", hasFxserverExecutable: true };
          case "get_windows_artifact_metadata": return { recommendedArtifact: "10000", brokenArtifacts: [] };
          case "configure_live_bridge": case "get_live_bridge_status": return { workspaceId: "default", enabled: false, connected: false, snapshot: null };
          case "load_database_login": return { host: "localhost", port: 3306, username: "root", password: "fixture-secret", database: "qbox" };
          case "validate_mariadb_credentials": return;
          case "list_mariadb_databases": return ["qbox"];
          case "list_mariadb_tables": return ["players"];
          case "get_mariadb_status": return { installed: true, running: true, version: "11.4", installPath: "C:/mock/mariadb" };
          case "get_backup_manager": return { schedules: [{ config, enabled: false, running: false, nextRun: null, lastRun: null, lastError: null }], snapshots: [], busy: false };
          case "get_fxserver_status": return { running: state.running, pid: state.running ? state.pid : null, artifactPath: "C:/mock/artifacts", startedAt: "1", resources: null };
          case "prepare_fxserver_maintenance_launch": return `ticket-${++callback}`;
          case "reserve_maintenance_backups": case "renew_maintenance_backup_reservation": return { token: "backup-fixture", expiresAt: Date.now() + 120000 };
          case "release_maintenance_backup_reservation": return;
          case "validate_fxserver_maintenance_launch": case "discard_fxserver_maintenance_launch": return;
          case "stop_fxserver_for_maintenance": state.running = false; return;
          case "start_fxserver_after_maintenance": state.running = true; state.pid++; return { pid: state.pid, artifactPath: "C:/mock/artifacts", startedAt: "2" };
          case "run_scheduled_backup_now": {
            if (args.reservationToken !== "backup-fixture") throw new Error("Missing backup reservation");
            return new Promise((resolve, reject) => { state.pendingBackup = () => state.failBackup ? reject(new Error("Backup fixture failed")) : resolve({ id: "snapshot", workspaceId: "default", database: "qbox" }); });
          }
          case "run_fxserver_preflight": return { blocking: false, errorCount: 0, warningCount: 0, checks: [] };
          case "list_txdata_profiles": return { dataPath: "C:/mock/txData", profiles: ["default"], hasRootLogs: false };
          case "get_fxserver_rcon_password": return "fixture-secret";
          case "get_fxserver_terminal": return { entries: [] };
          default: throw new Error(`Unexpected fixture command: ${command}`);
        }
      },
    };
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  const nav = page.getByRole("navigation", { name: "Workspace navigation" });
  async function navigate(title, section) {
    const parent = nav.getByTitle(section, { exact: true });
    if (await parent.getAttribute("aria-expanded") !== "true") await parent.click();
    await nav.getByTitle(title, { exact: true }).click();
  }
  await navigate("Queries & Files", "MariaDB");
  await page.waitForFunction(() => window.maintenanceTest.calls.includes("validate_mariadb_credentials"));
  await navigate("Maintenance Plans", "FXServer");
  await page.getByRole("heading", { name: "Maintenance Plans", exact: true }).waitFor();
  for (const width of [1440, 800]) {
    await page.setViewportSize({ width, height: 1050 });
    await page.getByText("Daily Qbox / qbox", { exact: true }).waitFor();
    if (await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth)) throw new Error(`Maintenance overflow at ${width}`);
    await page.screenshot({ path: `output/playwright/maintenance-${width}.png`, fullPage: true });
  }
  await page.getByRole("checkbox").first().check();
  await page.getByRole("button", { name: "Review plan", exact: true }).click();
  await page.getByRole("heading", { name: "Ready for review" }).waitFor();
  const run = page.getByRole("button", { name: "Run reviewed maintenance" });
  if (await run.isEnabled()) throw new Error("Maintenance bypassed confirmation");
  await page.getByRole("checkbox", { name: /I reviewed the database selection/ }).check();
  await page.getByRole("button", { name: "Review plan", exact: true }).click();
  await page.getByRole("heading", { name: "Ready for review" }).waitFor();
  if (await run.isEnabled()) throw new Error("A replacement review reused an earlier confirmation");
  await nav.getByTitle("Home", { exact: true }).click();
  await page.getByRole("heading", { name: /FXServer setup/ }).waitFor();
  await navigate("Maintenance Plans", "FXServer");
  await page.getByText("Daily Qbox / qbox", { exact: true }).waitFor();
  if (await page.getByRole("heading", { name: "Ready for review" }).count()) throw new Error("A review survived navigation while its visible selections reset");
  await page.getByRole("checkbox").first().check();
  await page.getByRole("button", { name: "Review plan", exact: true }).click();
  await page.getByRole("heading", { name: "Ready for review" }).waitFor();
  await page.getByRole("checkbox", { name: /I reviewed the database selection/ }).check();
  await run.click();
  await page.waitForFunction(() => typeof window.maintenanceTest.pendingBackup === "function");
  await nav.getByTitle("Home", { exact: true }).click();
  await page.getByRole("heading", { name: /FXServer setup/ }).waitFor();
  await navigate("Maintenance Plans", "FXServer");
  await page.getByRole("heading", { name: "Progress / running" }).waitFor();
  await page.evaluate(() => window.maintenanceTest.pendingBackup());
  await page.getByRole("heading", { name: "Progress / paused" }).waitFor();
  await page.getByText("Backup fixture failed", { exact: true }).waitFor();
  const calls = await page.evaluate(() => window.maintenanceTest.calls);
  if (calls.includes("start_fxserver_after_maintenance") || calls.includes("apply_resource_update")) throw new Error("Maintenance continued after backup failure");
  if (!calls.includes("discard_fxserver_maintenance_launch")) throw new Error("Maintenance kept its launch ticket after failure");
  if (!calls.includes("release_maintenance_backup_reservation")) throw new Error("Maintenance kept its backup reservation after failure");
  if (errors.length) throw new Error(errors.join("\n"));
  return "Maintenance: explicit confirmation, narrow/wide layout, background navigation, retained progress, backup failure stops restart.";
}
