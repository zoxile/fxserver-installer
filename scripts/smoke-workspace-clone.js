async (page) => {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.setViewportSize({ width: 1440, height: 1100 });
  await page.route("https://raw.githubusercontent.com/**", (route) => route.fulfill({ json: { version: "0.3.2" } }));
  await page.addInitScript(() => {
    const qaId = "11111111-1111-4111-8111-111111111111";
    const now = Date.now();
    if (!sessionStorage.getItem("workspace-clone-seeded")) {
      localStorage.clear();
      const workspace = (id, name, path) => ({ id, name, artifactPath: `${path}/artifacts`, txDataPath: `${path}/txData`, profile: "default",
        environment: { TXHOST_DATA_PATH: `${path}/txData`, TXHOST_FXS_PORT: "30120", TXHOST_TXA_PORT: "40120" },
        database: { host: "localhost", port: 3306, username: "root", database: "fixture_source" } });
      localStorage.setItem("fxserver-installer.workspaces.v1", JSON.stringify({ activeId: "default", items: [workspace("default", "Source fixture", "C:/fixture/source"), workspace(qaId, "QA fixture", "C:/fixture/qa")] }));
      localStorage.setItem("fxserver-installer.incidents.v1", JSON.stringify([
        { id: "seed-config", timestamp: now - 300000, workspaceId: "default", type: "config", level: "info", title: "Fixture configuration saved", detail: 'api_key="fixture-seeded-secret"', panel: "server-configure" },
        { id: "seed-qa", timestamp: now - 600000, workspaceId: qaId, type: "health", level: "warn", title: "QA memory threshold", detail: "Fixture threshold", panel: "health" },
        { id: "seed-old", timestamp: now - 3 * 86400000, workspaceId: "default", type: "restart", level: "info", title: "Old fixture restart", detail: "Historical fixture", panel: "server-manage" },
      ]));
      sessionStorage.setItem("workspace-clone-seeded", "1");
    }
    const callbacks = new Map();
    const events = new Map();
    const state = window.workspaceCloneFixture = { calls: [], unknown: [], counter: 0, previews: {}, pending: null, logs: [], failNextExecute: false, deferNextExecute: false };
    state.emit = (name, payload) => (events.get(name) || []).forEach((id) => callbacks.get(id)?.({ payload }));
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
      transformCallback: (callback) => { callbacks.set(++state.counter, callback); return state.counter; },
      invoke: async (command, args = {}) => {
        state.calls.push({ command, args });
        switch (command) {
          case "fetch_latest_app_release": return { version: "0.3.2", tagName: "v0.3.2", htmlUrl: "https://github.com/zoxile/fxserver-installer/releases/tag/v0.3.2", installerUrl: "https://github.com/zoxile/fxserver-installer/releases/download/v0.3.2/FXServer.Installer_0.3.2_windows_x64-setup.exe" };
          case "plugin:window|title": return "FXServer Installer";
          case "plugin:app|version": return "0.3.2";
          case "plugin:event|listen": { const ids = events.get(args.event) || []; ids.push(args.handler); events.set(args.event, ids); return args.handler; }
          case "plugin:event|unlisten": for (const [name, ids] of events) events.set(name, ids.filter((id) => id !== args.eventId)); return;
          case "plugin:dialog|open": return "C:/fixture/reviewed.sql";
          case "initialize_health_workspace": return;
          case "configure_live_bridge": return { workspaceId: args.target.workspaceId, enabled: false, connected: false, snapshot: null };
          case "read_app_logs": return { path: "fixture.log", entries: state.logs };
          case "append_app_log": state.logs.push(args.entry); return;
          case "get_fxserver_rcon_password": return "";
          case "get_windows_artifact_metadata": return { recommendedArtifact: "10000", windowsDownloadLink: "https://example.invalid/artifact.zip", brokenArtifacts: [] };
          case "get_installed_windows_artifact_info": return { installed: true, version: "10000", destination: args.destination, hasFxserverExecutable: true, detectionSource: "marker" };
          case "get_fxserver_status": return { running: false, pid: null, resources: null };
          case "list_workspace_clone_choices": return { sourcePath: args.sourcePath, resources: ["[local]/demo", "[licensed]/private-resource"], configs: ["server.cfg", "permissions.cfg"] };
          case "preview_workspace_clone": {
            const request = args.request;
            if (request.destinationPath.endsWith("/existing")) throw new Error("The destination already exists. Nothing will be overwritten.");
            const id = `preview-${++state.counter}`;
            const database = request.database ? { sourcePath: request.mode === "import" ? `${request.sourcePath}/database.sql` : request.database.dumpPath,
              sourceDatabase: request.database.sourceDatabase || "fixture_source", sizeBytes: 2048, sha256: "b".repeat(64), tableCount: 2,
              target: request.mode === "export" ? null : { database: `fxsi_clone_${String(state.counter).padStart(32, "0")}`, host: request.database.host, port: request.database.port } } : null;
            const preview = { id, sourcePath: request.sourcePath, destinationPath: request.destinationPath, mode: request.mode, serverPort: request.serverPort, txAdminPort: request.txAdminPort,
              files: [{ path: "server-data/server.cfg", size: 100, sha256: "a".repeat(64) }, { path: "server-data/resources/[local]/demo/server.lua", size: 200, sha256: "c".repeat(64) }, { path: "server-data/resources/[local]/demo/LICENSE", size: 300, sha256: "d".repeat(64) }],
              excluded: [{ path: "server-data/resources/[local]/demo/.env", reason: "Known secret file excluded" }, { path: "server-data/server.cfg", reason: "License key, endpoints and external references removed" }, ...(request.mode === "import" && !database ? [{ path: "database.sql", reason: "Database copy was not selected" }] : [])],
              totalBytes: 600 + (database?.sizeBytes || 0), expiresAt: Math.floor(Date.now() / 1000) + 900, database };
            state.previews[id] = { preview, request }; return preview;
          }
          case "discard_workspace_clone_preview": delete state.previews[args.previewId]; return;
          case "execute_workspace_clone": {
            const prepared = state.previews[args.previewId];
            if (!prepared || args.confirmedDestination !== prepared.preview.destinationPath || !args.privateCopyConfirmed) throw new Error("Fixture rejected an unconfirmed clone.");
            const target = prepared.preview.database?.target;
            if (target && (args.confirmedDatabase !== target.database || args.databaseCredentials?.database !== target.database)) throw new Error("Fixture refused source or existing database target.");
            if (!target && args.databaseCredentials) throw new Error("Unexpected database credentials on a file-only operation.");
            delete state.previews[args.previewId];
            if (state.failNextExecute) { state.failNextExecute = false; throw new Error("Fixture copy failed before promotion. No destination was created; new owned database cleaned up."); }
            const root = prepared.preview.destinationPath;
            const result = { destinationPath: root, serverDataPath: `${root}/server-data`, txDataPath: `${root}/txData`, artifactPath: `${root}/artifacts`, fileCount: 3,
              database: target ? { host: target.host, port: target.port, username: args.databaseCredentials.username, database: target.database } : null };
            if (state.deferNextExecute) { state.deferNextExecute = false; return new Promise((resolve) => { state.pending = () => { state.pending = null; resolve(result); }; }); }
            return result;
          }
          default: state.unknown.push(command); throw new Error(`Unmocked IPC command: ${command}`);
        }
      },
    };
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  const nav = page.getByRole("navigation", { name: "Workspace navigation" });
  const main = page.getByRole("main");
  const clone = page.getByRole("region", { name: "Server cloning and migration" });
  const navigate = async (name, parent) => {
    if (parent) { const toggle = nav.getByTitle(parent, { exact: true }); if (await toggle.getAttribute("aria-expanded") !== "true") await toggle.click(); }
    await nav.getByTitle(name, { exact: true }).click();
    await main.getByRole("heading", { name, exact: true }).waitFor();
  };
  const assertDisabled = async (locator, message) => { if (!await locator.isDisabled()) throw new Error(message); };
  const chooseFilter = async (name, option) => {
    await main.getByLabel(name, { exact: true }).click();
    await page.getByRole("option", { name: option, exact: true }).click();
  };
  const workspaceCount = () => page.evaluate(() => JSON.parse(localStorage.getItem("fxserver-installer.workspaces.v1")).items.length);
  const openClone = async () => { await page.getByRole("button", { name: "Clone or migrate Source fixture", exact: true }).click(); await clone.waitFor(); };
  const selectFiles = async () => {
    await clone.getByRole("textbox", { name: /^Source server-data folder/ }).fill("C:/fixture/source/server-data");
    await clone.getByRole("button", { name: "Load source files", exact: true }).click();
    await clone.getByRole("checkbox", { name: "[local]/demo", exact: true }).check();
    await clone.getByRole("checkbox", { name: "server.cfg", exact: true }).check();
  };
  const confirmFiles = async () => {
    const destination = await clone.getByRole("textbox", { name: "Confirm destination path", exact: true }).getAttribute("placeholder");
    await clone.getByRole("textbox", { name: "Confirm destination path", exact: true }).fill(destination);
    await clone.getByRole("checkbox", { name: /^I have permission/ }).check();
  };
  const confirmDatabase = async () => {
    const field = clone.getByRole("textbox", { name: "Confirm new database name", exact: true });
    const target = await field.getAttribute("placeholder");
    await field.fill(target);
    await clone.getByLabel("Target database password", { exact: true }).fill("fixture-db-password");
    return target;
  };
  const screenshot = async (name, target = main) => {
    const overflow = await target.evaluate((element) => element.scrollWidth > element.clientWidth + 2);
    if (overflow) throw new Error(`${name}: horizontal overflow`);
    await page.screenshot({ path: `output/playwright/${name}.png`, fullPage: true, animations: "disabled" });
  };

  await navigate("Workspaces");
  await openClone();
  if (await clone.getByRole("checkbox", { name: "Include a reviewed database dump" }).isChecked()) throw new Error("Database copy is not opt-in");
  await selectFiles();
  await clone.getByRole("textbox", { name: /^Destination parent folder/ }).fill("C:/fixture/clones");
  await clone.getByRole("textbox", { name: "New workspace name", exact: true }).fill("Private clone fixture");
  await clone.getByRole("textbox", { name: "New folder name", exact: true }).fill("existing");
  await clone.getByRole("button", { name: "Preview manifest", exact: true }).click();
  await clone.getByText("The destination already exists. Nothing will be overwritten.", { exact: true }).waitFor();
  await clone.getByRole("textbox", { name: "New folder name", exact: true }).fill("private-clone");
  await clone.getByRole("button", { name: "Preview manifest", exact: true }).click();
  await clone.getByRole("heading", { name: "Reviewed manifest", exact: true }).waitFor();
  const create = clone.getByRole("button", { name: "Create private clone", exact: true });
  await assertDisabled(create, "File clone bypassed confirmation");
  await clone.getByRole("textbox", { name: "Confirm destination path", exact: true }).fill("C:/fixture/source/server-data");
  await clone.getByRole("checkbox", { name: /^I have permission/ }).check();
  await assertDisabled(create, "Wrong destination was accepted");
  const fileRequest = await page.evaluate(() => window.workspaceCloneFixture.calls.filter((call) => call.command === "preview_workspace_clone").at(-1).args.request);
  if (fileRequest.database !== null || fileRequest.resources.join() !== "[local]/demo" || fileRequest.configs.join() !== "server.cfg") throw new Error("File-only preview did not preserve explicit selections");
  await clone.getByRole("button", { name: "Revise selection", exact: true }).click();
  await clone.getByRole("checkbox", { name: "Include a reviewed database dump" }).check();
  await clone.getByRole("button", { name: "Choose SQL dump", exact: true }).click();
  await clone.getByRole("textbox", { name: /^SQL dump file/ }).waitFor();
  if (await clone.getByRole("textbox", { name: /^SQL dump file/ }).inputValue() !== "C:/fixture/reviewed.sql") throw new Error("SQL dialog selection was not applied");
  await clone.getByRole("textbox", { name: "Target database username", exact: true }).fill("fixture_importer");
  await clone.getByRole("button", { name: "Preview manifest", exact: true }).click();
  await clone.getByRole("heading", { name: "Database manifest", exact: true }).waitFor();
  await confirmFiles();
  await clone.getByRole("textbox", { name: "Confirm new database name", exact: true }).fill("fixture_source");
  await assertDisabled(create, "Source database name enabled clone import");
  await confirmDatabase();
  await clone.getByRole("heading", { name: "Database manifest", exact: true }).scrollIntoViewIfNeeded();
  await screenshot("workspace-clone-desktop", clone);
  await page.setViewportSize({ width: 640, height: 1000 });
  await clone.getByRole("heading", { name: "Database manifest", exact: true }).scrollIntoViewIfNeeded();
  await screenshot("workspace-clone-narrow", clone);
  await page.setViewportSize({ width: 1440, height: 1100 });
  await page.evaluate(() => { window.workspaceCloneFixture.failNextExecute = true; });
  await create.click();
  await clone.getByText("Fixture copy failed before promotion. No destination was created; new owned database cleaned up.", { exact: true }).waitFor();
  if (await workspaceCount() !== 2) throw new Error("Failed copy registered a workspace");
  await clone.getByRole("button", { name: "Preview manifest", exact: true }).click();
  await confirmFiles();
  const databaseName = await confirmDatabase();
  await page.evaluate(() => { window.workspaceCloneFixture.deferNextExecute = true; });
  await create.click();
  await page.waitForFunction(() => !!window.workspaceCloneFixture.pending);
  await assertDisabled(clone.getByRole("button", { name: "Close clone panel" }), "Running copy can be closed");
  await assertDisabled(page.getByRole("button", { name: "Clone or migrate QA fixture" }), "Overlapping clone is enabled");
  await page.waitForFunction(() => window.workspaceCloneFixture.calls.some((call) => call.command === "plugin:event|listen" && call.args.event === "fxserver-health-event"));
  await page.evaluate(() => {
    const health = { id: 700, timestamp: Date.now(), workspaceId: "default", level: "warn", kind: "cpu", message: "CPU threshold while timeline hidden" };
    window.workspaceCloneFixture.emit("fxserver-health-event", health);
    window.workspaceCloneFixture.emit("fxserver-health-event", health);
    window.dispatchEvent(new CustomEvent("app-log-entry", { detail: { id: "hidden-config", timestamp: new Date().toISOString(), level: "info", scope: "server.config", message: "Configuration changed while timeline hidden", detail: 'token="fixture-stream-secret"' } }));
  });
  await navigate("Incident Timeline", "Logs");
  await main.getByRole("heading", { name: "CPU threshold while timeline hidden", exact: true }).waitFor();
  if (await main.getByRole("heading", { name: "CPU threshold while timeline hidden", exact: true }).count() !== 1) throw new Error("Health event was duplicated");
  await page.evaluate(() => window.workspaceCloneFixture.pending());
  await page.waitForFunction(() => JSON.parse(localStorage.getItem("fxserver-installer.workspaces.v1")).items.length === 3);
  const saved = await page.evaluate(() => JSON.parse(localStorage.getItem("fxserver-installer.workspaces.v1")));
  const copied = saved.items.find((item) => item.name === "Private clone fixture");
  if (saved.activeId !== "default" || copied.database.database !== databaseName || copied.txDataPath !== "C:/fixture/clones/private-clone/txData" || copied.environment.TXHOST_FXS_PORT !== "30121" || copied.environment.TXHOST_TXA_PORT !== "40121") throw new Error("Clone did not preserve isolated workspace paths, ports, and owned database defaults");
  if (JSON.stringify(saved).includes("fixture-db-password")) throw new Error("Database credentials persisted in workspace");

  await chooseFilter("Workspace", "QA fixture");
  await chooseFilter("Type", "Health");
  await main.getByRole("heading", { name: "QA memory threshold", exact: true }).waitFor();
  await assertDisabled(main.getByRole("button", { name: "Open Health & Recovery", exact: true }), "Inactive workspace incident link is enabled");
  await main.getByRole("button", { name: "Clear history", exact: true }).click();
  await main.getByText("No matching events.", { exact: true }).waitFor();
  await chooseFilter("Workspace", "Source fixture");
  await chooseFilter("Type", "All types");
  const dates = await page.evaluate(() => { const local = (timestamp) => new Date(timestamp - new Date(timestamp).getTimezoneOffset() * 60000).toISOString().slice(0, 16); return { after: local(Date.now() - 86400000), before: local(Date.now() + 86400000) }; });
  await main.getByLabel("From", { exact: true }).fill(dates.after);
  await main.getByLabel("Until", { exact: true }).fill(dates.before);
  if (await main.getByRole("heading", { name: "Old fixture restart", exact: true }).count()) throw new Error("Time filter includes older incidents");
  await main.getByLabel("Search", { exact: true }).fill("Configuration changed while timeline hidden");
  await main.getByRole("heading", { name: "Configuration changed while timeline hidden", exact: true }).waitFor();
  await main.locator("summary").filter({ hasText: "Details" }).click();
  if ((await main.innerText()).includes("fixture-stream-secret")) throw new Error("Secret was visible in timeline details");
  await main.getByLabel("Search", { exact: true }).fill("never-matches-fixture");
  await main.getByText("No matching events.", { exact: true }).waitFor();
  await main.getByLabel("Search", { exact: true }).fill("");
  await main.getByRole("heading", { name: "Incident Timeline", exact: true }).scrollIntoViewIfNeeded();
  await screenshot("incident-timeline-desktop");
  await page.setViewportSize({ width: 640, height: 1000 });
  await screenshot("incident-timeline-narrow");
  await main.getByLabel("Workspace", { exact: true }).click();
  await page.getByRole("option", { name: "All workspaces", exact: true }).waitFor();
  await screenshot("incident-timeline-filter-menu-narrow");
  await page.keyboard.press("Escape");
  await page.setViewportSize({ width: 1440, height: 1100 });
  await page.waitForFunction(() => (localStorage.getItem("fxserver-installer.incidents.v1") || "").includes("CPU threshold while timeline hidden"));
  const persisted = await page.evaluate(() => localStorage.getItem("fxserver-installer.incidents.v1"));
  for (const secret of ["fixture-seeded-secret", "fixture-stream-secret", "fixture-db-password"]) if (persisted.includes(secret)) throw new Error(`Timeline persisted ${secret}`);
  const cloneEvent = main.locator("article").filter({ has: page.getByRole("heading", { name: "Private server clone created", exact: true }) });
  await cloneEvent.getByRole("button", { name: "Open Workspaces", exact: true }).click();
  await main.getByRole("heading", { name: "Workspaces", exact: true }).waitFor();

  await openClone();
  await clone.getByRole("button", { name: "Export package", exact: true }).click();
  await selectFiles();
  await clone.getByRole("textbox", { name: /^Destination parent folder/ }).fill("C:/fixture/exports");
  await clone.getByRole("textbox", { name: "New folder name", exact: true }).fill("private-package");
  await clone.getByRole("button", { name: "Preview manifest", exact: true }).click();
  await confirmFiles();
  await clone.getByRole("button", { name: "Export local package", exact: true }).click();
  await clone.getByText("Local package exported", { exact: true }).waitFor();
  if (await workspaceCount() !== 3) throw new Error("Package export registered a workspace");
  await clone.getByRole("button", { name: "Close clone panel", exact: true }).click();
  await openClone();
  await clone.getByRole("button", { name: "Import package", exact: true }).click();
  await clone.getByRole("textbox", { name: /^Source package folder/ }).fill("C:/fixture/exports/private-package");
  await clone.getByRole("textbox", { name: /^Destination parent folder/ }).fill("C:/fixture/imports");
  await clone.getByRole("textbox", { name: "New folder name", exact: true }).fill("imported-package");
  await clone.getByRole("textbox", { name: "New workspace name", exact: true }).fill("Imported package fixture");
  await clone.getByRole("button", { name: "Preview manifest", exact: true }).click();
  await clone.getByText("Database copy was not selected", { exact: true }).waitFor();
  await confirmFiles();
  await clone.getByRole("button", { name: "Create private clone", exact: true }).click();
  await clone.getByText("Clone created", { exact: true }).waitFor();
  if (await workspaceCount() !== 4) throw new Error("Imported package did not register an isolated workspace");
  const calls = await page.evaluate(() => window.workspaceCloneFixture.calls);
  if (calls.some((call) => /^(start_fxserver|restart_fxserver|prepare_workspace_switch|restore_backup_snapshot|execute_mariadb_query|backup_mariadb|install_)/.test(call.command))) throw new Error("Clone launched, switched, installed, or invoked a production database workflow");
  if (!calls.some((call) => call.command === "discard_workspace_clone_preview")) throw new Error("Revised preview was not discarded");
  if (await page.evaluate(() => window.workspaceCloneFixture.unknown.length)) throw new Error(`Unmocked IPC: ${await page.evaluate(() => window.workspaceCloneFixture.unknown.join(", "))}`);
  await page.reload({ waitUntil: "domcontentloaded" });
  await navigate("Incident Timeline", "Logs");
  await main.getByRole("heading", { name: "CPU threshold while timeline hidden", exact: true }).waitFor();
  if (await main.getByRole("heading", { name: "QA memory threshold", exact: true }).count()) throw new Error("Cleared workspace incidents returned after reload");
  if (errors.length) throw new Error(errors.join("\n"));
  return "Clone and timeline UI fixtures passed: explicit files/DB confirmation, existing-target and copy failures, deferred task isolation, private export/import, hidden capture/deduplication, redaction, workspace/type/time/search filters, related panels, reload persistence, desktop/narrow screenshots.";
}
