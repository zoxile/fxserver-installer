async (page) => {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.route("**/jsonv2", (route) => route.fulfill({ json: { recommendedArtifact: "10000", windowsDownloadLink: "https://fixture.invalid", brokenArtifacts: [] } }));
  await page.addInitScript(() => {
    localStorage.clear();
    localStorage.setItem("installPath", "C:/fixture/artifacts");
    localStorage.setItem("fxserver.manage.env", JSON.stringify({ TXHOST_DATA_PATH: "C:/fixture/artifacts/custom-data" }));
    localStorage.setItem("fxserver.manage.serverProfile", "saved-profile");
    const state = window.enhancedFixture = { calls: [], pending: null, installed: false, counter: 0 };
    const version = "00000000-0000-0000-0000-000000000001";
    const downloadUrl = `https://downloads.cfx-services.net/prod/${version}/cfx-server_win_x64.zip`;
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
      transformCallback: () => ++state.counter,
      invoke: async (command, args = {}) => {
        state.calls.push({ command, args });
        switch (command) {
          case "plugin:window|title": return "FXServer Installer";
          case "plugin:app|version": return "0.4.1";
          case "plugin:event|listen": return ++state.counter;
          case "read_app_logs": return { path: "fixture.log", entries: [] };
          case "get_windows_artifact_metadata": return { recommendedArtifact: "10000", windowsDownloadLink: "https://fixture.invalid", brokenArtifacts: [] };
          case "get_windows_artifact_catalog": return { builds: [], fetchedAt: 0, stale: false, warning: null };
          case "get_installed_windows_artifact_info": return { installed: state.installed, edition: state.installed ? "enhanced" : null, version: state.installed ? version : null, hasFxserverExecutable: state.installed, detectionSource: "marker" };
          case "get_enhanced_artifact_catalog": return { builds: [{ version, downloadUrl }], warning: null, sourceUrl: "https://docs.fivem.net/docs/server-download/?platform=enhanced&os=windows" };
          case "install_enhanced_artifact": return new Promise((resolve) => { state.pending = () => { state.installed = true; resolve({ version, destination: args.request.destination, markerPath: "fixture-marker" }); state.pending = null; }; });
          case "list_txdata_profiles": return { dataPath: args.dataPath, profiles: ["saved-profile", "other"], hasRootLogs: true, exists: true };
          case "read_txdata_log": return { path: `${args.request.dataPath}/saved-profile/logs/fxserver.log`, logName: "fxserver.log", lineCount: 3000, content: Array.from({ length: 3000 }, (_, index) => `[core] line ${index} ${"x".repeat(index === 2999 ? 10000 : 4)}`).join("\n") };
          case "get_fxserver_status": return { running: false };
          case "get_mariadb_status": return { installed: false, running: false };
          case "get_mariadb_package_info": return { latestVersion: "12.3.3", updateAvailable: false };
          case "get_backup_manager": return { schedules: [], snapshots: [], restoreTests: [], busy: false };
          case "configure_live_bridge": return { workspaceId: "default", enabled: false, connected: false, snapshot: null };
          default: return null;
        }
      },
    };
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  const nav = page.getByRole("navigation", { name: "Workspace navigation" });
  const navigate = async (parent, child) => {
    const toggle = nav.getByTitle(parent, { exact: true });
    if (await toggle.getAttribute("aria-expanded") !== "true") await toggle.click();
    await nav.getByTitle(child, { exact: true }).click();
  };
  await navigate("Artifacts", "Install Artifact");
  await page.getByRole("button", { name: "Enhanced", exact: true }).click();
  const installButton = page.getByRole("button", { name: "Install Enhanced Server", exact: true });
  await installButton.waitFor();
  await page.waitForFunction(() => window.enhancedFixture.calls.some((call) => call.command === "get_enhanced_artifact_catalog"));
  await page.waitForTimeout(350);
  await page.screenshot({ path: "output/playwright/enhanced-desktop.png" });
  await installButton.click();
  await page.waitForFunction(() => !!window.enhancedFixture.pending);
  await navigate("Logs", "Server Logs");
  await page.getByText("1500 of 3000 visible entries", { exact: false }).waitFor();
  const calls = () => page.evaluate(() => window.enhancedFixture.calls.filter((call) => call.command === "read_txdata_log").length);
  const initial = await calls();
  await page.waitForTimeout(300);
  if (await calls() !== initial) throw new Error("Server log reactive loop");
  if (await page.locator("article").count() !== 1500) throw new Error("Log render cap missing");
  await page.getByTitle(/^Task Center/).click();
  await page.getByText("Install Enhanced artifact", { exact: true }).waitFor();
  await page.evaluate(() => window.enhancedFixture.pending());
  await navigate("Artifacts", "Install Artifact");
  await page.getByRole("heading", { name: "FiveM for GTAV Enhanced", exact: true }).waitFor();
  await page.setViewportSize({ width: 390, height: 844 });
  await page.waitForTimeout(400);
  await page.screenshot({ path: "output/playwright/enhanced-mobile.png", fullPage: true });
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth + 1);
  if (overflow) throw new Error("Enhanced UI overflows mobile viewport");
  const saved = await page.evaluate(() => ({ env: JSON.parse(localStorage.getItem("fxserver.manage.env")), profile: localStorage.getItem("fxserver.manage.serverProfile") }));
  if (saved.env.TXHOST_DATA_PATH !== "C:/fixture/artifacts/custom-data" || saved.profile !== "saved-profile") throw new Error("Saved txData selection was migrated");
  if (errors.length) throw new Error(errors.join("\n"));
  return "Enhanced desktop/mobile, tracked navigation, custom txData preservation, and bounded logs passed (mocked backend).";
}
