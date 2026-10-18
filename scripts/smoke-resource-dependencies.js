async (page) => {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.route("https://raw.githubusercontent.com/**", (route) => route.fulfill({ body: "version '2.0.0'", contentType: "text/plain" }));
  await page.route("https://api.github.com/**", (route) => route.fulfill({ json: route.request().url().includes("releases") ? [] : { default_branch: "main", html_url: "https://github.com/example/resource" } }));
  await page.addInitScript(() => {
    localStorage.clear();
    localStorage.setItem("installPath", "C:/fixture/artifacts");
    localStorage.setItem("fxserver.manage.env", JSON.stringify({ TXHOST_DATA_PATH: "C:/fixture/txData" }));
    localStorage.setItem("fxserver.manage.serverProfile", "test");
    const callbacks = new Map();
    const state = window.dependencyFixture = { calls: [], counter: 0, failScan: false };
    const resource = (name, dependencies = [], extra = {}) => ({ name, path: `C:/fixture/resources/${name}`, origin: "data", dependencies, provides: [], groups: [], dynamic: false, ...extra });
    state.inventory = {
      resources: [resource("core"), resource("job", ["core"]), resource("ui", ["job"]), resource("broken", ["missing", "/onesync"]), resource("a", ["b"]), resource("b", ["a"]), resource("chat", [], { origin: "artifact", path: "C:/fixture/artifacts/citizen/system_resources/chat" })],
      resourcesIncomplete: false, configsIncomplete: false, startup: [{ command: "ensure", target: "core" }, { command: "start", target: "job" }], warnings: [],
    };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
      transformCallback: (callback) => { callbacks.set(++state.counter, callback); return state.counter; },
      invoke: async (command, args = {}) => {
        state.calls.push({ command, args });
        switch (command) {
          case "get_platform_info": return { os: "windows", arch: "x86_64", artifactPlatform: "windows", clientLogsSupported: true, secureStorageSupported: true };
          case "plugin:window|title": return "FXServer Installer";
          case "plugin:app|version": return "0.5.1";
          case "plugin:event|listen": return args.handler;
          case "plugin:event|unlisten":
          case "initialize_health_workspace":
          case "append_app_log":
          case "save_fxserver_rcon_password":
          case "send_fxserver_rcon_command":
          case "discard_resource_preview": return;
          case "load_database_login": return null;
          case "read_app_logs": return { path: "fixture.log", entries: [] };
          case "get_fxserver_rcon_password": return "fixture-password";
          case "configure_live_bridge": return { workspaceId: "default", enabled: false, connected: false, snapshot: null };
          case "get_windows_artifact_metadata": return { recommendedArtifact: "10000", brokenArtifacts: [] };
          case "get_installed_windows_artifact_info": return { installed: true, version: "10000", destination: "C:/fixture/artifacts", hasFxserverExecutable: true };
          case "scan_fxserver_resources": return { txDataPath: "C:/fixture/txData", profile: "test", dataPath: "C:/fixture", resourceRoot: "C:/fixture/resources", resources: [{ name: "core", path: "C:/fixture/resources/core", manifestPath: "C:/fixture/resources/core/fxmanifest.lua", manifestName: "fxmanifest.lua", version: "1.0.0", repository: "https://github.com/example/resource" }] };
          case "scan_resource_dependencies": if (state.failScan) throw new Error("Fixture unreadable profile"); return state.inventory;
          case "preview_resource_update": return { id: "preview", resourceName: "core", repository: "https://github.com/example/resource", branch: "main", archiveBytes: 64, archiveSha256: "a".repeat(64), createdAt: Math.floor(Date.now() / 1000), changes: [{ path: "server.lua", kind: "modified", oldSize: 1, newSize: 2, preserve: false, canPreserve: true }] };
          case "apply_resource_update": return { id: "snapshot", resourceName: "core", createdAt: Date.now() / 1000, fileCount: 1, sizeBytes: 2, reason: "update" };
          case "fetch_latest_app_release": return { version: "0.5.1", tagName: "v0.5.1", htmlUrl: "https://github.com/zoxile/fxserver-installer/releases/tag/v0.5.1", installerUrl: "https://github.com/zoxile/fxserver-installer/releases/download/v0.5.1/FXServer.Installer_0.5.1_windows_x64-setup.exe" };
          default: throw new Error(`Unmocked command: ${command}`);
        }
      },
    };
  });
  await page.reload();
  const nav = page.getByRole("navigation", { name: "Workspace navigation" });
  const fxserver = nav.getByTitle("FXServer", { exact: true });
  if (await fxserver.getAttribute("aria-expanded") !== "true") await fxserver.click();
  await nav.getByTitle("Resource Manager", { exact: true }).click();
  await page.getByRole("button", { name: "Inspect dependencies for core", exact: true }).waitFor();
  await page.getByRole("button", { name: "Dependencies", exact: true }).click();
  const inspector = page.getByRole("dialog", { name: "Resource Dependency Inspector", exact: true });
  await inspector.getByLabel("Inspect resource", { exact: true }).click();
  await page.getByRole("option", { name: "core / data", exact: true }).click();
  await inspector.getByRole("heading", { name: "Direct Dependents (1)", exact: true }).waitFor();
  await inspector.getByRole("heading", { name: "Transitive Dependents (Indirect) (1)", exact: true }).waitFor();
  await inspector.getByText("All Missing Dependencies, Ambiguities and Cycles", { exact: true }).click();
  await inspector.getByText("broken: missing / missing", { exact: true }).waitFor();
  await inspector.getByText("Cycle group: a, b", { exact: true }).waitFor();
  for (const width of [1440, 760, 390]) {
    await page.setViewportSize({ width, height: 850 });
    await page.screenshot({ path: `output/playwright/resource-dependencies-${width}.png`, fullPage: true });
    if (await inspector.evaluate((element) => element.scrollWidth > element.clientWidth + 1)) throw new Error(`Inspector overflow at ${width}`);
    const box = await inspector.boundingBox();
    if (box.x < 0 || box.x + box.width > width + 1 || box.y < 0 || box.y + box.height > 851) throw new Error(`Inspector offscreen at ${width}`);
  }
  await inspector.getByRole("button", { name: "Close", exact: true }).click();
  await page.setViewportSize({ width: 1440, height: 1000 });
  const sent = () => page.evaluate(() => window.dependencyFixture.calls.filter((call) => call.command === "send_fxserver_rcon_command").length);
  await page.getByRole("button", { name: "Stop", exact: true }).click();
  let review = page.getByRole("dialog", { name: "stop: Dependency Review", exact: true });
  await review.getByRole("heading", { name: "Direct Dependents (1)", exact: true }).waitFor();
  if (await sent() !== 0 || await review.getByRole("button", { name: "Confirm stop", exact: true }).isEnabled()) throw new Error("Stop bypassed acknowledgment");
  await review.getByRole("button", { name: "Cancel", exact: true }).click();
  if (await sent() !== 0) throw new Error("Cancel sent an RCON command");
  await page.getByRole("button", { name: "Stop", exact: true }).click();
  review = page.getByRole("dialog", { name: "stop: Dependency Review", exact: true });
  await review.getByRole("checkbox", { name: "Acknowledge dependency impact" }).click();
  await review.getByRole("button", { name: "Confirm stop", exact: true }).click();
  await page.waitForFunction(() => window.dependencyFixture.calls.some((call) => call.command === "send_fxserver_rcon_command" && call.args.request.command === "stop core"));
  await page.locator("article").filter({ has: page.getByRole("button", { name: "Inspect dependencies for core", exact: true }) }).click({ button: "right" });
  await page.getByRole("menu").getByRole("button", { name: "Restart", exact: true }).click();
  review = page.getByRole("dialog", { name: "restart: Dependency Review", exact: true });
  await review.getByRole("checkbox", { name: "Acknowledge dependency impact" }).click();
  await review.getByRole("button", { name: "Rescan", exact: true }).click();
  await review.getByRole("heading", { name: "Direct Dependents (1)", exact: true }).waitFor();
  if (await review.getByRole("button", { name: "Confirm restart", exact: true }).isEnabled()) throw new Error("Rescan retained stale acknowledgment");
  await review.getByRole("checkbox", { name: "Acknowledge dependency impact" }).click();
  await review.getByRole("button", { name: "Confirm restart", exact: true }).click();
  await page.waitForFunction(() => window.dependencyFixture.calls.some((call) => call.command === "send_fxserver_rcon_command" && call.args.request.command === "restart core"));
  await page.evaluate(() => { window.dependencyFixture.failScan = true; });
  await page.getByRole("button", { name: "Stop", exact: true }).click();
  review = page.getByRole("dialog", { name: "stop: Dependency Review", exact: true });
  await review.getByText(/Fixture unreadable profile/).waitFor();
  if (await review.getByRole("button", { name: "Confirm stop", exact: true }).isEnabled()) throw new Error("Failed scan permits action");
  await review.getByRole("button", { name: "Cancel", exact: true }).click();
  await page.evaluate(() => { window.dependencyFixture.failScan = false; });
  await page.getByRole("button", { name: "Check", exact: true }).click();
  await page.getByRole("button", { name: "Update", exact: true }).click();
  const update = page.getByRole("dialog", { name: "Preview Update: core", exact: true });
  await update.getByRole("button", { name: "Review Update", exact: true }).click();
  review = page.getByRole("dialog", { name: "Update: Dependency Review", exact: true });
  await review.getByRole("heading", { name: "Direct Dependents (1)", exact: true }).waitFor();
  if (await page.evaluate(() => window.dependencyFixture.calls.some((call) => call.command === "apply_resource_update"))) throw new Error("Update applied before dependency review");
  await review.getByRole("checkbox", { name: "Acknowledge dependency impact" }).click();
  await review.getByRole("button", { name: "Confirm Update", exact: true }).click();
  await page.waitForFunction(() => window.dependencyFixture.calls.some((call) => call.command === "apply_resource_update"));
  await page.getByRole("dialog").waitFor({ state: "hidden" });
  await page.evaluate(() => {
    const resource = (name, dependencies = []) => ({ name, path: `C:/fixture/resources/${name}`, origin: "data", dependencies, provides: [], groups: [], dynamic: false });
    const resources = [resource("core"),
      ...Array.from({ length: 2999 }, (_, index) => resource(`leaf-${String(index).padStart(4, "0")}`, ["core"])),
      ...Array.from({ length: 1000 }, (_, index) => resource(`cycle-${index}`, [`cycle-${index ^ 1}`])),
      ...Array.from({ length: 1000 }, (_, index) => resource(`broken-${index}`, [`missing-${index}`]))];
    window.dependencyFixture.inventory = { resources, resourcesIncomplete: false, configsIncomplete: false,
      startup: resources.map((entry) => ({ command: "ensure", target: entry.name })),
      warnings: Array.from({ length: 2000 }, (_, index) => ({ code: `fixture-${index}`, detail: `Fixture warning ${index}` })) };
  });
  await page.getByRole("button", { name: "Dependencies", exact: true }).click();
  await inspector.getByText("Showing 1-50 of 5000", { exact: true }).waitFor();
  await inspector.getByLabel("Inspect resource", { exact: true }).click();
  if (await page.getByRole("option").count() !== 50) throw new Error("Resource selector is not bounded to 50 options");
  if ((await page.getByRole("option").allTextContents()).some((label) => label.includes("C:/"))) throw new Error("Full paths leaked into selector labels");
  await page.keyboard.press("Escape");
  await inspector.getByRole("button", { name: "Next resources page", exact: true }).click();
  await inspector.getByText("Showing 51-100 of 5000", { exact: true }).waitFor();
  await inspector.getByRole("textbox", { name: "Search dependency resources", exact: true }).fill("leaf-2998");
  await inspector.getByLabel("Inspect resource", { exact: true }).click();
  await page.getByRole("option", { name: "leaf-2998 / data", exact: true }).click();
  await inspector.getByRole("textbox", { name: "Search dependency resources", exact: true }).fill("core");
  await inspector.getByLabel("Inspect resource", { exact: true }).click();
  await page.getByRole("option", { name: "core / data", exact: true }).click();
  const dependents = inspector.locator('[data-dependency-list="Direct Dependents"]');
  await dependents.getByText("Showing 1-50 of 2999", { exact: true }).waitFor();
  await dependents.getByRole("button", { name: "Next Direct Dependents page", exact: true }).click();
  await dependents.getByText("Showing 51-100 of 2999", { exact: true }).waitFor();
  if (await inspector.locator("details:not([open]) li").count()) throw new Error("Closed details render hidden list entries");
  for (const summary of ["All Missing Dependencies, Ambiguities and Cycles", "All Resources / Suggested Startup Order", "Configured Startup Sequence (5000)", "Scan Warnings (2000)", "Suggested Startup Order / Selected Resource and Dependents"]) {
    await inspector.locator("summary").filter({ hasText: summary }).click();
  }
  const oversized = await inspector.locator("[data-dependency-list]").evaluateAll((lists) => lists.filter((list) => list.querySelectorAll("li").length > 50).map((list) => list.dataset.dependencyList));
  if (oversized.length) throw new Error(`Unbounded lists: ${oversized.join(", ")}`);
  if (await inspector.locator("li").count() > 500) throw new Error("Large dependency inventory created an excessive DOM");
  const configured = inspector.locator('[data-dependency-list="Configured Startup Sequence"]');
  await configured.getByRole("button", { name: "Next Configured Startup Sequence page", exact: true }).click();
  await configured.getByText("Showing 51-100 of 5000", { exact: true }).waitFor();
  if (await configured.locator("ol").getAttribute("start") !== "51") throw new Error("Startup pagination lost sequence numbering");
  await inspector.locator("summary").filter({ hasText: "Scan Warnings (2000)" }).click();
  await inspector.locator('[data-dependency-list="Scan Warnings"]').waitFor({ state: "detached" });
  await page.setViewportSize({ width: 390, height: 850 });
  await page.screenshot({ path: "output/playwright/resource-dependencies-5000-narrow.png", fullPage: true });
  if (await inspector.evaluate((element) => element.scrollWidth > element.clientWidth + 1)) throw new Error("Large inventory overflows narrow inspector");
  if (errors.length) throw new Error(errors.join("\n"));
  return "PASS: graph/impact reviews, fail-closed scan, stop/context restart/update, 1440/760/390 layouts, searchable 50-option Select, 5000-resource paginated lists and lazy closed details.";
}
