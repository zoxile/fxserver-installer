async (page) => {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.setViewportSize({ width: 1440, height: 1050 });
  await page.route("https://raw.githubusercontent.com/**", (route) => route.fulfill({ json: { version: "0.3.2" } }));
  await page.addInitScript(() => {
    localStorage.clear();
    localStorage.setItem("installPath", "C:/fixture/artifacts");
    localStorage.setItem("fxserver.manage.env", JSON.stringify({ TXHOST_DATA_PATH: "C:/fixture/txData" }));
    localStorage.setItem("fxserver.manage.serverProfile", "test");
    const callbacks = new Map();
    const events = new Map();
    const state = window.bridgeFixture = { installed: false, counter: 0, calls: [], unknown: [], pending: null, refuse: false };
    const installation = () => ({ workspaceId: "default", installed: state.installed, managed: state.installed, resourcePath: "C:/fixture/resources/fxserver_installer_bridge", version: state.installed ? "1.1.0" : null, cfgEnabled: state.installed, keyAvailable: state.installed, warning: null });
    state.snapshot = { protocol: 2, version: "1.1.0", instanceId: "fixture-instance", timestamp: Date.now(), uptimeSeconds: 65, schedulerDelayMs: 2, hostname: "Fixture server", gameBuild: "3258", onesync: "on", maxPlayers: 48, playerCount: 2, resourceCount: 120,
      resources: Array.from({ length: 120 }, (_, index) => ({ name: `resource_${index}`, state: index % 2 ? "stopped" : "started", version: "1.0.0" })),
      players: [{ id: "1", name: "Fixture player", ping: 32 }, { id: "2", name: "Another player", ping: 65 }],
      events: [{ id: 1, timestamp: Date.now(), kind: "resource-started", resource: "resource_0" }] };
    state.status = (enabled) => ({ workspaceId: "default", enabled, connected: enabled, receivedAt: Date.now(), error: null, snapshot: enabled ? state.snapshot : null });
    state.emit = (status) => callbacks.get(events.get("live-bridge-update"))({ payload: status });
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
          case "plugin:event|listen": events.set(args.event, args.handler); return args.handler;
          case "plugin:event|unlisten":
          case "initialize_health_workspace":
          case "append_app_log": return;
          case "read_app_logs": return { path: "fixture.log", entries: [] };
          case "get_windows_artifact_metadata": return { recommendedArtifact: "10000", brokenArtifacts: [] };
          case "get_installed_windows_artifact_info": return { installed: true, version: "10000", destination: "C:/fixture/artifacts", hasFxserverExecutable: true };
          case "get_live_bridge_installation": return installation();
          case "configure_live_bridge": return state.status(args.enabled);
          case "preview_live_bridge_change": return { id: args.remove ? "remove" : "install", remove: args.remove, resourcePath: installation().resourcePath, files: ["server.js", "fxmanifest.lua", "bridge-token.txt (secret)"], configLines: ["ensure fxserver_installer_bridge"], expiresInSeconds: 600 };
          case "apply_live_bridge_change":
            if (state.refuse) throw new Error("Stop FXServer before changing bridge files.");
            state.installed = args.previewId === "install"; return installation();
          case "send_live_bridge_action": return new Promise((resolve) => { state.pending = resolve; });
          default: state.unknown.push(command); throw new Error(`Unmocked command: ${command}`);
        }
      },
    };
  });
  await page.reload();
  const nav = page.getByRole("navigation", { name: "Workspace navigation" });
  const open = async () => {
    const parent = nav.getByTitle("FXServer", { exact: true });
    if (await parent.getAttribute("aria-expanded") !== "true") await parent.click();
    await nav.getByTitle("Live Bridge", { exact: true }).click();
  };
  await open();
  await page.getByRole("button", { name: "Install bridge", exact: true }).click();
  if (await page.evaluate(() => window.bridgeFixture.installed)) throw new Error("Installed before review");
  await page.getByRole("button", { name: "Confirm installation", exact: true }).click();
  await page.getByRole("heading", { name: "Fixture server" }).waitFor();
  if (await page.getByTitle(/^start resource_/).count() !== 30) throw new Error("Live resources are not initially bounded");
  await page.getByLabel("Search live resources").fill("resource_119");
  await page.getByTitle("start resource_119", { exact: true }).click();
  await page.waitForFunction(() => Boolean(window.bridgeFixture.pending));
  await nav.getByTitle("Home", { exact: true }).click();
  await page.getByText("Project", { exact: true }).waitFor();
  await page.evaluate(() => { window.bridgeFixture.pending(); window.bridgeFixture.pending = null; });
  await open();
  await page.getByRole("heading", { name: "Fixture server" }).waitFor();
  await page.screenshot({ path: "output/playwright/live-bridge.png", fullPage: true });
  await page.setViewportSize({ width: 760, height: 900 });
  await page.screenshot({ path: "output/playwright/live-bridge-narrow.png", fullPage: true });
  await page.evaluate(() => window.bridgeFixture.emit({ ...window.bridgeFixture.status(false), enabled: true, error: "Disconnected fixture" }));
  await page.getByText("Disconnected fixture", { exact: true }).waitFor();
  if (await page.getByRole("heading", { name: "Fixture server" }).count()) throw new Error("Stale runtime data remained visible");
  await page.getByRole("button", { name: "Remove bridge", exact: true }).click();
  await page.evaluate(() => { window.bridgeFixture.refuse = true; });
  await page.getByRole("button", { name: "Confirm removal", exact: true }).click();
  await page.getByText(/Stop FXServer before changing bridge files\.$/).waitFor();
  await page.evaluate(() => { window.bridgeFixture.refuse = false; });
  await page.getByRole("button", { name: "Confirm removal", exact: true }).click();
  await page.getByRole("button", { name: "Install bridge", exact: true }).waitFor();
  if (await page.evaluate(() => window.bridgeFixture.installed)) throw new Error("Bridge removal did not complete");
  const unknown = await page.evaluate(() => window.bridgeFixture.unknown);
  if (errors.length || unknown.length) throw new Error([...errors, ...unknown].join("\n"));
  console.log("PASS: bridge reviewed install/remove, stopped-server refusal, live status, bounded search, action during navigation, disconnect clears stale state, desktop/narrow layouts.");
}
