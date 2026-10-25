async (page) => {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.addInitScript(() => {
    localStorage.clear();
    localStorage.setItem("installPath", "C:/fixture/artifacts");
    localStorage.setItem("fxserver.manage.env", JSON.stringify({ TXHOST_DATA_PATH: "C:/fixture/txData" }));
    localStorage.setItem("fxserver.manage.serverProfile", "default");
    const state = window.updateFirewallFixture = { calls: [], callbacks: {}, events: {}, sequence: 0, owned: false, fail: false };
    state.update = { phase: "idle", currentVersion: "0.6.0", availableVersion: null, releaseUrl: null, downloadedBytes: 0, totalBytes: null, message: null, supported: true };
    state.emit = (phase, fields = {}) => {
      Object.assign(state.update, { phase }, fields);
      for (const callback of state.events["app-update-progress"] || []) state.callbacks[callback]({ event: "app-update-progress", payload: { ...state.update } });
    };
    const firewall = (request) => ({
      backend: "windows", active: true, requiresElevation: false, canConfigure: !state.owned, canRemove: state.owned, reloadRequired: false,
      checks: ["inbound", "outbound"].flatMap((direction) => ["tcp", "udp"].map((protocol) => ({ direction, protocol, port: request[`${protocol}Port`],
        state: state.owned && direction === "inbound" ? "owned" : direction === "outbound" ? "defaultAllowed" : "missing", ownedRulePresent: state.owned && direction === "inbound", detail: direction === "outbound" ? "Allowed by the existing outbound policy." : "Fixture game-port rule." }))),
      notices: ["Host firewall checks do not verify router forwarding."],
    });
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
      transformCallback(callback) { const id = ++state.sequence; state.callbacks[id] = callback; return id; },
      unregisterCallback(id) { delete state.callbacks[id]; },
      invoke: async (command, args = {}) => {
        state.calls.push({ command, args });
        switch (command) {
          case "plugin:event|listen": (state.events[args.event] ??= []).push(args.handler); return ++state.sequence;
          case "get_platform_info": return { os: "windows", arch: "x86_64", artifactPlatform: "windows", clientLogsSupported: true, secureStorageSupported: true };
          case "plugin:window|title": return "FXServer Installer";
          case "get_app_update_status": return { ...state.update };
          case "check_app_update": state.emit("available", { availableVersion: "0.6.1" }); return { ...state.update };
          case "download_app_update":
            state.emit("downloading", { downloadedBytes: 1048576, totalBytes: 4194304 });
            return new Promise((resolve) => { state.finishDownload = () => { state.emit("ready"); resolve({ ...state.update }); }; });
          case "install_app_update": throw new Error("Test installation blocked: background operation is running.");
          case "get_fivem_firewall_ports": return { tcpPorts: [30121], udpPorts: [30122], warnings: [] };
          case "inspect_fivem_firewall": return firewall(args.request);
          case "configure_fivem_firewall":
            if (state.fail) throw new Error("Administrator permission was cancelled.");
            state.owned = true; return firewall(args.request);
          case "remove_fivem_firewall": state.owned = false; return firewall(args.request);
          case "read_app_logs": return { path: "fixture.log", entries: [] };
          case "get_backup_manager": return { schedules: [], snapshots: [], restoreTests: [], busy: false };
          case "configure_live_bridge": return { workspaceId: "default", enabled: false, connected: false, snapshot: null };
          case "get_installed_windows_artifact_info": return { installed: false, destination: args.destination, hasFxserverExecutable: false, detectionSource: "none" };
          default: return null;
        }
      },
    };
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  const assert = (value, message) => { if (!value) throw new Error(message); };
  const project = page.locator('[aria-label="Project and app updates"]');
  await project.getByRole("button", { name: "Update", exact: true }).click();
  await project.getByRole("button", { name: "Cancel", exact: true }).click();
  assert(await page.evaluate(() => !window.updateFirewallFixture.calls.some((c) => c.command === "download_app_update")), "Cancel started a download");
  await project.getByRole("button", { name: "Update", exact: true }).click();
  await project.getByRole("button", { name: "Update and restart", exact: true }).click();
  await project.getByText("1.0 MB / 4.0 MB", { exact: true }).waitFor();
  assert(await project.getByRole("button", { name: "Update", exact: true }).isDisabled(), "Duplicate update allowed");
  await page.screenshot({ path: "output/playwright/updater-progress.png" });
  await page.getByRole("button", { name: "Firewall", exact: true }).click();
  const firewall = page.locator('[aria-label="FiveM firewall"]');
  await firewall.getByRole("button", { name: "Read profile ports", exact: true }).click();
  assert(await firewall.getByLabel("TCP game port").inputValue() === "30121", "Profile TCP port not loaded");
  assert(await firewall.getByLabel("UDP game port").inputValue() === "30122", "Profile UDP port not loaded");
  await firewall.getByRole("button", { name: "Check firewall", exact: true }).click();
  assert(await firewall.getByRole("button", { name: "Remove app rules", exact: true }).isDisabled(), "External/default rules removable");
  await firewall.getByRole("button", { name: "Set up FiveM rules", exact: true }).click();
  await firewall.getByRole("button", { name: "Cancel", exact: true }).click();
  assert(await page.evaluate(() => !window.updateFirewallFixture.calls.some((c) => c.command === "configure_fivem_firewall")), "Cancel changed firewall");
  await page.evaluate(() => { window.updateFirewallFixture.fail = true; });
  await firewall.getByRole("button", { name: "Set up FiveM rules", exact: true }).click();
  await firewall.getByRole("button", { name: "Apply rules", exact: true }).click();
  await firewall.getByText("Administrator permission was cancelled.", { exact: false }).waitFor();
  await page.evaluate(() => { window.updateFirewallFixture.fail = false; });
  await firewall.getByRole("button", { name: "Check firewall", exact: true }).click();
  await firewall.getByRole("button", { name: "Set up FiveM rules", exact: true }).click();
  await firewall.getByRole("button", { name: "Apply rules", exact: true }).click();
  await firewall.getByText("App rule", { exact: true }).first().waitFor();
  for (const width of [1440, 900]) {
    await page.setViewportSize({ width, height: 1000 });
    await firewall.scrollIntoViewIfNeeded();
    await page.screenshot({ path: `output/playwright/firewall-${width}.png` });
    assert(!await page.evaluate(() => document.documentElement.scrollWidth > innerWidth + 1), "Horizontal overflow");
  }
  await firewall.getByRole("button", { name: "Remove app rules", exact: true }).click();
  await firewall.getByRole("button", { name: "Confirm removal", exact: true }).click();
  await firewall.getByText("Rule missing", { exact: true }).first().waitFor();
  await firewall.getByLabel("TCP game port").fill("0");
  assert(await firewall.getByRole("button", { name: "Check firewall", exact: true }).isDisabled(), "Invalid port accepted");
  assert(await firewall.getByRole("button", { name: "Remove app rules", exact: true }).count() === 0, "Stale inspection survived port change");
  await page.getByRole("navigation", { name: "Workspace navigation" }).getByTitle("Home", { exact: true }).click();
  await project.getByText("1.0 MB / 4.0 MB", { exact: true }).waitFor();
  assert(await page.evaluate(() => window.updateFirewallFixture.calls.filter((c) => c.command === "get_app_update_status").length === 1), "Remount duplicated updater initialization");
  await page.evaluate(() => window.updateFirewallFixture.finishDownload());
  await project.getByText("Test installation blocked: background operation is running.", { exact: false }).waitFor();
  await project.getByRole("button", { name: "Dismiss App update failed", exact: true }).click();
  assert(await project.getByText("App update failed", { exact: true }).count() === 0, "Error not dismissible");
  await project.getByRole("button", { name: "Refresh latest version status" }).click();
  assert(await page.evaluate(() => window.updateFirewallFixture.calls.filter((c) => c.command === "check_app_update").at(-1).args.force === true), "Refresh did not bypass cache");
  assert(errors.length === 0, errors.join("\n"));
  return "PASS: updater progress/remount/exclusive failure, firewall ports/confirmation/cancellation/removal, narrow layout.";
}
