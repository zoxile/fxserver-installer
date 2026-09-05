async (page) => {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.route("https://raw.githubusercontent.com/**", (route) => route.request().url().includes("fixture") ? route.fulfill({ body: "version '2.0.0'\nrepository 'https://github.com/example/fixture'" }) : route.fulfill({ json: { version: "0.3.2" } }));
  await page.route("https://api.github.com/repos/**", (route) => route.fulfill({ json: route.request().url().endsWith("/releases/latest") ? { name: "Fixture release", tag_name: "v2.0.0", body: "Fixture release notes: configuration migration required.", html_url: "https://github.com/example/fixture/releases/tag/v2.0.0" } : { default_branch: "main", html_url: "https://github.com/example/fixture" } }));
  await page.addInitScript(() => {
    localStorage.clear();
    localStorage.setItem("installPath", "C:/fixture/artifacts");
    localStorage.setItem("fxserver.manage.env", JSON.stringify({ TXHOST_DATA_PATH: "C:/fixture/txData" }));
    localStorage.setItem("fxserver.manage.serverProfile", "test");
    const state = window.artifactPlanFixture = { calls: [], pending: null, installed: "10000", previewCount: 0, previews: {}, counter: 0 };
    const callbacks = new Map();
    const metadata = { recommendedArtifact: "10000", windowsDownloadLink: "https://runtime.fivem.net/artifacts/fivem/build_server_windows/master/10000-0123456789abcdef0123456789abcdef01234567/server.zip", brokenArtifacts: [{ artifact: "9999", reason: "Fixture crash when players join" }] };
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
          case "plugin:event|listen": return ++state.counter;
          case "plugin:event|unlisten":
          case "append_app_log":
          case "initialize_health_workspace": return;
          case "read_app_logs": return { path: "fixture.log", entries: [] };
          case "configure_live_bridge": return { workspaceId: args.target.workspaceId, enabled: false, connected: false, snapshot: null };
          case "get_fxserver_rcon_password": return "";
          case "get_windows_artifact_metadata": return metadata;
          case "get_windows_artifact_catalog": return { fetchedAt: Date.now() / 1000, metadataFetchedAt: Date.now() / 1000, stale: false, warning: null,
            builds: Array.from({ length: 60 }, (_, index) => { const version = String(10020 - index); return { version, downloadUrl: metadata.windowsDownloadLink.replaceAll("10000", version), health: version === "9999" ? "known-issue" : version === "10000" ? "healthy" : "unknown", issues: version === "9999" ? metadata.brokenArtifacts : [], recommended: version === "10000" }; }) };
          case "get_installed_windows_artifact_info": return { installed: true, version: state.installed, destination: "C:/fixture/artifacts", hasFxserverExecutable: true, detectionSource: "marker" };
          case "install_windows_artifact": state.installed = args.request.version; return { version: state.installed, destination: args.request.destination, markerPath: "fixture-marker" };
          case "get_fxserver_status": return { running: false, pid: null, resources: null };
          case "get_mariadb_status": return { installed: false, running: false };
          case "get_mariadb_package_info": return { latestVersion: "12.3.3", updateAvailable: false };
          case "run_fxserver_preflight": return { checkedAt: Date.now(), blocking: false, errorCount: 0, warningCount: 0, resourceCount: 3, configCount: 1, checks: [] };
          case "scan_fxserver_resources": return { txDataPath: "C:/fixture/txData", profile: "test", dataPath: "C:/fixture", resourceRoot: "C:/fixture/resources", resources: ["alpha", "beta", "gamma"].map((name) => ({ name, path: `C:/fixture/resources/${name}`, manifestPath: `C:/fixture/resources/${name}/fxmanifest.lua`, manifestName: "fxmanifest.lua", version: "1.0.0", repository: "https://github.com/example/fixture" })) };
          case "preview_resource_update": {
            const id = `preview-${++state.previewCount}`;
            const name = args.request.target.resourcePath.split("/").at(-1);
            const result = { id, resourceName: name, repository: "https://github.com/example/fixture", branch: "main", archiveSha256: "a".repeat(64), archiveBytes: 1234, createdAt: Math.floor(Date.now() / 1000), changes: [{ path: "config.lua", kind: "modified", oldSize: 20, newSize: 30, preserve: true, canPreserve: true }, { path: "server.lua", kind: "modified", oldSize: 100, newSize: 200, preserve: false, canPreserve: true }] };
            state.previews[id] = result; return result;
          }
          case "discard_resource_preview": delete state.previews[args.previewId]; return;
          case "apply_resource_update": return new Promise((resolve, reject) => { state.pending = { resolve, reject }; });
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
  const browser = page.getByRole("region", { name: "Official Windows artifact browser" });
  await browser.getByText("60 builds", { exact: false }).waitFor();
  await browser.getByRole("button", { name: "Next artifact page" }).click();
  await browser.getByRole("button", { name: "Install build 9971", exact: true }).waitFor();
  await browser.getByLabel("Search artifact builds").fill("9999");
  await browser.getByRole("button", { name: "Install build 9999", exact: true }).click();
  let dialog = page.getByRole("dialog");
  await dialog.getByText("Known issues reported by JG Scripts", { exact: true }).waitFor();
  if (!await dialog.getByRole("button", { name: "Confirm Install" }).isDisabled()) throw new Error("Known-issue install was not confirmation gated");
  await page.screenshot({ path: "output/playwright/artifact-warning-desktop.png" });
  await dialog.getByRole("checkbox").check();
  await dialog.getByRole("button", { name: "Confirm Install" }).click();
  await page.getByText("Installed artifact 9999.", { exact: true }).waitFor();
  if (!await page.evaluate(() => window.artifactPlanFixture.calls.find((entry) => entry.command === "install_windows_artifact")?.args.request.acknowledgeRisk)) throw new Error("Risk acknowledgement missing from request");
  await browser.getByLabel("Search artifact builds").fill("");
  await browser.getByLabel("Artifact health filter").click();
  await page.getByRole("option", { name: "Unknown health", exact: true }).click();
  if (await browser.getByText("Healthy (JG)", { exact: true }).count()) throw new Error("Unknown filter contains healthy builds");
  await browser.getByRole("button", { name: "Refresh artifact catalog" }).click();

  await navigate("FXServer", "Resource Manager");
  const resource = (name) => page.locator("article").filter({ has: page.getByText(name, { exact: true }) });
  await resource("alpha").waitFor();
  await resource("gamma").getByRole("button", { name: "Pin gamma", exact: true }).click();
  await resource("gamma").getByText("Pinned: 1.0.0", { exact: true }).waitFor();
  await page.getByRole("button", { name: "Check Updates", exact: true }).click();
  for (const name of ["alpha", "beta"]) {
    await resource(name).getByRole("button", { name: "Update", exact: true }).click();
    dialog = page.getByRole("dialog");
    await dialog.getByRole("button", { name: "Queue Reviewed Update" }).waitFor();
    if (!await dialog.getByRole("checkbox", { name: "Preserve config.lua" }).isDisabled()) throw new Error("Default-protected config can be stripped");
    if (name === "alpha") {
      await dialog.getByText("Release Notes", { exact: true }).click();
      await dialog.getByText("Fixture release notes: configuration migration required.", { exact: true }).waitFor();
    }
    await dialog.getByRole("button", { name: "Queue Reviewed Update" }).click();
  }
  await nav.getByTitle("Home", { exact: true }).click();
  await navigate("FXServer", "Resource Manager");
  const queue = page.getByRole("region", { name: "Reviewed resource update queue" });
  await queue.getByText("2 ready / idle", { exact: true }).waitFor();
  if (await page.evaluate(() => window.artifactPlanFixture.calls.filter((entry) => entry.command === "discard_resource_preview").length)) throw new Error("Navigation discarded queued previews");
  await queue.getByRole("button", { name: "Apply Reviewed", exact: true }).click();
  await page.waitForFunction(() => !!window.artifactPlanFixture.pending);
  await nav.getByTitle("Home", { exact: true }).click();
  await navigate("FXServer", "Resource Manager");
  await queue.getByText("applying: Applying reviewed archive", { exact: true }).waitFor();
  await page.evaluate(() => { window.artifactPlanFixture.pending.reject(new Error("Fixture failure; no files changed")); window.artifactPlanFixture.pending = null; });
  await queue.getByText("1 ready / paused", { exact: true }).waitFor();
  if (await page.evaluate(() => window.artifactPlanFixture.calls.filter((entry) => entry.command === "apply_resource_update").length) !== 1) throw new Error("Queue continued automatically after failure");
  await page.screenshot({ path: "output/playwright/resource-queue-paused-desktop.png" });
  await queue.getByRole("button", { name: "Continue Remaining" }).click();
  await page.waitForFunction(() => !!window.artifactPlanFixture.pending);
  await page.evaluate(() => { window.artifactPlanFixture.pending.resolve({ id: "fixture-snapshot", resourceName: "beta" }); window.artifactPlanFixture.pending = null; });
  await queue.getByText("0 ready / completed", { exact: true }).waitFor();
  await page.setViewportSize({ width: 640, height: 900 });
  await page.screenshot({ path: "output/playwright/resource-queue-mobile.png" });
  const overflow = await queue.evaluate((element) => element.scrollWidth > element.clientWidth + 1);
  if (overflow) throw new Error("Queue overflows narrow viewport");
  if (errors.length) throw new Error(errors.join("\n"));
  return "Artifact and resource planning UI fixtures passed: badges/filter/pagination, explicit risky install, locked config protection, release notes, persisted pin, navigation retention, paused failure, explicit remaining apply.";
}
