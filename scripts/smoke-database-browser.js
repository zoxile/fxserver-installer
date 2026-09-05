async (page) => {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.setViewportSize({ width: 1440, height: 1050 });
  await page.route("https://raw.githubusercontent.com/**", (route) => route.fulfill({ json: { version: "0.3.2" } }));
  await page.route("**/jsonv2", (route) => route.fulfill({ json: { recommended: "10000", latest: "10000" } }));
  await page.addInitScript(() => {
    localStorage.clear();
    let sequence = 0;
    const callbacks = new Map();
    const state = window.testDatabaseBrowser = {
      calls: [], unknown: [], pending: null, failChange: false, refuseTest: false, restoreTests: [],
      rows: Array.from({ length: 27 }, (_, i) => [String(i + 1), i === 0 ? null : i === 1 ? "" : `Player ${i + 1}`, i === 0 ? "NULL" : `note\t${i}\nline 2`]),
    };
    const metadata = {
      columns: [
        { name: "id", columnType: "int(11)", nullable: false, defaultValue: null, extra: "auto_increment", binary: false },
        { name: "name", columnType: "varchar(128)", nullable: true, defaultValue: null, extra: "", binary: false },
        { name: "note", columnType: "text", nullable: false, defaultValue: null, extra: "", binary: false },
      ],
      indexes: [{ name: "PRIMARY", column: "id", sequence: 1, unique: true, indexType: "BTREE", prefixLength: null }],
      editable: true, editReason: null,
    };
    const snapshot = { id: "fixture-snapshot", workspaceId: "default", scheduleId: "daily", database: "qbx", directory: "C:/fixture/fxserver-managed-backups/default/daily", createdAt: 1700000000000, sizeBytes: 4096, sha256: "a".repeat(64), kind: "manual", sourceHost: "fixture.invalid", sourcePort: 3306 };
    const temporary = `fxsi_restore_test_${"b".repeat(32)}`;
    const evidence = (status, extra = {}) => ({ id: `test-${state.restoreTests.length}`, workspaceId: "default", snapshotId: snapshot.id, snapshotSha256: snapshot.sha256, targetHost: "fixture.invalid", targetPort: 3306, temporaryDatabase: temporary, status, startedAt: Date.now(), finishedAt: Date.now(), tablesVerified: [], error: null, cleanupError: null, cleanedUp: false, created: false, ...extra });
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
      transformCallback: (callback) => { callbacks.set(++sequence, callback); return sequence; },
      invoke: async (command, args = {}) => {
        state.calls.push({ command, args: structuredClone(args) });
        switch (command) {
          case "fetch_latest_app_release": return { version: "0.3.2", tagName: "v0.3.2", htmlUrl: "https://github.com/zoxile/fxserver-installer/releases/tag/v0.3.2", installerUrl: "https://github.com/zoxile/fxserver-installer/releases/download/v0.3.2/FXServer.Installer_0.3.2_windows_x64-setup.exe" };
          case "plugin:window|title": return "FXServer Installer";
          case "plugin:app|version": return "0.3.2";
          case "plugin:event|listen": return args.handler;
          case "plugin:event|unlisten":
          case "append_app_log":
          case "initialize_health_workspace": return;
          case "configure_live_bridge": return { workspaceId: "default", enabled: false, connected: false, receivedAt: null, error: null, snapshot: null };
          case "read_app_logs": return { path: "mock.log", entries: [] };
          case "get_windows_artifact_metadata": return { recommendedArtifact: "10000", windowsDownloadLink: "https://example.invalid/artifact.zip", brokenArtifacts: [] };
          case "get_installed_windows_artifact_info": return { installed: false };
          case "validate_mariadb_credentials": return;
          case "list_mariadb_databases": return ["mysql", "qbx"];
          case "list_mariadb_tables": return ["players"];
          case "get_database_browser_metadata": return state.wide ? { columns: Array.from({ length: 128 }, (_, i) => ({ ...metadata.columns[2], name: `column_${i}` })), indexes: [], editable: false, editReason: "Wide table fixture is read-only." } : structuredClone(metadata);
          case "get_database_browser_rows": {
            if (state.wide) {
              const pageSize = Math.min(args.request.pageSize, Math.floor(4000 / 128));
              return { rows: Array.from({ length: pageSize }, (_, i) => Array.from({ length: 128 }, (_, j) => `${i}:${j}`)), hasMore: true, truncatedCells: false, pageSize };
            }
            const rows = args.request.filters.length ? state.rows.filter((row) => row[2].includes(args.request.filters[0].value ?? "")) : state.rows;
            return { rows: structuredClone(rows.slice(args.request.offset, args.request.offset + args.request.pageSize)), hasMore: rows.length > args.request.offset + args.request.pageSize, truncatedCells: false };
          }
          case "plugin:dialog|save": return "C:/fixture/browser.csv";
          case "export_database_browser_csv": return { path: args.outputPath, rows: state.rows.length, hasMore: false };
          case "preview_database_browser_change":
            state.previewChange = structuredClone(args.change);
            return { token: `change-${++sequence}`, sql: `${args.change.kind.toUpperCase()} \`qbx\`.\`players\` /* fixture SQL preview */ LIMIT 1;`, parameters: args.change.values, confirmation: "qbx.players", expiresAt: Date.now() + 120000, kind: args.change.kind, host: "fixture.invalid", port: 3306 };
          case "apply_database_browser_change":
            if (args.confirmation !== "qbx.players") throw new Error("Confirmation mismatch");
            if (state.failChange) throw new Error("No change committed: the row or schema changed. Refresh and review again.");
            return 1;
          case "get_backup_manager": return { schedules: [], snapshots: [snapshot], restoreTests: structuredClone(state.restoreTests), busy: Boolean(state.pending) };
          case "preview_backup_restore_test":
            if (state.refuseTest) { state.restoreTests.unshift(evidence("preflight_refused", { error: "Cross-schema directive refused. No SQL was sent." })); throw new Error("Cross-schema directive refused. No SQL was sent."); }
            return { token: "restore-test-token", snapshotId: snapshot.id, targetHost: "fixture.invalid", targetPort: 3306, temporaryDatabase: temporary, tables: ["players"], statements: 8, expiresAt: Date.now() + 300000 };
          case "test_backup_restore":
            if (args.confirmationDatabase !== temporary || !args.confirmCleanup) throw new Error("Restore confirmation missing");
            return new Promise((resolve) => { state.pending = resolve; });
          default: state.unknown.push(command); throw new Error(`Unmocked command: ${command}`);
        }
      },
    };
    state.finishRestore = () => {
      const result = evidence("passed", { created: true, cleanedUp: true, tablesVerified: ["players"] });
      state.restoreTests.unshift(result); const resolve = state.pending; state.pending = null; resolve(result);
    };
  });
  await page.reload({ waitUntil: "domcontentloaded", timeout: 120000 });
  const nav = page.getByRole("navigation", { name: "Workspace navigation" });
  const parent = nav.getByTitle("MariaDB", { exact: true });
  if (await parent.getAttribute("aria-expanded") !== "true") await parent.click();
  await nav.getByTitle("Database Browser", { exact: true }).click();
  await page.getByRole("heading", { name: "Database Browser", exact: true }).waitFor();
  await page.getByRole("button", { name: "Change Credentials", exact: true }).click();
  await page.getByTitle("SQL NULL", { exact: true }).first().waitFor();
  if (await page.getByRole("button", { name: "Edit this row", exact: true }).count()) throw new Error("Browser did not default to read-only");
  await page.getByRole("button", { name: "Next page", exact: true }).click();
  await page.getByText("26-27 rows", { exact: true }).waitFor();
  await page.getByRole("button", { name: "Previous page", exact: true }).click();
  await page.getByRole("button", { name: "Filter", exact: true }).click();
  await page.getByRole("textbox", { name: "Filter 1 value", exact: true }).fill("NULL");
  await page.getByRole("button", { name: "Apply", exact: true }).click();
  await page.getByText("1-1 rows", { exact: true }).waitFor();
  await page.getByRole("button", { name: "Remove filter 1", exact: true }).click();
  await page.getByRole("button", { name: "Apply", exact: true }).click();
  await page.getByRole("tab", { name: "rows", exact: true }).focus();
  await page.keyboard.press("ArrowRight");
  await page.getByRole("cell", { name: "varchar(128)", exact: true }).waitFor();
  if (await page.getByRole("tab", { name: "columns", exact: true }).getAttribute("aria-selected") !== "true") throw new Error("Arrow key did not activate the columns tab");
  if (await page.getByRole("tabpanel").count() !== 1) throw new Error("Inactive tab panels remained mounted");
  await page.getByRole("tab", { name: "indexes", exact: true }).click();
  await page.getByRole("cell", { name: "PRIMARY", exact: true }).waitFor();
  await page.getByRole("tab", { name: "rows", exact: true }).click();
  await page.getByRole("checkbox", { name: "Enable row editing", exact: true }).check();
  await page.getByRole("button", { name: "Edit this row", exact: true }).nth(2).click();
  await page.locator("#row-field-1").fill("Edited name");
  await page.getByRole("button", { name: "Preview SQL", exact: true }).click();
  await page.getByRole("heading", { name: "Pending Change", exact: true }).waitFor();
  const apply = page.getByRole("button", { name: "Confirm & Apply", exact: true });
  if (!await apply.isDisabled()) throw new Error("Mutation enabled before exact confirmation");
  await page.getByRole("textbox", { name: "Confirm qbx.players", exact: true }).fill("players");
  if (!await apply.isDisabled()) throw new Error("Mutation accepted incomplete confirmation");
  await page.getByRole("textbox", { name: "Confirm qbx.players", exact: true }).fill("qbx.players");
  await page.screenshot({ path: "output/playwright/database-browser-desktop.png", fullPage: true });
  await apply.click();
  await page.getByText("One row changed.", { exact: true }).waitFor();
  await page.getByRole("button", { name: "Insert Row", exact: true }).click();
  await page.locator("#row-field-2").fill("Inserted fixture");
  await page.getByRole("button", { name: "Preview SQL", exact: true }).click();
  await page.getByRole("heading", { name: "Pending Change", exact: true }).waitFor();
  const inserted = await page.evaluate(() => window.testDatabaseBrowser.previewChange);
  if (inserted.kind !== "insert" || inserted.values.some((input) => input.column === "id")) throw new Error("Insert did not preserve omitted auto-increment field");
  await page.getByRole("button", { name: "Close row editor", exact: true }).click();
  await page.getByRole("button", { name: "Delete this row", exact: true }).first().click();
  await page.getByRole("button", { name: "Preview SQL", exact: true }).click();
  await page.getByRole("heading", { name: "Pending Change", exact: true }).waitFor();
  await page.getByRole("textbox", { name: "Confirm qbx.players", exact: true }).fill("qbx.players");
  await page.evaluate(() => { window.testDatabaseBrowser.failChange = true; });
  await apply.click();
  await page.getByText(/No change committed: the row or schema changed/).waitFor();
  await page.getByRole("button", { name: "Close row editor", exact: true }).click();
  await page.getByRole("button", { name: "Export CSV", exact: true }).click();
  await page.getByText(/27 rows exported/).waitFor();
  await page.setViewportSize({ width: 1000, height: 900 });
  await page.screenshot({ path: "output/playwright/database-browser-narrow.png", fullPage: true });
  await page.getByRole("button", { name: "Rows per page", exact: true }).click();
  await page.getByRole("option", { name: "200", exact: true }).click();
  await page.evaluate(() => { window.testDatabaseBrowser.wide = true; });
  await page.getByRole("button", { name: "Refresh table", exact: true }).click();
  await page.getByRole("button", { name: "Rows per page", exact: true }).filter({ hasText: "31" }).waitFor();
  await page.getByText("1-31 rows", { exact: true }).waitFor();
  const renderedCells = await page.locator('[role="tabpanel"] tbody td').count();
  if (renderedCells !== 31 * 128 || renderedCells > 4000) throw new Error(`Wide row page rendered ${renderedCells} cells; expected 3,968 within the 4,000-cell budget`);
  await nav.getByTitle("Backups & Restore", { exact: true }).click();
  await page.getByRole("button", { name: "Test Restore", exact: true }).waitFor();
  await page.getByRole("button", { name: "Test Restore", exact: true }).click();
  await page.getByRole("heading", { name: "Isolated Restore Test", exact: true }).waitFor();
  const test = page.getByRole("button", { name: "Confirm & Test", exact: true });
  if (!await test.isDisabled()) throw new Error("Restore test enabled before confirmation");
  await page.getByRole("textbox", { name: /^Confirm temporary database:/ }).fill(`fxsi_restore_test_${"b".repeat(32)}`);
  if (!await test.isDisabled()) throw new Error("Restore test did not require cleanup consent");
  await page.getByRole("checkbox", { name: /Confirm automatic cleanup/ }).check();
  await test.click();
  await page.waitForFunction(() => Boolean(window.testDatabaseBrowser.pending));
  await nav.getByTitle("Home", { exact: true }).click();
  await page.evaluate(() => window.testDatabaseBrowser.finishRestore());
  await nav.getByTitle("Backups & Restore", { exact: true }).click();
  await page.getByRole("heading", { name: "Restore Test Evidence", exact: true }).waitFor();
  await page.locator('section[aria-label="Restore test evidence"] summary').filter({ hasText: /passed.*Cleaned up/i }).waitFor();
  await page.evaluate(() => { window.testDatabaseBrowser.refuseTest = true; });
  await page.getByRole("button", { name: "Test Restore", exact: true }).click();
  await page.getByText("Cross-schema directive refused. No SQL was sent.", { exact: true }).first().waitFor();
  await page.screenshot({ path: "output/playwright/restore-test-evidence.png", fullPage: true });
  if (errors.length) throw new Error(errors.join("\n"));
  const state = await page.evaluate(() => ({ unknown: window.testDatabaseBrowser.unknown, commands: window.testDatabaseBrowser.calls.map((call) => call.command) }));
  if (state.unknown.length) throw new Error(`Missing mocks: ${state.unknown.join(", ")}`);
  if (state.commands.includes("restore_backup_snapshot")) throw new Error("Test flow invoked production restore");
  console.log("PASS: read-only defaults, NULLs, paging, structured filters, metadata, row previews/confirmations, conflict rejection, CSV, isolated restore consent, background navigation and saved refusal evidence.");
}
