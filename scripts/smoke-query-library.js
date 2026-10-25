async (page) => {
  const assert = (condition, message) => { if (!condition) throw new Error(message); };
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.setViewportSize({ width: 1440, height: 1050 });
  await page.evaluate(() => localStorage.clear());
  await page.addInitScript(() => {
    const login = { host: "library-fixture.invalid", port: 3306, username: "library-fixture-user", password: "library-private-7!z", database: "game" };
    const state = window.testQueryLibrary = { calls: [], unknown: [], accepted: [], premature: [] };
    const signature = (credentials) => JSON.stringify([credentials.host, credentials.port, credentials.username, credentials.password]);
    const guarded = new Set(["list_mariadb_databases", "list_mariadb_tables", "get_database_browser_metadata", "get_database_browser_rows", "execute_mariadb_query"]);
    let sequence = 0;
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
      transformCallback: () => ++sequence,
      invoke: async (command, args = {}) => {
        state.calls.push({ command, args: structuredClone(args) });
        if (guarded.has(command) && !state.accepted.includes(signature(args.credentials))) {
          state.premature.push(command);
          throw new Error("Fixture command was called without validation");
        }
        switch (command) {
          case "get_platform_info": return { os: "windows", arch: "x86_64", artifactPlatform: "windows", clientLogsSupported: true, secureStorageSupported: true };
          case "get_app_update_status": return { phase: "disabled", currentVersion: "0.5.2", availableVersion: null, releaseUrl: null, downloadedBytes: 0, totalBytes: null, message: "Updates disabled in fixture.", supported: false };
          case "fetch_latest_app_release": return null;
          case "plugin:window|title": return "FXServer Installer";
          case "plugin:app|version": return "0.5.1";
          case "plugin:path|resolve_directory": return "C:/fixture/backups";
          case "plugin:event|listen": return args.handler;
          case "plugin:event|unlisten":
          case "append_app_log":
          case "initialize_health_workspace":
          case "save_database_login":
          case "clear_database_login": return;
          case "load_database_login": return { ...login };
          case "validate_mariadb_credentials": state.accepted.push(signature(args.credentials)); return;
          case "configure_live_bridge": return { workspaceId: "default", enabled: false, connected: false, snapshot: null };
          case "read_app_logs": return { path: "mock.log", entries: [] };
          case "get_windows_artifact_metadata": return { recommendedArtifact: "10000", windowsDownloadLink: "https://example.invalid/artifact.zip", brokenArtifacts: [] };
          case "get_installed_windows_artifact_info": return { installed: false };
          case "list_txdata_profiles": return { dataPath: "", profiles: [], hasRootLogs: false };
          case "list_mariadb_databases": return ["game", "other_game"];
          case "list_mariadb_tables": return ["players", "inventory"];
          case "get_database_browser_metadata": return {
            columns: ["id", "name"].map((name) => ({ name, columnType: name === "id" ? "int" : "varchar(128)", nullable: false, defaultValue: null, extra: "", binary: false })),
            indexes: [{ name: "PRIMARY", column: "id", sequence: 1, unique: true, indexType: "BTREE", prefixLength: null }], editable: true,
          };
          case "get_database_browser_rows": return {
            rows: [[args.request.filters[0]?.value || "1", `${args.request.table}-result-never-store`]],
            hasMore: true, truncatedCells: false, pageSize: args.request.pageSize,
          };
          case "execute_mariadb_query": return { success: true, columns: ["result"], rows: [["query-result-never-store"]], stdout: "", stderr: "" };
          default: state.unknown.push(command); throw new Error(`Unmocked query library command: ${command}`);
        }
      },
    };
  });
  const calls = (command) => page.evaluate((command) => window.testQueryLibrary.calls.filter((call) => call.command === command), command);
  const savedData = () => page.evaluate(() => JSON.parse(localStorage.getItem("fxserver-installer.database-library.v1") || "null"));
  const navigate = async (title) => {
    const nav = page.getByRole("navigation", { name: "Workspace navigation" });
    const group = nav.getByTitle("MariaDB", { exact: true });
    if (await group.getAttribute("aria-expanded") !== "true") await group.click();
    await nav.getByTitle(title, { exact: true }).click();
    await page.getByRole("heading", { name: title, exact: true }).waitFor();
  };
  const openDetails = async (name) => {
    const details = page.locator("details").filter({ has: page.locator("summary").filter({ hasText: name }) });
    if (!await details.evaluate((element) => element.open)) await details.locator(":scope > summary").click();
    return details;
  };
  const choose = async (trigger, label) => {
    await trigger.click();
    await page.getByRole("option", { name: label, exact: true }).click();
  };
  const noOverflow = async (name) => {
    const sizes = await page.evaluate(() => ({ width: document.documentElement.clientWidth, scroll: document.documentElement.scrollWidth }));
    assert(sizes.scroll <= sizes.width + 1, `${name} overflows horizontally: ${JSON.stringify(sizes)}`);
  };
  const noControlOverlap = async (root, name) => {
    const overlaps = await root.evaluate((element) => {
      const controls = [...element.querySelectorAll('input:not([type="hidden"]), button, textarea')]
        .map((control) => ({ label: control.getAttribute("aria-label") || control.getAttribute("title") || control.tagName, box: control.getBoundingClientRect() }))
        .filter(({ box }) => box.width > 4 && box.height > 4);
      return controls.flatMap((left, index) => controls.slice(index + 1).filter((right) =>
        Math.min(left.box.right, right.box.right) - Math.max(left.box.left, right.box.left) > 1
        && Math.min(left.box.bottom, right.box.bottom) - Math.max(left.box.top, right.box.top) > 1
      ).map((right) => `${left.label} overlaps ${right.label}`));
    });
    assert(!overlaps.length, `${name}: ${overlaps.join("; ")}`);
  };
  const expectStored = async (kind, length) => page.waitForFunction(({ kind, length }) => JSON.parse(localStorage.getItem("fxserver-installer.database-library.v1") || "{}")[kind]?.length === length, { kind, length });

  await page.reload({ waitUntil: "domcontentloaded" });
  await navigate("Queries & Files");
  const editor = page.getByRole("textbox", { name: "SQL query", exact: true });
  await editor.fill("SELECT name FROM players WHERE id = {{id:number}} AND name = {{name:text}} AND active = {{active:boolean}} LIMIT {{maximum:number}} OFFSET {{offset:number}};");
  let library = await openDetails(/^Saved Queries/);
  await library.getByLabel("Folder name", { exact: true }).fill("Operations");
  await library.getByRole("button", { name: "Create folder", exact: true }).click();
  await expectStored("folders", 1);
  await library.getByLabel("Query name", { exact: true }).fill("Player lookup");
  await library.getByRole("button", { name: "Save New", exact: true }).click();
  await expectStored("queries", 1);
  let saved = await savedData();
  assert(saved.queries[0].folderId === saved.folders[0].id && saved.folders[0].name === "Operations", "Named folder was not stored with the query");
  await library.getByLabel("Folder name", { exact: true }).fill("Reports");
  await library.getByRole("button", { name: "Rename folder", exact: true }).click();
  await page.waitForFunction(() => JSON.parse(localStorage.getItem("fxserver-installer.database-library.v1")).folders[0].name === "Reports");
  await editor.fill("SELECT 0;");
  await library.getByRole("button", { name: "Load", exact: true }).click();
  assert((await editor.inputValue()).includes("{{id:number}}"), "Loading did not restore the template");
  assert((await calls("execute_mariadb_query")).length === 0, "Loading a saved query executed SQL");

  const injection = "volatile-input-42'; DROP TABLE players; --";
  await page.getByLabel("id (number)", { exact: true }).fill("42 OR 1=1");
  await page.getByLabel("name (text)", { exact: true }).fill(injection);
  await page.getByLabel("maximum (number)", { exact: true }).fill("20");
  await page.getByLabel("offset (number)", { exact: true }).fill("40");
  const booleanParameter = page.getByLabel("active (boolean)", { exact: true });
  assert(await booleanParameter.getAttribute("data-slot") === "select-trigger", "Boolean parameter did not reuse the existing Select component");
  assert(await page.locator("select").count() === 0, "Native select introduced into the query console");
  await choose(booleanParameter, "false");
  await page.getByRole("button", { name: "Review", exact: true }).click();
  await page.getByText("Enter a plain finite number for id.", { exact: true }).waitFor();
  assert((await calls("execute_mariadb_query")).length === 0, "An invalid numeric parameter executed");
  await page.getByLabel("id (number)", { exact: true }).fill("42");
  await page.getByRole("button", { name: "Review", exact: true }).click();
  let review = page.getByRole("region", { name: "SQL execution confirmation" });
  await review.waitFor();
  assert(!(await review.locator("pre").textContent()).includes("DROP TABLE"), "Text input escaped its encoded value");
  assert(await review.getByRole("button", { name: "Execute", exact: true }).isDisabled(), "Confirmation was optional");
  await review.getByLabel("Type RUN", { exact: true }).fill("NO");
  assert(await review.getByRole("button", { name: "Execute", exact: true }).isDisabled(), "Wrong confirmation enabled SQL");
  await review.getByRole("button", { name: "Cancel execution", exact: true }).click();
  assert((await calls("execute_mariadb_query")).length === 0, "Cancelled SQL was executed");
  await page.getByRole("button", { name: "Review", exact: true }).click();
  await review.getByLabel("Type RUN", { exact: true }).fill("RUN");
  await review.getByRole("button", { name: "Execute", exact: true }).click();
  await page.getByRole("cell", { name: "query-result-never-store", exact: true }).waitFor();
  const executed = await calls("execute_mariadb_query");
  assert(executed.length === 1 && /\bid\s*=\s+42\s+AND\b/.test(executed[0].args.query) && executed[0].args.query.includes("(FALSE)") && executed[0].args.query.includes("CONVERT(X'"), "Reviewed draft did not use the existing execution path exactly once");
  assert(/\bLIMIT\s+20\s+OFFSET\s+40\s*;/.test(executed[0].args.query), "LIMIT/OFFSET parameters were not plain integer literals");
  assert(!executed[0].args.query.includes("DROP TABLE") && (await editor.inputValue()).includes("{{id:number}}"), "Unsafe text execution or template loss");
  assert(await page.getByLabel("name (text)", { exact: true }).inputValue() === "", "Parameter values were retained after execution");
  assert((await booleanParameter.textContent()).includes("Choose value"), "Boolean parameter was retained after execution");

  await editor.fill("DELETE FROM players WHERE id = {{id:number}};");
  await library.getByLabel("Query name", { exact: true }).fill("Delete player");
  await library.getByRole("button", { name: "Save New", exact: true }).click();
  await expectStored("queries", 2);
  await editor.fill("SELECT 1;");
  await library.getByRole("button", { name: "Load", exact: true }).click();
  await page.getByLabel("id (number)", { exact: true }).fill("42");
  await page.getByRole("button", { name: "Review", exact: true }).click();
  await review.getByLabel("Type RUN", { exact: true }).fill("NO");
  assert(await review.getByRole("button", { name: "Execute", exact: true }).isDisabled(), "Saved destructive query bypassed confirmation");
  await page.screenshot({ path: "output/playwright/query-library-wide.png", fullPage: true });
  for (const width of [1000, 480, 360]) {
    await page.setViewportSize({ width, height: 900 });
    await review.scrollIntoViewIfNeeded();
    await noOverflow(`Query library at ${width}px`);
    await noControlOverlap(library, `Saved query controls at ${width}px`);
    await noControlOverlap(review, `SQL confirmation controls at ${width}px`);
    await page.screenshot({ path: `output/playwright/query-library-narrow-${width}.png`, fullPage: true });
  }
  await review.getByRole("button", { name: "Cancel execution", exact: true }).click();
  assert((await calls("execute_mariadb_query")).length === 1, "Rejected destructive query reached the backend");

  await editor.fill("SELECT 'library-private-7!z';");
  await library.getByLabel("Query name", { exact: true }).fill("Sensitive query");
  await library.getByRole("button", { name: "Save New", exact: true }).click();
  await library.getByText(/Cannot save:/).waitFor();
  assert((await savedData()).queries.length === 2, "Known credential was persisted in SQL");
  await page.setViewportSize({ width: 1440, height: 1050 });
  await navigate("Database Browser");
  await page.getByRole("cell", { name: "players-result-never-store", exact: true }).waitFor();
  let bookmarks = await openDetails(/^Bookmarks/);
  await page.getByRole("button", { name: "Filter", exact: true }).click();
  await page.getByRole("textbox", { name: "Filter 1 value", exact: true }).fill("42");
  await page.getByRole("button", { name: "Apply", exact: true }).click();
  await page.getByRole("cell", { name: "42", exact: true }).waitFor();
  await page.getByTitle("Sort id", { exact: true }).click();
  await page.getByRole("button", { name: "Next page", exact: true }).click();
  await page.getByRole("checkbox", { name: "Enable row editing", exact: true }).check();
  await bookmarks.getByLabel("Bookmark name", { exact: true }).fill("Filtered players");
  await bookmarks.getByRole("button", { name: "Save New", exact: true }).click();
  await expectStored("bookmarks", 1);
  const storedView = (await savedData()).bookmarks[0];
  assert(storedView.filters[0].value === "42" && storedView.offset === 25 && storedView.descending && storedView.view === "rows", "Bookmark did not capture applied filter/sort/page");
  await page.getByRole("textbox", { name: "Filter 1 value", exact: true }).fill("unapplied-draft");
  await bookmarks.getByRole("button", { name: "Update View", exact: true }).click();
  assert((await savedData()).bookmarks[0].filters[0].value === "42", "Bookmark persisted an unapplied filter draft");
  await choose(page.locator("#browser-table"), "inventory");
  await page.getByRole("cell", { name: "inventory-result-never-store", exact: true }).waitFor();
  await bookmarks.getByRole("button", { name: "Restore", exact: true }).click();
  await page.getByText("Bookmark restored in read-only mode.", { exact: true }).waitFor();
  assert(await page.getByRole("textbox", { name: "Filter 1 value", exact: true }).inputValue() === "42", "Restored filters were lost");
  assert(!await page.getByRole("checkbox", { name: "Enable row editing", exact: true }).isChecked(), "Bookmark restored write access");
  assert(await page.getByRole("button", { name: "Previous page", exact: true }).isEnabled(), "Bookmark lost its page offset");
  await page.screenshot({ path: "output/playwright/database-bookmarks-wide.png", fullPage: true });

  await page.getByRole("tab", { name: "columns", exact: true }).click();
  await bookmarks.getByLabel("Bookmark name", { exact: true }).fill("Player columns");
  await bookmarks.getByRole("button", { name: "Save New", exact: true }).click();
  await expectStored("bookmarks", 2);
  await page.getByRole("tab", { name: "rows", exact: true }).click();
  await bookmarks.getByRole("button", { name: "Restore", exact: true }).click();
  await page.getByRole("tab", { name: "columns", exact: true }).waitFor();
  assert(await page.getByRole("tab", { name: "columns", exact: true }).getAttribute("aria-selected") === "true", "Column bookmark did not restore its view");
  await choose(bookmarks.locator('[data-slot="select-trigger"]'), "Filtered players");
  for (const width of [1000, 480, 360]) {
    await page.setViewportSize({ width, height: 900 });
    await bookmarks.getByRole("button", { name: "Restore", exact: true }).click();
    await page.getByRole("cell", { name: "players-result-never-store", exact: true }).waitFor();
    await bookmarks.scrollIntoViewIfNeeded();
    await noOverflow(`Bookmarks at ${width}px`);
    await noControlOverlap(bookmarks, `Bookmark controls at ${width}px`);
    await page.screenshot({ path: `output/playwright/database-bookmarks-narrow-${width}.png`, fullPage: true });
  }
  await page.setViewportSize({ width: 1440, height: 1050 });

  const beforeNavigation = (await calls("get_database_browser_rows")).length;
  const validations = (await calls("validate_mariadb_credentials")).length;
  await navigate("Queries & Files");
  library = await openDetails(/^Saved Queries/);
  await choose(library.locator('[data-slot="select-trigger"]').first(), "Reports / Player lookup");
  await library.getByRole("button", { name: "Load", exact: true }).click();
  assert(await page.getByLabel("name (text)", { exact: true }).inputValue() === "", "Navigation restored parameter values");
  await navigate("Database Browser");
  await page.getByRole("cell", { name: "players-result-never-store", exact: true }).waitFor();
  assert((await calls("get_database_browser_rows")).length === beforeNavigation, "Warm bookmark navigation bypassed the shared cache");
  assert((await calls("validate_mariadb_credentials")).length === validations, "Navigation repeated session validation");

  const libraryJson = JSON.stringify(await savedData());
  for (const forbidden of ["library-private-7!z", "mysql://", "library-fixture.invalid", "library-fixture-user", "parameterValues", "result-never-store", "volatile-input-42", "unapplied-draft", "editMode"]) assert(!libraryJson.includes(forbidden), `Forbidden runtime content persisted: ${forbidden}`);
  const allStorage = await page.evaluate(() => Object.values(localStorage).join("\n"));
  assert(!allStorage.includes("library-private-7!z") && !allStorage.includes("mysql://"), "Credentials leaked into ordinary browser settings");
  assert((await calls("execute_mariadb_query")).length === 1, "Bookmark actions executed raw SQL");
  const fixture = await page.evaluate(() => ({ unknown: window.testQueryLibrary.unknown, premature: window.testQueryLibrary.premature }));
  assert(!fixture.unknown.length && !fixture.premature.length && !errors.length, JSON.stringify({ ...fixture, errors }));

  await page.reload({ waitUntil: "domcontentloaded" });
  await navigate("Queries & Files");
  library = await openDetails(/^Saved Queries/);
  await choose(library.locator('[data-slot="select-trigger"]').first(), "Reports / Player lookup");
  await library.getByRole("button", { name: "Load", exact: true }).click();
  assert((await editor.inputValue()).includes("{{id:number}}"), "Saved query did not survive reload");
  assert((await calls("execute_mariadb_query")).length === 0, "App reload replayed a saved query");
  assert((await savedData()).bookmarks.length === 2 && !errors.length, "Bookmarks did not survive reload or a browser error occurred");

  const malformed = '{"version":99,"valuable":"must remain unchanged"}';
  await page.evaluate((raw) => {
    localStorage.setItem("fxserver-installer.database-library.v1", raw);
    window.dispatchEvent(new StorageEvent("storage", { key: "fxserver-installer.database-library.v1", newValue: raw }));
  }, malformed);
  await library.getByText(/remains unchanged in local storage/).waitFor();
  assert(await library.getByRole("button", { name: "Save New", exact: true }).isDisabled(), "Malformed saved data did not disable writes");
  const reset = library.getByRole("region", { name: "Reset saved database library" });
  assert(await reset.getByRole("button", { name: "Reset Library", exact: true }).isDisabled(), "Reset lacked explicit confirmation");
  await reset.getByLabel("Type RESET", { exact: true }).fill("NO");
  assert(await reset.getByRole("button", { name: "Reset Library", exact: true }).isDisabled(), "Incorrect reset confirmation enabled deletion");
  assert(await page.evaluate(() => localStorage.getItem("fxserver-installer.database-library.v1")) === malformed, "Malformed data was silently discarded");
  await reset.getByLabel("Type RESET", { exact: true }).fill("RESET");
  await reset.getByRole("button", { name: "Reset Library", exact: true }).click();
  await page.waitForFunction(() => localStorage.getItem("fxserver-installer.database-library.v1") === null);
  assert(!errors.length, JSON.stringify(errors));
  return "PASS: saved folders/rename/load, typed draft injection checks, confirmation rejection including saved DELETE, bookmarks/filter/sort/page/view and read-only restore, shared cache/session navigation, credential-free storage/reload, malformed-data preservation and explicit reset at wide/narrow viewports. All IPC mocked; no live DB or file operations.";
}
