async (page) => {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.addInitScript(() => {
    localStorage.clear();
    const record = (query, seconds = 1, rows = 100) => `# Time: 260923 10:00:00\n# User@Host: private-user @ private-host\n# Thread_id: 1 Schema: fixture QC_hit: No\n# Query_time: ${seconds} Lock_time: 0 Rows_sent: 1 Rows_examined: ${rows}\nSET timestamp=1790150400;\n${query}\n`;
    const state = window.slowQueryTest = {
      calls: [], counter: 0, mode: "normal", pending: [], path: "C:/fixture/slow.log",
      content: record("SELECT * FROM players WHERE token='private-literal';", 10, 1000) + record("SELECT * FROM players WHERE token='<script>alert(1)</script>';", 20, 3000) + record("SELECT evil_udf('private-udf');", 3) + record("DELETE FROM players WHERE token='private-delete';", 2) + Array.from({ length: 32 }, (_, i) => record(`SELECT * FROM table_${i};`, i / 10)).join("") + record("SELECT 1; DROP DATABASE fixture;") + record("SELECT /*!50000 evil_udf(1) */ 1;") + record("SELECT 1;", "Infinity"),
    };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
      transformCallback: () => ++state.counter,
      invoke: async (command, args = {}) => {
        state.calls.push({ command, args });
        switch (command) {
          case "get_platform_info": return { os: "windows", arch: "x86_64", artifactPlatform: "windows", clientLogsSupported: true, secureStorageSupported: true };
          case "load_database_login": return { host: "localhost", port: 3306, username: "fixture", password: "fixture", database: "fixture" };
          case "save_database_login": case "clear_database_login": case "initialize_health_workspace": case "append_app_log": case "plugin:event|unlisten": case "validate_mariadb_credentials": return;
          case "plugin:event|listen": return args.handler;
          case "plugin:window|title": return "FXServer Installer";
          case "plugin:app|version": return "0.5.1";
          case "plugin:path|resolve_directory": return "C:/fixture/backups";
          case "configure_live_bridge": return { workspaceId: "default", enabled: false, connected: false, snapshot: null };
          case "read_app_logs": return { path: "mock.log", entries: [] };
          case "get_windows_artifact_metadata": return { recommendedArtifact: "10000", windowsDownloadLink: "https://example.invalid/artifact.zip", brokenArtifacts: [] };
          case "get_installed_windows_artifact_info": return { installed: false };
          case "get_app_update_status": return { phase: "disabled", currentVersion: "0.5.2", availableVersion: null, releaseUrl: null, downloadedBytes: 0, totalBytes: null, message: "Updates disabled in fixture.", supported: false };
          case "fetch_latest_app_release": return null;
          case "get_default_mariadb_backup_output_dir": return "C:/fixture/backups";
          case "list_mariadb_databases": return ["fixture", "other_fixture"];
          case "plugin:dialog|open": return state.mode === "cancel" ? null : state.path;
          case "read_text_file":
            if (args.maxBytes !== 4 * 1024 * 1024) throw new Error("Incorrect import limit");
            if (state.mode === "pending") return new Promise((resolve, reject) => state.pending.push({ resolve, reject }));
            if (state.mode === "failure") throw new Error("private-path private-literal");
            if (state.mode === "oversized") return "x".repeat(4 * 1024 * 1024 + 1);
            if (state.mode === "empty") return "unsupported log format";
            return state.content;
          case "execute_mariadb_query": throw new Error("Analyzer must never execute SQL");
          default: return null;
        }
      },
    };
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  const nav = page.getByRole("navigation", { name: "Workspace navigation" });
  await nav.getByTitle("MariaDB", { exact: true }).click();
  await nav.getByTitle("Queries & Files", { exact: true }).click();
  await page.getByText("Credentials validated.", { exact: true }).waitFor();
  const diagnostics = page.locator("details").filter({ has: page.locator("summary").filter({ hasText: "SQL diagnostics" }) }).last();
  await diagnostics.locator(":scope > summary").click();
  await diagnostics.getByText("Slow query analyzer", { exact: true }).click();
  const analyzer = diagnostics.getByRole("region", { name: "Slow query analyzer", exact: true });
  const importLog = () => analyzer.getByRole("button", { name: "Import slow query log", exact: true }).click();
  const imported = () => analyzer.getByText("36 statements in 35 groups. 3 records skipped.", { exact: true }).waitFor();
  const search = analyzer.getByRole("textbox", { name: "Search fingerprints or databases" });
  await importLog();
  await imported();
  if (await analyzer.getByRole("button", { name: /^Review query group/ }).count() !== 25) throw new Error("Unbounded query rendering");
  const body = await analyzer.innerText();
  if (/private-literal|private-user|private-host|alert\(1\)|DROP DATABASE|50000/.test(body)) throw new Error("Raw values or rejected statements appeared in analysis");
  await analyzer.getByRole("button", { name: "Next query groups", exact: true }).click();
  if (await analyzer.getByRole("button", { name: /^Review query group/ }).count() !== 10) throw new Error("Pagination failed");
  await search.fill("players");
  if (await analyzer.getByRole("button", { name: /^Review query group/ }).count() !== 2) throw new Error("Search failed");
  await analyzer.getByRole("button", { name: "Review query group 1", exact: true }).click();
  await analyzer.getByText("Rows examined per statement: 2,000 average, 3,000 maximum.", { exact: true }).waitFor();
  if (await analyzer.getByRole("textbox", { name: "Reviewed EXPLAIN draft" }).count()) throw new Error("Draft bypassed review");
  await analyzer.getByRole("checkbox", { name: /I reviewed the database scope/ }).check();
  const draft = analyzer.getByRole("textbox", { name: "Reviewed EXPLAIN draft" });
  if (!/EXPLAIN\nSELECT/.test(await draft.inputValue())) throw new Error("EXPLAIN draft missing");
  await analyzer.getByRole("button", { name: "Select draft", exact: true }).click();
  if (!await draft.evaluate((element) => element.selectionEnd === element.value.length)) throw new Error("Draft selection failed");
  await analyzer.getByRole("button", { name: "Review query group 3", exact: true }).click();
  await analyzer.getByText(/Drafts are limited to single SELECT/).waitFor();
  await search.fill("evil_udf");
  await analyzer.getByRole("button", { name: "Review query group 2", exact: true }).click();
  await analyzer.getByText(/Function calls are excluded/).waitFor();
  await search.fill("does-not-exist");
  await analyzer.getByText("No matching query groups.", { exact: true }).waitFor();
  await search.fill("");
  await analyzer.getByRole("button", { name: "Sort ascending", exact: true }).click();
  if (await analyzer.getByRole("button", { name: /^Review query group/ }).first().getAttribute("aria-label") !== "Review query group 4") throw new Error("Sorting failed");
  const assertLayout = async () => {
    const layout = await analyzer.evaluate((element) => {
      const bounds = element.getBoundingClientRect();
      const controls = [...element.querySelectorAll("input, button, textarea")].filter((control) => control.getBoundingClientRect().width > 0);
      return {
        client: element.clientWidth, scroll: element.scrollWidth, left: bounds.left, right: bounds.right, viewport: window.innerWidth,
        overflow: controls.some((control) => {
          const box = control.getBoundingClientRect();
          return !control.closest('[aria-label="Slow query groups"]') && (box.left < bounds.left - 1 || box.right > bounds.right + 1);
        }),
      };
    });
    if (layout.scroll > layout.client + 1 || layout.left < 0 || layout.right > layout.viewport + 1 || layout.overflow) throw new Error(`Analyzer layout overflow: ${JSON.stringify(layout)}`);
  };
  await assertLayout();
  await analyzer.getByRole("button", { name: "Import slow query log", exact: true }).scrollIntoViewIfNeeded();
  await page.screenshot({ path: "output/playwright/slow-query-analyzer-desktop.png" });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.waitForFunction(() => document.querySelector('nav[aria-label="Workspace navigation"]')?.closest("aside")?.getBoundingClientRect().width <= 65);
  await assertLayout();
  await analyzer.getByRole("button", { name: "Import slow query log", exact: true }).scrollIntoViewIfNeeded();
  await page.screenshot({ path: "output/playwright/slow-query-analyzer-mobile.png" });
  await page.setViewportSize({ width: 1440, height: 1000 });

  await page.evaluate(() => window.slowQueryTest.mode = "cancel");
  await importLog();
  await imported();
  await page.evaluate(() => window.slowQueryTest.mode = "failure");
  await importLog();
  await analyzer.getByText("Slow query import failed", { exact: true }).waitFor();
  if ((await analyzer.innerText()).includes("private-path")) throw new Error("Read error leaked sensitive data");
  await analyzer.getByRole("button", { name: "Clear analysis", exact: true }).click();
  await page.evaluate(() => window.slowQueryTest.mode = "oversized");
  await importLog();
  await analyzer.getByText("Slow query import failed", { exact: true }).waitFor();
  await analyzer.getByRole("button", { name: "Clear analysis", exact: true }).click();
  await page.evaluate(() => window.slowQueryTest.mode = "empty");
  await importLog();
  await analyzer.getByText("No complete supported slow-query records found.", { exact: true }).waitFor();

  await page.evaluate(() => window.slowQueryTest.mode = "pending");
  await importLog();
  await page.waitForFunction(() => window.slowQueryTest.pending.length === 1);
  await analyzer.getByRole("button", { name: "Cancel import and clear analysis", exact: true }).click();
  await page.evaluate(() => window.slowQueryTest.pending.shift().resolve(window.slowQueryTest.content));
  await analyzer.getByText("No slow query log imported.", { exact: true }).waitFor();
  await importLog();
  await page.waitForFunction(() => window.slowQueryTest.pending.length === 1);
  await page.getByTitle("Choose whether the query runs globally or inside one database", { exact: true }).click();
  await page.getByRole("option", { name: "other_fixture", exact: true }).click();
  await page.evaluate(() => window.slowQueryTest.pending.shift().resolve(window.slowQueryTest.content));
  await diagnostics.getByText("Slow query analyzer", { exact: true }).click();
  await analyzer.getByText("No slow query log imported.", { exact: true }).waitFor();
  const audit = await page.evaluate(() => ({ calls: window.slowQueryTest.calls, storage: Object.values(localStorage).join(" ") }));
  if (audit.calls.some((call) => call.command === "execute_mariadb_query" || call.command === "inspect_database_sql")) throw new Error("Unexpected database execution");
  const saved = audit.calls.filter((call) => /save|append_app_log/.test(call.command));
  if (/private-literal|private-user|private-host/.test(JSON.stringify(saved) + audit.storage)) throw new Error("Slow log data persisted");
  if (errors.length) throw new Error(errors.join("\n"));
  return { groups: 35, pageSize: 25, executionCalls: 0, viewports: [1440, 390], checked: "import, privacy, review gate, unsafe drafts, sorting, search, pagination, errors, cancellation, stale scope" };
}
