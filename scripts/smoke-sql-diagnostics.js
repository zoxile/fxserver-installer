async (page) => {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.route("**/api/jg-artifacts/jsonv2", (route) => route.fulfill({
    json: { recommendedArtifact: "10000", windowsDownloadLink: "https://example.invalid/artifact.zip", brokenArtifacts: [] },
  }));
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.addInitScript(() => {
    localStorage.clear();
    const state = window.sqlDiagnosticTest = { calls: [], unknown: [], inspections: [], executions: [], counter: 0, inspectionMode: "normal", pending: [] };
    state.report = (table) => ({
      environment: { version: "12.2.fixture", sqlMode: "STRICT_TRANS_TABLES", charset: "utf8mb4", collation: "utf8mb4_general_ci", schemaCollation: "utf8mb4_unicode_ci" },
      columns: table === "missing" ? [] : [
        { table: table || "players", column: "identifier", columnType: "varchar(64)", charset: "utf8mb4", collation: "utf8mb4_unicode_ci", engine: "InnoDB" },
        ...(!table ? [{ table: "jobs", column: "name", columnType: "varchar(100)", charset: "utf8mb4", collation: "utf8mb4_general_ci", engine: "InnoDB" }] : []),
      ],
      foreignKeys: table === "players" ? [{ constraint: "players_identifier_fk", column: "identifier", parentDatabase: "accounts", parentTable: "users", parentColumn: "identifier", parentType: null, parentCollation: null }] : [],
      truncated: table === "partial",
    });
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
      transformCallback: () => ++state.counter,
      invoke: async (command, args = {}) => {
        state.calls.push(command);
        switch (command) {
          case "load_database_login": return { host: "localhost", port: 3306, username: "fixture", password: "private-fixture", database: "fixture" };
          case "save_database_login":
          case "clear_database_login":
          case "initialize_health_workspace":
          case "append_app_log":
          case "plugin:event|unlisten":
          case "validate_mariadb_credentials": return;
          case "plugin:event|listen": return args.handler;
          case "plugin:window|title": return "FXServer Installer";
          case "plugin:app|version": return "0.5.0";
          case "plugin:path|resolve_directory": return "C:/fixture/backups";
          case "configure_live_bridge": return { workspaceId: "default", enabled: false, connected: false, snapshot: null };
          case "read_app_logs": return { path: "mock.log", entries: [] };
          case "get_windows_artifact_metadata": return { recommendedArtifact: "10000", windowsDownloadLink: "https://example.invalid/artifact.zip", brokenArtifacts: [] };
          case "get_installed_windows_artifact_info": return { installed: false };
          case "fetch_latest_app_release": return null;
          case "get_default_mariadb_backup_output_dir": return "C:/fixture/backups";
          case "list_mariadb_databases": return ["fixture", "other_fixture"];
          case "plugin:dialog|open": return "C:/fixture/rejected.sql";
          case "read_text_file": return "SELECT 'file-rejection-fixture';";
          case "execute_mariadb_query":
            state.executions.push(args);
            if (args.query === "SELECT 'query-rejection-fixture';") throw new Error("ERROR 2013: Lost connection during the query fixture.");
            if (args.query === "SELECT 'file-rejection-fixture';") throw "ERROR 1064: Rejected SQL file fixture.";
            return { success: false, stdout: "", stderr: "ERROR 1267 (HY000): Illegal mix of collations", columns: [], rows: [] };
          case "inspect_database_sql":
            state.inspections.push(args);
            if (state.inspectionMode === "pending") return new Promise((resolve, reject) => state.pending.push({ resolve, reject, table: args.table }));
            if (state.inspectionMode === "failure") throw new Error("Fixture metadata permission denied.");
            return state.report(args.table);
          default: state.unknown.push(command); throw new Error(`Unmocked command: ${command}`);
        }
      },
    };
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  const nav = page.getByRole("navigation", { name: "Workspace navigation" });
  await nav.getByTitle("MariaDB", { exact: true }).click();
  await nav.getByTitle("Queries & Files", { exact: true }).click();
  await page.getByText("Credentials validated.", { exact: true }).waitFor();
  if (await page.getByRole("textbox", { name: "Admin Username", exact: true }).inputValue() !== "fixture") throw new Error("Remembered login was not restored");
  if (!await page.getByRole("checkbox", { name: "Remember login for this workspace" }).isChecked()) throw new Error("Remember preference was not restored");

  const diagnostics = page.locator("details").filter({ has: page.locator("summary").filter({ hasText: "SQL diagnostics" }) }).last();
  const ensureOpen = async () => {
    if (!await diagnostics.evaluate((element) => element.open)) await diagnostics.locator(":scope > summary").click();
  };
  const chooseScope = async (name) => {
    await page.getByTitle("Choose whether the query runs globally or inside one database", { exact: true }).click();
    await page.getByRole("option", { name, exact: true }).click();
    await ensureOpen();
  };
  const inspect = () => diagnostics.getByRole("button", { name: "Inspect schema", exact: true }).click();
  const table = diagnostics.getByRole("textbox", { name: "Table name (optional)", exact: true });
  const shown = (text) => diagnostics.getByText(text, { exact: true }).waitFor();
  const idle = "Not inspected yet. Inspect schema to see the current table definitions for this selection.";
  await ensureOpen();
  await shown("No SQL error to explain. You can still inspect the selected database without running a query or file.");
  await shown(idle);
  if (await page.evaluate(() => window.sqlDiagnosticTest.inspections.length) !== 0) throw new Error("Diagnostics inspected automatically");

  await chooseScope("Global query");
  await shown("Global scope has no selected database. Choose a database in the scope above to inspect its tables. A USE statement in your SQL does not change this selection.");
  if (!await diagnostics.getByRole("button", { name: "Inspect schema", exact: true }).isDisabled()) throw new Error("Global inspection was enabled");
  await chooseScope("fixture");
  await page.getByTitle("SQL query to execute against MariaDB.", { exact: true }).fill("SELECT VERSION();");
  await page.getByRole("button", { name: "Execute", exact: true }).click();
  await shown("Last SQL error");
  await shown("1267: Incompatible text collations");
  await shown("The statement compares or combines text with comparison rules the server cannot use together.");

  // A completed read must not reappear after switching away and back.
  await page.evaluate(() => { window.sqlDiagnosticTest.inspectionMode = "pending"; });
  await inspect();
  await shown("Reading schema metadata...");
  if (!await diagnostics.getByRole("button", { name: "Inspecting...", exact: true }).isDisabled() || !await table.isDisabled()) throw new Error("Inspection loading controls stayed enabled");
  await chooseScope("other_fixture");
  await shown("Last SQL error");
  await shown(idle);
  await chooseScope("fixture");
  await page.evaluate(() => {
    const state = window.sqlDiagnosticTest;
    const pending = state.pending.shift();
    pending.resolve(state.report(pending.table));
    state.inspectionMode = "normal";
  });
  await shown(idle);
  if (await diagnostics.getByText("Schema inspection complete", { exact: true }).isVisible()) throw new Error("Stale schema result appeared");

  await inspect();
  await shown("Schema inspection complete");
  await shown("2 visible columns across 2 tables.");
  await shown("Multiple collations are not automatically an error. Compare only the columns involved in the failing statement.");
  if (await diagnostics.getByRole("cell", { name: "players.identifier", exact: true }).isVisible()) throw new Error("Raw columns dominate the initial result");
  if (await diagnostics.getByText("12.2.fixture", { exact: true }).isVisible()) throw new Error("Raw server metadata was expanded by default");
  await diagnostics.getByText("Column details (2)", { exact: true }).click();
  await diagnostics.getByRole("cell", { name: "players.identifier", exact: true }).waitFor();
  await diagnostics.getByRole("columnheader", { name: "Character set", exact: true }).waitFor();
  await diagnostics.getByText("Server details", { exact: true }).click();
  await shown("12.2.fixture");
  await shown("To review existing foreign keys, enter a table name and inspect again. Relationships are not fetched for an all-table inspection.");

  await table.fill("missing");
  await shown(idle);
  await inspect();
  await shown("No visible table columns found");
  await shown("0 visible columns across 0 tables.");
  await shown("Check the exact table name and selected database, or clear the optional table field to inspect all visible tables. The table may be missing, a view, or hidden from this account.");
  if (await diagnostics.getByText(/^Column details/).count()) throw new Error("Empty result showed an empty metadata table");

  await table.fill("partial");
  await inspect();
  await shown("Partial schema inspection");
  await shown("1 visible column across 1 table in this limited result.");
  await shown("Results are limited to 500 columns and 500 foreign-key pairs. Some metadata is omitted. Enter a table name to narrow an all-table inspection.");
  await diagnostics.getByText("Existing foreign keys (0)", { exact: true }).click();
  await shown("No existing foreign keys were returned for this table. A relationship rejected by the server will not appear here.");

  await page.evaluate(() => { window.sqlDiagnosticTest.inspectionMode = "failure"; });
  await inspect();
  await shown("Schema inspection could not finish");
  await shown("Fixture metadata permission denied.");
  if (await diagnostics.getByText("Partial schema inspection", { exact: true }).isVisible()) throw new Error("Failed refresh retained a misleading previous result");
  await table.fill("players");
  await shown(idle);
  if (await diagnostics.getByText("Fixture metadata permission denied.", { exact: true }).isVisible()) throw new Error("Failure leaked into a different table scope");

  // Connection edits invalidate both pending success and pending failure.
  await page.evaluate(() => { window.sqlDiagnosticTest.inspectionMode = "pending"; });
  await inspect();
  await shown("Reading schema metadata...");
  await page.getByRole("textbox", { name: "Admin Username", exact: true }).fill("fixture-edited");
  await shown("Validate the connection above before inspecting a database. Error explanations are available without a validated connection.");
  await page.evaluate(() => {
    const state = window.sqlDiagnosticTest;
    state.pending.shift().reject(new Error("Stale inspection failure"));
    state.inspectionMode = "normal";
  });
  if (await diagnostics.getByText("Stale inspection failure", { exact: true }).isVisible()) throw new Error("Stale failure appeared after connection edit");
  if (!await diagnostics.getByRole("button", { name: "Inspect schema", exact: true }).isDisabled()) throw new Error("Unvalidated credentials allowed inspection");
  await page.getByRole("textbox", { name: "Admin Username", exact: true }).fill("fixture");
  if (await diagnostics.getByRole("button", { name: "Inspect schema", exact: true }).isDisabled()) {
    await page.getByRole("button", { name: /Change Credentials|Validate connection|Validate credentials|Connect/i }).click();
  }
  await page.getByText("Credentials validated.", { exact: true }).waitFor();
  await ensureOpen();
  await shown(idle);
  await table.fill("  players  ");
  await inspect();
  await shown("Schema inspection complete");
  await diagnostics.getByText("Column details (1)", { exact: true }).click();
  await diagnostics.getByRole("cell", { name: "players.identifier", exact: true }).waitFor();
  await diagnostics.getByText("Server details", { exact: true }).click();
  await diagnostics.getByText("Existing foreign keys (1)", { exact: true }).click();
  await shown("players_identifier_fk");
  await shown("Referenced type: Not visible. Referenced collation: Not returned (non-text or not visible).");
  const requests = await page.evaluate(() => window.sqlDiagnosticTest.inspections);
  if (requests[0].table !== null || requests.at(-1).table !== "players") throw new Error("Optional table was not normalized correctly");
  if (requests.some((args) => "query" in args || args.database === "__global__")) throw new Error("Inspection received SQL or global scope");
  if (await page.evaluate(() => window.sqlDiagnosticTest.calls.filter((command) => command === "execute_mariadb_query").length) !== 1) throw new Error("Diagnostics reran the failed query");
  const stored = await page.evaluate(() => Object.values(localStorage).join(" "));
  if (stored.includes("private-fixture")) throw new Error("Remembered password leaked into browser storage");
  await page.getByRole("checkbox", { name: "Remember login for this workspace" }).uncheck();
  await page.waitForFunction(() => window.sqlDiagnosticTest.calls.includes("clear_database_login"));

  await diagnostics.scrollIntoViewIfNeeded();
  await page.screenshot({ path: "output/playwright/sql-diagnostics-desktop.png", fullPage: true });
  for (const width of [480, 360]) {
    await page.setViewportSize({ width, height: 800 });
    await diagnostics.scrollIntoViewIfNeeded();
    if (await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)) throw new Error(`SQL diagnostics overflow at ${width}px width`);
    const bounds = await diagnostics.boundingBox();
    if (!bounds || bounds.x < 0 || bounds.x + bounds.width > width) throw new Error(`Diagnostics panel escaped the ${width}px viewport`);
    await page.screenshot({ path: `output/playwright/sql-diagnostics-narrow-${width}.png`, fullPage: true });
  }

  // Rejected invocations belong to the operation that failed, even after the other operation runs.
  await page.setViewportSize({ width: 1440, height: 1000 });
  const fileDiagnostics = page.locator("details").filter({ has: page.locator("summary").filter({ hasText: "SQL diagnostics" }) }).first();
  if (!await fileDiagnostics.evaluate((element) => element.open)) await fileDiagnostics.locator(":scope > summary").click();
  await fileDiagnostics.getByText("No SQL error to explain. You can still inspect the selected database without running a query or file.", { exact: true }).waitFor();
  await page.getByTitle("SQL query to execute against MariaDB.", { exact: true }).fill("SELECT 'query-rejection-fixture';");
  await page.getByRole("button", { name: "Execute", exact: true }).click();
  await shown("Last SQL error");
  await shown("2013: Connection failed or was lost");
  await shown("After a lost connection, the result of a write may be unknown. Check the database before retrying.");
  if (await diagnostics.getByText("1267: Incompatible text collations", { exact: true }).count()) throw new Error("Rejected query kept the previous query diagnosis");
  if (await fileDiagnostics.getByText("Last SQL error", { exact: true }).count()) throw new Error("Rejected query leaked into SQL file diagnostics");
  if (!await page.getByRole("button", { name: "Execute", exact: true }).isDisabled()) throw new Error("Lost connection did not require explicit validation");
  if (await page.evaluate(() => window.sqlDiagnosticTest.executions.length) !== 2) throw new Error("Rejected query was automatically retried");

  await page.getByRole("button", { name: "Change Credentials", exact: true }).click();
  await page.getByText("Credentials validated.", { exact: true }).waitFor();
  await page.getByTitle("Browse for a .sql file", { exact: true }).click();
  if (await page.getByPlaceholder("SQL file contents will appear here.", { exact: true }).inputValue() !== "SELECT 'file-rejection-fixture';") throw new Error("SQL file fixture was not loaded");
  await page.getByTitle("Run SQL file", { exact: true }).click();
  await fileDiagnostics.getByText("Last SQL error", { exact: true }).waitFor();
  await fileDiagnostics.getByText("1064: SQL syntax rejected", { exact: true }).waitFor();
  await fileDiagnostics.getByText("The server could not parse part of the statement. Schema inspection alone cannot validate SQL syntax.", { exact: true }).waitFor();
  await shown("2013: Connection failed or was lost");
  if (await diagnostics.getByText("1064: SQL syntax rejected", { exact: true }).count()) throw new Error("Rejected file overwrote query diagnostics");
  if (await fileDiagnostics.getByText("2013: Connection failed or was lost", { exact: true }).count()) throw new Error("Query rejection leaked into file diagnostics");
  if (await page.evaluate(() => window.sqlDiagnosticTest.inspections.length) !== requests.length) throw new Error("Invocation rejection automatically inspected the schema");
  await fileDiagnostics.getByRole("button", { name: "Inspect schema", exact: true }).click();
  await fileDiagnostics.getByText("Schema inspection complete", { exact: true }).waitFor();
  await fileDiagnostics.getByText("1064: SQL syntax rejected", { exact: true }).waitFor();
  await shown("2013: Connection failed or was lost");
  const executedSql = await page.evaluate(() => window.sqlDiagnosticTest.executions.map((args) => args.query));
  if (JSON.stringify(executedSql) !== JSON.stringify(["SELECT VERSION();", "SELECT 'query-rejection-fixture';", "SELECT 'file-rejection-fixture';"])) throw new Error(`Unexpected SQL execution or automatic retry: ${JSON.stringify(executedSql)}`);

  const unknown = await page.evaluate(() => window.sqlDiagnosticTest.unknown);
  if (errors.length || unknown.length) throw new Error(JSON.stringify({ errors, unknown }));
  return "PASS: explained result errors and rejected query/file invocations in the correct panel; explicit read-only inspection; global/unvalidated/loading/empty/partial/failure states; stale result and failure isolation; optional table and charset metadata; no SQL retry, plaintext storage or responsive overflow.";
}
