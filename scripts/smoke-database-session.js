async (page) => {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.setViewportSize({ width: 1440, height: 1050 });
  await page.route("**/jsonv2", (route) => route.fulfill({ json: { recommendedArtifact: "10000", windowsDownloadLink: "https://example.invalid/artifact.zip", brokenArtifacts: [] } }));
  await page.addInitScript(() => {
    localStorage.clear();
    const saved = { host: "localhost", port: 3306, username: "session-fixture", password: "fixture-only", database: "game" };
    const state = window.testDatabaseSession = { calls: [], unknown: [], pending: [], hold: true, accepted: [], premature: [] };
    const signature = (credentials) => JSON.stringify([credentials.host, Number(credentials.port), credentials.username, credentials.password]);
    const databaseReads = new Set(["list_mariadb_users", "list_mariadb_databases", "list_mariadb_tables", "get_database_browser_metadata", "get_database_browser_rows"]);
    let callback = 0;
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
      transformCallback: () => ++callback,
      invoke: async (command, args = {}) => {
        state.calls.push({ command, args: JSON.parse(JSON.stringify(args)) });
        if (databaseReads.has(command) && !state.accepted.includes(signature(args.credentials))) {
          state.premature.push(command);
          throw new Error(`Database read before login validation: ${command}`);
        }
        switch (command) {
          case "fetch_latest_app_release": return null;
          case "plugin:window|title": return "FXServer Installer";
          case "plugin:app|version": return "0.5.0";
          case "plugin:path|resolve_directory": return "C:/fixture/backups";
          case "plugin:event|listen": return args.handler;
          case "plugin:event|unlisten":
          case "append_app_log":
          case "initialize_health_workspace":
          case "save_database_login":
          case "clear_database_login": return;
          case "load_database_login": return { ...saved };
          case "validate_mariadb_credentials": {
            const key = signature(args.credentials);
            if (!state.hold) { state.accepted.push(key); return; }
            return new Promise((resolve, reject) => state.pending.push({
              resolve: () => { state.accepted.push(key); resolve(); },
              reject,
            }));
          }
          case "configure_live_bridge": return { workspaceId: "default", enabled: false, connected: false, snapshot: null };
          case "read_app_logs": return { path: "mock.log", entries: [] };
          case "get_windows_artifact_metadata": return { recommendedArtifact: "10000", windowsDownloadLink: "https://example.invalid/artifact.zip", brokenArtifacts: [] };
          case "get_installed_windows_artifact_info": return { installed: false };
          case "list_txdata_profiles": return { dataPath: "", profiles: [], hasRootLogs: false };
          case "get_mariadb_status": return { installed: true, running: true, version: "11.4.13", serviceName: "MariaDB", serviceDisplayName: "MariaDB", installPath: null };
          case "get_mariadb_package_info": return { latestVersion: "11.4.13", installedPackageVersion: "11.4.13", updateAvailable: false, error: null };
          case "list_mariadb_users": return [{ username: "session-fixture", host: "localhost", plugin: "ed25519", locked: "N" }];
          case "list_mariadb_databases": return ["game", "other_game"];
          case "list_mariadb_tables": return ["players"];
          case "get_database_browser_metadata": return { columns: [{ name: "id", columnType: "varchar(64)", nullable: false, defaultValue: null, extra: "", binary: false }], indexes: [], editable: false, editReason: "Read-only fixture." };
          case "get_database_browser_rows": return { rows: [["session-fixture-row"]], hasMore: false, truncatedCells: false, pageSize: 25 };
          case "get_backup_manager": return { schedules: [], snapshots: [], restoreTests: [], busy: false };
          default: state.unknown.push(command); throw new Error(`Unmocked database session command: ${command}`);
        }
      },
    };
  });

  const assert = (condition, message) => { if (!condition) throw new Error(message); };
  const views = ["Manage MariaDB", "Queries & Files", "Backups & Restore", "Database Browser", "Configure Server"];
  const calls = (command) => page.evaluate((command) => window.testDatabaseSession.calls.filter((call) => call.command === command), command);
  const validationCount = async () => (await calls("validate_mariadb_credentials")).length;
  const waitForValidation = (count) => page.waitForFunction((count) => window.testDatabaseSession.calls.filter((call) => call.command === "validate_mariadb_credentials").length >= count, count);
  const adminUsername = () => page.getByTitle("Admin username used to connect to MariaDB.", { exact: true });
  const navigate = async (view) => {
    const nav = page.getByRole("navigation", { name: "Workspace navigation" });
    const group = nav.getByTitle(view === "Configure Server" ? "FXServer" : "MariaDB", { exact: true });
    if (await group.getAttribute("aria-expanded") !== "true") await group.click();
    await nav.getByTitle(view, { exact: true }).click();
    await page.getByRole("heading", { name: view, exact: true }).waitFor();
    // Queries & Files schedules its mount load after 120ms; include that path in no-repeat checks.
    await page.waitForTimeout(200);
  };
  const showConnection = async (view) => {
    if (view !== "Database Browser") return;
    const details = page.locator("details").filter({ has: adminUsername() });
    if (!await details.evaluate((element) => element.open)) await details.locator(":scope > summary").click();
  };
  const ready = async (view) => {
    if (view === "Configure Server") {
      await page.getByTitle("Copy database connection string", { exact: true }).click({ trial: true });
    } else {
      await showConnection(view);
      await page.getByText("Credentials validated.", { exact: true }).waitFor();
      await page.getByRole("button", { name: "Change Credentials", exact: true }).click({ trial: true });
    }
    if (view === "Manage MariaDB") await page.getByRole("button", { name: "Add User", exact: true }).click({ trial: true });
    if (view === "Database Browser") await page.getByRole("cell", { name: "session-fixture-row", exact: true }).waitFor();
  };
  const recheck = async (view) => {
    await showConnection(view);
    await (view === "Configure Server"
      ? page.getByTitle("Validate these database credentials", { exact: true })
      : page.getByRole("button", { name: "Change Credentials", exact: true })).click();
  };
  const release = async () => {
    await page.evaluate(() => {
      const state = window.testDatabaseSession;
      state.hold = false;
      for (const request of state.pending.splice(0)) request.resolve();
    });
  };
  const noFixtureErrors = async () => {
    const state = await page.evaluate(() => ({ unknown: window.testDatabaseSession.unknown, premature: window.testDatabaseSession.premature }));
    assert(!state.unknown.length && !state.premature.length && !errors.length, JSON.stringify({ ...state, errors }));
  };

  await page.reload({ waitUntil: "domcontentloaded" });
  await navigate("Manage MariaDB");
  await waitForValidation(1);
  assert((await calls("load_database_login")).length === 1, "Saved login was restored more than once");
  assert(await adminUsername().inputValue() === "session-fixture", "Saved credentials were not restored");
  assert(!await page.getByText("Credentials validated.", { exact: true }).count(), "Saved login was trusted before validation completed");
  assert(await page.getByRole("button", { name: "Add User", exact: true }).isDisabled(), "User writes enabled before saved-login validation");
  for (const view of [...views.slice(1), "Manage MariaDB"]) {
    await navigate(view);
    assert(await validationCount() === 1, `${view} started a duplicate validation while the saved login was pending`);
    if (view === "Configure Server") {
      assert(await page.getByTitle("Copy database connection string", { exact: true }).isDisabled(), "Configure Server exposed an unvalidated connection string");
    } else {
      assert(!await page.getByText("Credentials validated.", { exact: true }).count(), `${view} marked a pending login as validated`);
    }
  }
  assert(!(await calls("list_mariadb_databases")).length && !(await calls("list_mariadb_users")).length, "Database data loaded before authentication completed");
  await page.screenshot({ path: "output/playwright/database-session-pending.png", fullPage: true });
  await release();
  await ready("Manage MariaDB");
  const storedBeforeTour = (await calls("save_database_login")).length;
  for (const view of [...views.slice(1), ...views]) {
    await navigate(view);
    await ready(view);
    assert(await validationCount() === 1, `${view} repeated a completed session validation`);
  }
  assert((await calls("save_database_login")).length === storedBeforeTour, "Tab navigation rewrote the saved password");
  console.log("PASS: saved login validated once; in-flight navigation deduplicated; all five views reuse the completed session without rewriting credentials.");

  for (const view of views) {
    await navigate(view);
    await ready(view);
    const previous = await validationCount();
    await recheck(view);
    await waitForValidation(previous + 1);
    await ready(view);
    assert(await validationCount() === previous + 1, `${view} explicit recheck did not make exactly one validation request`);
  }

  await navigate("Manage MariaDB");
  await ready("Manage MariaDB");
  const beforeEdits = await validationCount();
  for (const [title, replacement] of [
    ["MariaDB host for admin actions.", "other-host.invalid"],
    ["MariaDB port for admin actions.", "3307"],
    ["Admin username used to connect to MariaDB.", "edited-user"],
    ["Admin password used to connect to MariaDB.", "edited-password"],
  ]) {
    const field = page.getByTitle(title, { exact: true });
    const original = await field.inputValue();
    await field.fill(replacement);
    await page.getByText("Apply credentials to validate the MariaDB connection.", { exact: true }).waitFor();
    assert(await page.getByRole("button", { name: "Add User", exact: true }).isDisabled(), `${title} retained stale write access`);
    assert(await validationCount() === beforeEdits, `${title} triggered implicit validation while typing`);
    await field.fill(original);
    await ready("Manage MariaDB");
  }
  await page.getByTitle("Optional database to use when running queries.", { exact: true }).fill("other_game");
  await ready("Manage MariaDB");
  assert(await validationCount() === beforeEdits, "Changing only the default schema repeated server authentication");
  await adminUsername().fill("edited-user");
  await recheck("Manage MariaDB");
  await waitForValidation(beforeEdits + 1);
  await ready("Manage MariaDB");
  for (const view of [...views.slice(1), "Manage MariaDB"]) {
    await navigate(view);
    await ready(view);
    assert(await validationCount() === beforeEdits + 1, `${view} did not reuse the explicitly validated replacement login`);
    if (view !== "Configure Server") assert(await adminUsername().inputValue() === "edited-user", `${view} restored the stale username`);
  }
  await page.screenshot({ path: "output/playwright/database-session-revalidated.png", fullPage: true });

  const beforeFailure = await validationCount();
  await page.evaluate(() => { window.testDatabaseSession.hold = true; });
  await recheck("Manage MariaDB");
  await waitForValidation(beforeFailure + 1);
  assert(await page.getByRole("button", { name: "Add User", exact: true }).isDisabled(), "Forced recheck retained stale write access");
  await page.evaluate(() => window.testDatabaseSession.pending.shift().reject(new Error("ERROR 1045: Fixture login rejected")));
  await page.getByText("ERROR 1045: Fixture login rejected", { exact: true }).first().waitFor();
  assert(!await page.getByText("Credentials validated.", { exact: true }).count(), "Rejected recheck left the session validated");
  assert(await page.getByRole("button", { name: "Add User", exact: true }).isDisabled(), "Rejected recheck enabled account changes");
  await page.evaluate(() => { window.testDatabaseSession.hold = false; });
  await recheck("Manage MariaDB");
  await waitForValidation(beforeFailure + 2);
  await ready("Manage MariaDB");
  assert((await calls("validate_mariadb_credentials")).every((call) => call.args.credentials.database === null), "Authentication checks were coupled to database selection");
  await noFixtureErrors();

  // Reopening the app must authenticate the restored login again, not persist trust in storage.
  await page.reload({ waitUntil: "domcontentloaded" });
  await navigate("Manage MariaDB");
  await waitForValidation(1);
  assert(await validationCount() === 1 && !await page.getByText("Credentials validated.", { exact: true }).count(), "App reload reused persisted validation");
  await release();
  await ready("Manage MariaDB");
  await noFixtureErrors();
  console.log("PASS: explicit recheck in every view, host/port/user/password edit guards, schema-independent authentication, replacement login sharing, rejected-recheck invalidation/recovery, and one fresh validation on app reload. No real database or file operations.");
}
