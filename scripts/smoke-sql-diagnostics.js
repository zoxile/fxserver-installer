async (page) => {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.addInitScript(() => {
    localStorage.clear();
    const state = window.sqlDiagnosticTest = { calls: [], unknown: [], counter: 0 };
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
          case "plugin:app|version": return "0.4.1";
          case "plugin:path|resolve_directory": return "C:/fixture/backups";
          case "configure_live_bridge": return { workspaceId: "default", enabled: false, connected: false, snapshot: null };
          case "read_app_logs": return { path: "mock.log", entries: [] };
          case "get_windows_artifact_metadata": return { recommendedArtifact: "10000", windowsDownloadLink: "https://example.invalid/artifact.zip", brokenArtifacts: [] };
          case "get_installed_windows_artifact_info": return { installed: false };
          case "fetch_latest_app_release": return null;
          case "get_default_mariadb_backup_output_dir": return "C:/fixture/backups";
          case "list_mariadb_databases": return ["fixture"];
          case "execute_mariadb_query": return { success: false, stdout: "", stderr: "ERROR 1267 (HY000): Illegal mix of collations", columns: [], rows: [] };
          case "inspect_database_sql": return { environment: { version: "12.2.fixture", sqlMode: "STRICT_TRANS_TABLES", charset: "utf8mb4", collation: "utf8mb4_general_ci", schemaCollation: "utf8mb4_unicode_ci" }, columns: [{ table: "players", column: "identifier", columnType: "varchar(64)", charset: "utf8mb4", collation: "utf8mb4_unicode_ci", engine: "InnoDB" }], foreignKeys: [], truncated: false };
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
  await page.getByTitle("SQL query to execute against MariaDB.", { exact: true }).fill("SELECT VERSION();");
  await page.getByRole("button", { name: "Execute", exact: true }).click();
  await page.getByText("1267: Incompatible text collations", { exact: true }).waitFor();
  await page.getByRole("button", { name: "Inspect schema", exact: true }).last().click();
  await page.getByText("12.2.fixture", { exact: true }).waitFor();
  await page.getByRole("cell", { name: "players.identifier", exact: true }).waitFor();
  if (await page.evaluate(() => window.sqlDiagnosticTest.calls.filter((command) => command === "execute_mariadb_query").length) !== 1) throw new Error("Diagnostics reran the failed query");
  const stored = await page.evaluate(() => Object.values(localStorage).join(" "));
  if (stored.includes("private-fixture")) throw new Error("Remembered password leaked into browser storage");
  await page.getByRole("checkbox", { name: "Remember login for this workspace" }).uncheck();
  await page.waitForFunction(() => window.sqlDiagnosticTest.calls.includes("clear_database_login"));
  await page.screenshot({ path: "output/playwright/sql-diagnostics-desktop.png", fullPage: true });
  await page.setViewportSize({ width: 480, height: 800 });
  if (await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)) throw new Error("SQL diagnostics overflow at narrow width");
  await page.screenshot({ path: "output/playwright/sql-diagnostics-narrow.png", fullPage: true });
  const unknown = await page.evaluate(() => window.sqlDiagnosticTest.unknown);
  if (errors.length || unknown.length) throw new Error(JSON.stringify({ errors, unknown }));
  return "PASS: restored opt-in login, no plaintext storage, validated query, review-only error guidance, bounded schema inspection, forget and responsive layout.";
}
