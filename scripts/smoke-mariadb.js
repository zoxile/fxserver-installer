async (page) => {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.route("**/jsonv2", (route) => route.fulfill({ json: { recommendedArtifact: "10000", windowsDownloadLink: "https://example.invalid/artifact.zip", brokenArtifacts: [] } }));
  await page.setViewportSize({ width: 1440, height: 1050 });
  await page.addInitScript(() => {
    localStorage.clear();
    const state = window.testMariaDB = { calls: [], pending: {}, installed: false, version: null, running: false, unknown: [] };
    let callback = 0;
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
      transformCallback: () => ++callback,
      invoke: async (command, args = {}) => {
        state.calls.push({ command, args: JSON.parse(JSON.stringify(args)) });
        const status = () => ({ installed: state.installed, running: state.running, version: state.version, serviceName: state.installed ? "MariaDB" : null, serviceDisplayName: "MariaDB", installPath: null });
        switch (command) {
          case "fetch_latest_app_release": return { version: "0.3.2", tagName: "v0.3.2", htmlUrl: "https://github.com/zoxile/fxserver-installer/releases/tag/v0.3.2" };
          case "plugin:window|title": return "FXServer Installer";
          case "plugin:app|version": return "0.4.1";
          case "plugin:event|listen": return args.handler;
          case "plugin:event|unlisten":
          case "append_app_log":
          case "initialize_health_workspace":
          case "save_database_login":
          case "clear_database_login": return;
          case "load_database_login": return { host: "localhost", port: 3306, username: "saved-fixture", password: "fixture-only", database: "" };
          case "configure_live_bridge": return { workspaceId: "default", enabled: false, connected: false, snapshot: null };
          case "read_app_logs": return { path: "mock.log", entries: [] };
          case "get_windows_artifact_metadata": return { recommendedArtifact: "10000", windowsDownloadLink: "https://example.invalid/artifact.zip", brokenArtifacts: [] };
          case "get_installed_windows_artifact_info": return { installed: false };
          case "get_mariadb_status": return status();
          case "get_mariadb_package_info": return { latestVersion: state.version || "11.4.13", installedPackageVersion: state.version, updateAvailable: false, error: null };
          case "list_mariadb_series": return [
            { series: "12.3", support: "Long Term Support", eol: "2029-06-12" },
            { series: "11.4", support: "Long Term Support", eol: "2029-05-29" },
            { series: "10.11", support: "Long Term Support", eol: "2028-02-16" },
          ];
          case "list_mariadb_releases": return (args.series === "12.3" ? ["12.3.3", "12.3.2"] : args.series === "10.11" ? ["10.11.14"] : ["11.4.13", "11.4.12"]).map((version) => ({ version, date: "2026-08-22" }));
          case "list_mariadb_users": return [];
          case "list_mariadb_databases": return ["mysql", "game"];
          case "validate_mariadb_credentials":
            if (sessionStorage.getItem("reject-saved-login")) throw new Error("Saved fixture login rejected");
            return new Promise((resolve) => { state.pending.validate = resolve; });
          case "install_mariadb":
            return new Promise((resolve) => { state.pending.install = () => { state.installed = true; state.version = args.options.version; state.running = true; resolve("Fixture installation completed."); }; });
          case "restart_mariadb_service":
            return new Promise((resolve, reject) => { state.pending.restart = { resolve: () => resolve(status()), reject }; });
          default: state.unknown.push(command); throw new Error(`Unmocked MariaDB smoke command: ${command}`);
        }
      },
    };
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  const openDatabase = async () => {
    const nav = page.getByRole("navigation", { name: "Workspace navigation" });
    const parent = nav.getByTitle("MariaDB", { exact: true });
    if (await parent.getAttribute("aria-expanded") !== "true") await parent.click();
    await nav.getByTitle("Manage MariaDB", { exact: true }).click();
    await page.getByRole("heading", { name: "Manage MariaDB", exact: true }).waitFor();
  };
  await openDatabase();
  await page.waitForFunction(() => !!window.testMariaDB.pending.validate);
  if (await page.getByText("Credentials validated.", { exact: true }).count()) throw new Error("Saved credentials were trusted without validation");
  if (await page.getByTitle("Admin username used to connect to MariaDB.", { exact: true }).inputValue() !== "saved-fixture") throw new Error("Saved login was not restored");
  await page.evaluate(() => window.testMariaDB.pending.validate());
  await page.getByText("Credentials validated.", { exact: true }).waitFor();
  const validations = () => page.evaluate(() => window.testMariaDB.calls.filter((call) => call.command === "validate_mariadb_credentials").length);
  if (await validations() !== 1) throw new Error("Saved login validation repeated on one mount");

  const username = page.getByTitle("Admin username used to connect to MariaDB.", { exact: true });
  await username.fill("edited-fixture");
  await page.getByText("Apply credentials to validate the MariaDB connection.", { exact: true }).waitFor();
  if (await page.getByText("Credentials validated.", { exact: true }).count()) throw new Error("Edited login retained stale validation");
  if (await validations() !== 1) throw new Error("Editing credentials triggered implicit validation");
  await username.fill("saved-fixture");
  await page.getByText("Credentials validated.", { exact: true }).waitFor();

  const install = page.getByRole("button", { name: "Install", exact: true });
  await page.getByLabel("Supported LTS Series", { exact: true }).click();
  await page.getByRole("option", { name: "12.3 (until 2029-06-12)", exact: true }).click();
  await page.getByLabel("Release", { exact: true }).click();
  await page.getByRole("option", { name: "12.3.2 (2026-08-22)", exact: true }).click();
  await page.getByText("12.3.2", { exact: true }).waitFor();
  await page.getByPlaceholder("Required root password").fill("fixture-only");
  await page.setViewportSize({ width: 480, height: 900 });
  await page.getByLabel("Release", { exact: true }).scrollIntoViewIfNeeded();
  for (const label of ["Preset", "Supported LTS Series", "Release"]) {
    const bounds = await page.getByLabel(label, { exact: true }).boundingBox();
    if (!bounds || bounds.x < 0 || bounds.x + bounds.width > 480) throw new Error(`${label} clips on a narrow viewport`);
  }
  await page.screenshot({ path: "output/playwright/mariadb-install-mobile.png", fullPage: true });
  await page.setViewportSize({ width: 1440, height: 1050 });
  await page.screenshot({ path: "output/playwright/mariadb-install-selection.png", fullPage: true });
  await install.click();
  await page.waitForFunction(() => !!window.testMariaDB.pending.install);
  const selected = await page.evaluate(() => window.testMariaDB.calls.find((call) => call.command === "install_mariadb").args.options.version);
  if (selected !== "12.3.2") throw new Error(`UI silently substituted ${selected}`);
  if (!await install.isDisabled()) throw new Error("Duplicate install remained enabled");
  await page.evaluate(() => window.testMariaDB.pending.install());
  await page.getByText("Install Configuration", { exact: true }).waitFor({ state: "hidden" });

  const restart = page.getByRole("button", { name: "Restart", exact: true });
  await restart.click();
  await page.waitForFunction(() => !!window.testMariaDB.pending.restart);
  if (!await restart.isDisabled()) throw new Error("Duplicate restart remained enabled");
  await page.evaluate(() => { window.testMariaDB.pending.restart.reject("Fixture service stop timed out; start was not attempted"); delete window.testMariaDB.pending.restart; });
  await page.getByText("Fixture service stop timed out; start was not attempted", { exact: true }).waitFor();
  if (await page.evaluate(() => window.testMariaDB.calls.some((call) => call.command === "start_mariadb_service"))) throw new Error("Restart failure fell back to start");
  await restart.click();
  await page.waitForFunction(() => !!window.testMariaDB.pending.restart);
  await page.evaluate(() => window.testMariaDB.pending.restart.resolve());
  await page.getByText("MariaDB service restarted.", { exact: true }).waitFor();

  await page.evaluate(() => sessionStorage.setItem("reject-saved-login", "true"));
  await page.reload({ waitUntil: "domcontentloaded" });
  await openDatabase();
  await page.getByText("Saved fixture login rejected", { exact: true }).first().waitFor();
  if (await page.getByText("Credentials validated.", { exact: true }).count()) throw new Error("Rejected saved login was marked ready");
  if (await validations() !== 1) throw new Error("Rejected saved login entered a retry loop");
  const unknown = await page.evaluate(() => window.testMariaDB.unknown);
  if (unknown.length || errors.length) throw new Error([...unknown, ...errors].join("\n"));
  console.log("PASS: exact install selection, duplicate action guards, restart failure without start fallback, and one-time saved-login validation success/failure; no real database operations.");
}
