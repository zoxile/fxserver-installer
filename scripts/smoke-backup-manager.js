async (page) => {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.route("https://raw.githubusercontent.com/**", (route) => route.fulfill({ json: { version: "0.3.2" } }));
  await page.route("**/jsonv2", (route) => route.fulfill({ json: { recommended: "10000", latest: "10000" } }));
  await page.addInitScript(() => {
    localStorage.clear();
    const callbacks = new Map();
    const events = new Map();
    let sequence = 0;
    const state = window.testBackups = { schedules: [], snapshots: [], calls: [], pending: null, busy: false, unknown: [] };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
      transformCallback: (callback) => { callbacks.set(++sequence, callback); return sequence; },
      invoke: async (command, args = {}) => {
        state.calls.push(command);
        switch (command) {
          case "plugin:window|title": return "FXServer Installer";
          case "plugin:app|version": return "0.3.2";
          case "plugin:event|listen": events.set(args.event, args.handler); return args.handler;
          case "plugin:event|unlisten": return;
          case "read_app_logs": return { path: "mock.log", entries: [] };
          case "append_app_log": return;
          case "initialize_health_workspace": return;
          case "get_windows_artifact_metadata": return { recommendedArtifact: "10000", windowsDownloadLink: "https://example.invalid/artifact.zip", brokenArtifacts: [] };
          case "get_installed_windows_artifact_info": return { installed: false };
          case "validate_mariadb_credentials": return;
          case "list_mariadb_databases": return ["mysql", "information_schema", "qbx", "qb"];
          case "get_backup_manager": return structuredClone({ schedules: state.schedules, snapshots: state.snapshots, busy: state.busy });
          case "save_backup_schedule":
            state.schedules = [{ config: structuredClone(args.config), enabled: args.enabled, running: false, nextRun: args.enabled ? Date.now() + 3600000 : null, lastRun: null, lastError: null }];
            return;
          case "remove_backup_schedule": state.schedules = []; return;
          case "run_scheduled_backup_now":
          case "restore_backup_snapshot":
            state.busy = true;
            return new Promise((resolve) => { state.pending = { command, resolve }; });
          case "preview_backup_restore": return {
            token: "mock-preview", snapshot: state.snapshots[0], targetHost: "localhost", targetPort: 3306,
            targetDatabase: "qbx", existingTables: 12, expiresAt: Date.now() + 300000,
            warnings: ["Existing tables and data can be replaced.", "A recovery backup must succeed before restoring."],
          };
          default: state.unknown.push(command); throw new Error(`Unmocked command: ${command}`);
        }
      },
    };
    state.finish = () => {
      const snapshot = {
        id: `mock-${state.snapshots.length}`, workspaceId: "default", scheduleId: state.schedules[0].config.id,
        database: "qbx", directory: "C:/mock/backups/fxserver-managed-backups/default/daily",
        createdAt: Date.now(), sizeBytes: 1024 * 1024, sha256: "mock-sha256", kind: "manual", sourceHost: "localhost", sourcePort: 3306,
      };
      state.snapshots.unshift(snapshot);
      state.busy = false;
      const pending = state.pending;
      state.pending = null;
      pending.resolve(pending.command === "restore_backup_snapshot" ? { recoverySnapshot: { ...snapshot, kind: "recovery" }, message: "Database restore completed." } : snapshot);
    };
  });
  await page.reload({ waitUntil: "domcontentloaded", timeout: 120000 });
  const nav = page.getByRole("navigation", { name: "Workspace navigation" });
  const openBackups = async () => {
    const parent = nav.getByTitle("MariaDB", { exact: true });
    if (await parent.getAttribute("aria-expanded") !== "true") await parent.click();
    await nav.getByTitle("Backups & Restore", { exact: true }).click();
    await page.getByRole("heading", { name: /^(Backups & Restore|Backup Manager)$/ }).waitFor();
  };
  const home = async () => {
    await nav.getByTitle("Home", { exact: true }).click();
    await page.getByText("Project", { exact: true }).waitFor();
  };
  await openBackups();
  await page.getByRole("button", { name: "Change Credentials", exact: true }).click();
  await page.getByText("Credentials validated.", { exact: true }).waitFor();
  await page.getByRole("textbox", { name: "Name", exact: true }).fill("Hourly test");
  await page.getByRole("textbox", { name: "Backup folder", exact: true }).fill("C:/mock/backups");
  await page.getByRole("button", { name: "Save Schedule", exact: true }).click();
  await page.getByText("Schedule saved and paused.", { exact: true }).waitFor();
  if (await page.evaluate(() => window.testBackups.schedules[0].enabled)) throw new Error("New schedules must default to paused");
  await page.getByRole("button", { name: "Enable schedule", exact: true }).click();
  await page.getByRole("button", { name: "Pause schedule", exact: true }).waitFor();
  await page.getByRole("button", { name: "Back Up", exact: true }).click();
  await page.waitForFunction(() => window.testBackups.pending?.command === "run_scheduled_backup_now");
  await home();
  await openBackups();
  if (!await page.getByRole("button", { name: "Back Up", exact: true }).isDisabled()) throw new Error("Background backup did not disable duplicate actions");
  await page.evaluate(() => window.testBackups.finish());
  await page.getByRole("button", { name: "Refresh backups", exact: true }).click();
  await page.getByRole("button", { name: "Review Restore", exact: true }).click();
  await page.getByRole("heading", { name: "Restore Preview", exact: true }).waitFor();
  const restore = page.getByRole("button", { name: "Back Up & Restore", exact: true });
  if (!await restore.isDisabled()) throw new Error("Restore was enabled before typed confirmation");
  await page.getByRole("textbox", { name: "Confirm database name: qbx", exact: true }).fill("qb");
  if (!await restore.isDisabled()) throw new Error("Restore accepted a different database name");
  await page.getByRole("textbox", { name: "Confirm database name: qbx", exact: true }).fill("qbx");
  await page.screenshot({ path: "output/playwright/backups-desktop.png", fullPage: true });
  await page.setViewportSize({ width: 1000, height: 900 });
  await page.screenshot({ path: "output/playwright/backups-narrow.png", fullPage: true });
  await restore.click();
  await page.waitForFunction(() => window.testBackups.pending?.command === "restore_backup_snapshot");
  await home();
  await page.evaluate(() => window.testBackups.finish());
  await openBackups();
  await page.getByRole("button", { name: "Review Restore", exact: true }).first().waitFor();
  if (errors.length) throw new Error(errors.join("\n"));
  const unknown = await page.evaluate(() => window.testBackups.unknown);
  if (unknown.length) throw new Error(`Missing mocks: ${unknown.join(", ")}`);
  console.log("PASS: paused-by-default schedules, enabling, background navigation, snapshot preview, exact confirmation, restore navigation, desktop/narrow layouts.");
}
