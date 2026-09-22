async (page) => {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.setViewportSize({ width: 1440, height: 1050 });
  await page.route("https://raw.githubusercontent.com/**", (route) => route.fulfill({ json: { version: "0.4.1" } }));
  await page.route("**/jsonv2", (route) => route.fulfill({ json: { recommended: "10000", latest: "10000" } }));
  await page.addInitScript(() => {
    localStorage.clear();
    let sequence = 0;
    const callbacks = new Map();
    const state = window.testDatabaseAdmin = { calls: [], unknown: [], failApply: false, failInspection: false, expired: false, heldInspection: null, holdInspection: false, previews: new Map() };
    const credentials = { host: "fixture.invalid", port: 3306, username: "fixture", password: "fixture-password", database: "qbx" };
    const table = { name: "players", kind: "BASE TABLE", engine: "InnoDB", rows: 17, collation: "utf8mb4_unicode_ci", dataBytes: 16384, indexBytes: 32768, freeBytes: 0 };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
      transformCallback: (callback) => { callbacks.set(++sequence, callback); return sequence; },
      invoke: async (command, args = {}) => {
        state.calls.push({ command, args: structuredClone(args) });
        switch (command) {
          case "fetch_latest_app_release": return { version: "0.4.1", tagName: "v0.4.1", htmlUrl: "https://github.com/zoxile/fxserver-installer/releases/tag/v0.4.1", installerUrl: "https://github.com/zoxile/fxserver-installer/releases/download/v0.4.1/FXServer.Installer_0.4.1_windows_x64-setup.exe" };
          case "plugin:window|title": return "FXServer Installer";
          case "plugin:app|version": return "0.4.1";
          case "plugin:event|listen": return args.handler;
          case "plugin:event|unlisten":
          case "append_app_log":
          case "initialize_health_workspace":
          case "save_database_login":
          case "clear_database_login": return;
          case "load_database_login": return structuredClone(credentials);
          case "configure_live_bridge": return { workspaceId: "default", enabled: false, connected: false, receivedAt: null, error: null, snapshot: null };
          case "read_app_logs": return { path: "mock.log", entries: [] };
          case "get_windows_artifact_metadata": return { recommendedArtifact: "10000", windowsDownloadLink: "https://example.invalid/artifact.zip", brokenArtifacts: [] };
          case "get_installed_windows_artifact_info": return { installed: false };
          case "validate_mariadb_credentials": return;
          case "list_mariadb_databases": return ["mysql", "qbx", "other"];
          case "list_mariadb_tables": return ["players"];
          case "get_database_browser_metadata": return { columns: [{ name: "id", columnType: "int(11)", nullable: false, defaultValue: null, extra: "auto_increment", binary: false }], indexes: [{ name: "PRIMARY", column: "id", sequence: 1, unique: true, indexType: "BTREE", prefixLength: null }], editable: false, editReason: "Fixture is read-only." };
          case "get_database_browser_rows": return { rows: [["1"]], hasMore: false, truncatedCells: false, pageSize: 25 };
          case "list_database_admin_tables": return { tables: [structuredClone(table), { ...table, name: "audit_view", kind: "VIEW", engine: null, rows: null }], hasMore: false };
          case "inspect_database": {
            if (state.failInspection) throw new Error("Inspection permission denied (fixture)");
            const result = { columns: ["Name", "Value"], rows: [[`${args.view}-fixture`, args.view === "variables" ? "[redacted]" : "visible"]], hasMore: args.view === "status", limit: 200, notice: "Read-only fixture snapshot." };
            if (state.holdInspection) return new Promise((resolve) => { state.heldInspection = () => resolve(result); });
            return result;
          }
          case "preview_database_admin_action": {
            const request = args.request;
            if (["mysql", "sys", "information_schema", "performance_schema"].includes(request.database.toLowerCase())) throw new Error("System database protected");
            const token = `admin-${++sequence}`;
            const confirmation = request.table ? `${request.database}.${request.table}` : request.database;
            const sql = request.action === "createTable" ? `CREATE TABLE \`${request.database}\`.\`${request.table}\` (\`id\` INT PRIMARY KEY)` : `${request.action.toUpperCase()} ${confirmation}`;
            const preview = { token, confirmation, sql, expiresAt: Date.now() + (state.expired ? -1000 : 90000), host: credentials.host, port: credentials.port, database: request.database, table: request.table, warning: "Nontransactional action; no rollback is promised. Refresh after an interrupted operation." };
            state.previews.set(token, { request: structuredClone(request), preview });
            return preview;
          }
          case "apply_database_admin_action": {
            const permit = state.previews.get(args.token); state.previews.delete(args.token);
            if (!permit || permit.preview.confirmation !== args.confirmation || permit.request.workspaceId !== args.workspaceId) throw new Error("Invalid exact confirmation");
            if (state.failApply) throw new Error("Schema changed since preview. No action was sent. Refresh and review again.");
            return { message: "Administrative action completed.", messages: [] };
          }
          default: state.unknown.push(command); throw new Error(`Unmocked command: ${command}`);
        }
      },
    };
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  const nav = page.getByRole("navigation", { name: "Workspace navigation" });
  const parent = nav.getByTitle("MariaDB", { exact: true });
  if (await parent.getAttribute("aria-expanded") !== "true") await parent.click();
  await nav.getByTitle("Database Browser", { exact: true }).click();
  await page.getByRole("cell", { name: "1", exact: true }).waitFor();
  if (await nav.getByTitle("Queries & Files", { exact: true }).count() !== 1) throw new Error("Queries & Files navigation was removed");
  const initial = await page.evaluate(() => window.testDatabaseAdmin.calls.map((call) => call.command));
  if (initial.indexOf("validate_mariadb_credentials") > initial.indexOf("get_database_browser_metadata")) throw new Error("Restored login bypassed validation");
  if (initial.some((command) => ["inspect_database", "list_database_admin_tables"].includes(command))) throw new Error("Admin/inspection commands were not lazy");

  for (const [tab, view] of [["Overview", "overview"], ["Status", "status"], ["Processes", "processes"], ["Variables", "variables"], ["Charsets", "charsets"], ["Engines", "engines"], ["Users", "users"], ["Grants", "grants"]]) {
    await page.getByRole("tab", { name: tab, exact: true }).click();
    await page.getByRole("cell", { name: `${view}-fixture`, exact: true }).waitFor();
    if (await page.getByRole("tabpanel").count() !== 1) throw new Error("Inactive panels stayed mounted");
    if (tab === "Status") await page.getByText(/Showing the first 200 records/).waitFor();
  }
  await page.evaluate(() => { window.testDatabaseAdmin.failInspection = true; });
  await page.getByRole("button", { name: "Refresh inspection", exact: true }).click();
  await page.getByText(/Inspection permission denied \(fixture\)/).waitFor();
  if (await page.getByRole("cell", { name: "grants-fixture", exact: true }).count()) throw new Error("Old successful data remained after failure");
  await page.getByRole("alert").getByRole("button", { name: "Dismiss notification", exact: true }).click();
  await page.getByRole("button", { name: "Refresh inspection", exact: true }).click();
  await page.getByText(/Inspection permission denied \(fixture\)/).waitFor();
  await page.evaluate(() => { window.testDatabaseAdmin.failInspection = false; window.testDatabaseAdmin.holdInspection = true; });
  await page.getByRole("tab", { name: "Status", exact: true }).click();
  await page.waitForFunction(() => Boolean(window.testDatabaseAdmin.heldInspection));
  await page.getByRole("tab", { name: "rows", exact: true }).click();
  await page.evaluate(() => { window.testDatabaseAdmin.holdInspection = false; window.testDatabaseAdmin.heldInspection(); });
  await page.getByRole("cell", { name: "1", exact: true }).waitFor();
  if (await page.getByRole("cell", { name: "status-fixture", exact: true }).count()) throw new Error("Late inspection overwrote rows");

  await page.getByRole("tab", { name: "Tables", exact: true }).click();
  await page.getByRole("button", { name: "players", exact: true }).click();
  const chooseAction = async (name) => {
    await page.getByRole("button", { name: "Table action", exact: true }).click();
    await page.getByRole("option", { name, exact: true }).click();
  };
  await chooseAction("Repair");
  if (!await page.getByRole("button", { name: "Review Action", exact: true }).isDisabled()) throw new Error("InnoDB repair was offered");
  await chooseAction("Empty");
  await page.getByRole("button", { name: "Review Action", exact: true }).click();
  await page.getByRole("heading", { name: "Pending Administration", exact: true }).waitFor();
  const confirm = page.getByRole("button", { name: "Confirm Administration", exact: true });
  if (!await confirm.isDisabled()) throw new Error("Confirmation was not required");
  await page.getByRole("textbox", { name: "Confirm qbx.players", exact: true }).fill("players");
  if (!await confirm.isDisabled()) throw new Error("Table-only confirmation accepted");
  await page.getByRole("textbox", { name: "Confirm qbx.players", exact: true }).fill("qbx.players");
  await page.screenshot({ path: "output/playwright/database-admin-confirmation.png", fullPage: true });
  await confirm.click();
  await page.getByText("Administrative action completed.", { exact: true }).waitFor();
  await page.getByRole("button", { name: "players", exact: true }).click();
  await chooseAction("Drop");
  await page.getByRole("button", { name: "Review Action", exact: true }).click();
  await page.getByRole("textbox", { name: "Confirm qbx.players", exact: true }).fill("qbx.players");
  await page.evaluate(() => { window.testDatabaseAdmin.failApply = true; });
  await confirm.click();
  await page.getByText(/Schema changed since preview/).waitFor();
  if (await confirm.count()) throw new Error("Failed token remained reusable");
  await page.getByRole("alert").getByRole("button", { name: "Dismiss notification", exact: true }).click();
  await page.evaluate(() => { window.testDatabaseAdmin.failApply = false; window.testDatabaseAdmin.expired = true; });
  await page.getByRole("button", { name: "Review Action", exact: true }).click();
  await page.getByText("Preview expired. Cancel and review again.", { exact: true }).waitFor();
  if (!await confirm.isDisabled()) throw new Error("Expired preview allowed mutation");
  await page.getByRole("button", { name: "Cancel Preview", exact: true }).click();
  await page.evaluate(() => { window.testDatabaseAdmin.expired = false; });

  await page.getByRole("button", { name: "Create Table", exact: true }).click();
  await page.getByRole("textbox", { name: "Table name", exact: true }).fill("new_table");
  await page.getByRole("button", { name: "Column 2 default", exact: true }).click();
  await page.getByRole("option", { name: "Literal value", exact: true }).click();
  await page.getByRole("textbox", { name: "Column 2 default value", exact: true }).fill("a'\\b");
  await page.getByRole("button", { name: "Review Create Table", exact: true }).click();
  await page.getByRole("heading", { name: "Pending Administration", exact: true }).waitFor();
  const created = await page.evaluate(() => window.testDatabaseAdmin.calls.filter((call) => call.command === "preview_database_admin_action").at(-1).args.request);
  if (!created.columns[0].primary || !created.columns[0].autoIncrement || created.columns[1].defaultValue !== "a'\\b") throw new Error("Keys/defaults were lost in the create-table request");
  await page.getByRole("button", { name: "Cancel Preview", exact: true }).click();
  await page.getByRole("button", { name: "Create Database", exact: true }).click();
  await page.getByRole("textbox", { name: "New database name", exact: true }).fill("new_database");
  await page.getByRole("button", { name: "Review Create Database", exact: true }).click();
  await page.getByRole("textbox", { name: "Confirm new_database", exact: true }).waitFor();
  await page.locator("#browser-database").click();
  await page.getByRole("option", { name: "other", exact: true }).click();
  await page.getByRole("button", { name: "players", exact: true }).waitFor();
  if (await page.getByRole("heading", { name: "Pending Administration", exact: true }).count()) throw new Error("Preview survived a database change");
  await page.locator("#browser-database").click();
  await page.getByRole("option", { name: "mysql", exact: true }).click();
  await page.getByText("System database: administration is disabled.", { exact: true }).waitFor();
  if (!await page.getByRole("button", { name: "Create Table", exact: true }).isDisabled()) throw new Error("Create table enabled for a system schema");
  await page.getByRole("button", { name: "players", exact: true }).click();
  if (!await page.getByRole("button", { name: "Review Action", exact: true }).isDisabled()) throw new Error("System schema maintenance enabled");
  await page.setViewportSize({ width: 1000, height: 900 });
  await page.screenshot({ path: "output/playwright/database-admin-narrow.png", fullPage: true });
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth + 1);
  if (overflow) throw new Error("Database administration overflowed the viewport");
  if (errors.length) throw new Error(errors.join("\n"));
  const state = await page.evaluate(() => ({ unknown: window.testDatabaseAdmin.unknown, calls: window.testDatabaseAdmin.calls }));
  if (state.unknown.length) throw new Error(`Missing mocks: ${state.unknown.join(", ")}`);
  if (state.calls.some((call) => ["execute_mariadb_query", "run_table_action", "create_table", "drop_database"].includes(call.command))) throw new Error("Admin bypassed reviewed structured commands");
  const applies = state.calls.filter((call) => call.command === "apply_database_admin_action");
  if (applies.length !== 2 || applies.some((call) => "credentials" in call.args || "request" in call.args)) throw new Error("Apply accepted a mutable target or unexpected invocation");
  console.log("PASS: restored-login validation, lazy bounded inspection tabs, errors/late responses, exact admin confirmation, stale/expired previews, keys/defaults, target invalidation, system protection and Queries & Files navigation.");
}
