async (page) => {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.addInitScript(() => {
    localStorage.clear();
    localStorage.setItem("installPath", "C:/fixture/artifacts");
    localStorage.setItem("fxserver.manage.env", JSON.stringify({ TXHOST_DATA_PATH: "C:/fixture/txData" }));
    localStorage.setItem("fxserver.manage.serverProfile", "default");
    const makeLine = (id, text) => ({ id, stream: "stdout", line: text, plainLine: text, timestamp: String(1700000000 + id), segments: [{ text, color: "#ee6666" }] });
    const state = window.consoleIssueFixture = { calls: [], counter: 0, entries: [], hold: false, pending: null, makeLine };
    state.entries = Array.from({ length: 1000 }, (_, id) => makeLine(id,
      id > 990 ? id % 2 ? "[script:inventory] SCRIPT ERROR: @inventory/server.lua:42: missing item" : "Nearby context line"
      : id === 990 ? "[script:auth] error: token=private-issue-fixture"
      : `[script:resource-${id % 70}] warning: unique-${id % 70}`));
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
      transformCallback: () => ++state.counter,
      invoke: async (command, args = {}) => {
        state.calls.push({ command, args });
        switch (command) {
          case "plugin:window|title": return "FXServer Installer";
          case "plugin:app|version": return "0.5.1";
          case "plugin:event|listen": return ++state.counter;
          case "read_app_logs": return { path: "fixture.log", entries: [] };
          case "get_windows_artifact_metadata": return { recommendedArtifact: "10000", windowsDownloadLink: "https://fixture.invalid", brokenArtifacts: [] };
          case "get_installed_windows_artifact_info": return { installed: true, hasFxserverExecutable: true, version: "10000" };
          case "list_txdata_profiles": return { dataPath: args.dataPath, profiles: ["default"], hasRootLogs: true, exists: true };
          case "read_txdata_log": return { path: "C:/fixture/txData/default/logs/fxserver.log", logName: "fxserver.log", lineCount: 1, content: "[core] raw log preserved" };
          case "get_fxserver_terminal": {
            const entries = state.entries.slice(-args.maxLines);
            if (state.hold) return new Promise((resolve) => { state.pending = () => { state.pending = null; resolve({ entries }); }; });
            return { entries };
          }
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
  const logs = nav.getByTitle("Logs", { exact: true });
  if (await logs.getAttribute("aria-expanded") !== "true") await logs.click();
  await nav.getByTitle("Server Logs", { exact: true }).click();
  await page.getByText("raw log preserved", { exact: true }).waitFor();
  if (await page.evaluate(() => window.consoleIssueFixture.calls.some((call) => call.command === "get_fxserver_terminal"))) throw new Error("Inactive issues tab polled the console");
  await page.getByRole("tab", { name: "Console Issues", exact: true }).click();
  const panel = page.getByRole("region", { name: "Grouped console issues", exact: true });
  await panel.getByText("1000 / 1000 retained lines; 72 groups in this window", { exact: true }).waitFor();
  const rows = () => panel.getByRole("button", { name: /^(Error|Warning) in / });
  if (await rows().count() !== 50) throw new Error("Group DOM limit exceeded");
  await panel.evaluate((element) => {
    window.consoleIssueFixture.mutations = 0;
    const list = element.querySelector('button[aria-label^="Error in "]').parentElement;
    window.consoleIssueFixture.observer = new MutationObserver((records) => { window.consoleIssueFixture.mutations += records.length; });
    window.consoleIssueFixture.observer.observe(list, { subtree: true, childList: true, characterData: true });
  });
  await page.waitForTimeout(1150);
  const unchanged = await page.evaluate(() => {
    window.consoleIssueFixture.observer.disconnect();
    return { mutations: window.consoleIssueFixture.mutations, requests: window.consoleIssueFixture.calls.filter((call) => call.command === "get_fxserver_terminal").map((call) => call.args) };
  });
  if (unchanged.mutations) throw new Error("Unchanged polling rerendered the issue list");
  if (unchanged.requests.some((args) => args.maxLines !== 1000 || args.afterId !== null)) throw new Error("Console polls were not bounded snapshots");
  await panel.getByRole("button", { name: "Next issue page", exact: true }).click();
  if (await rows().count() !== 22) throw new Error("Second issue page is incorrect");
  const search = panel.getByRole("textbox", { name: "Search console issues" });
  await search.fill("inventory");
  if (await rows().count() !== 1) throw new Error("Resource search failed");
  await rows().first().click();
  const nearby = panel.getByLabel("Nearby console lines", { exact: true });
  await nearby.waitFor();
  if (!(await nearby.textContent()).includes("Nearby context line")) throw new Error("Missing neighboring raw context");
  await panel.getByRole("button", { name: "Previous occurrence", exact: true }).click();
  await panel.getByText("4 of 5", { exact: true }).waitFor();
  await page.screenshot({ path: "output/playwright/console-issues-desktop.png" });
  await search.fill("auth");
  await rows().first().click();
  if ((await panel.textContent()).includes("private-issue-fixture")) throw new Error("Credential leaked into issue context");
  await panel.getByRole("button", { name: "Close issue context" }).click();
  await search.fill("");
  await panel.getByRole("button", { name: "Errors", exact: true }).click();
  if (await rows().count() !== 2) throw new Error("Severity filtering failed");
  await panel.getByRole("button", { name: "All", exact: true }).click();
  await page.setViewportSize({ width: 390, height: 844 });
  await search.fill("inventory"); await rows().first().click();
  await page.screenshot({ path: "output/playwright/console-issues-mobile.png", fullPage: true });
  if (await page.evaluate(() => document.documentElement.scrollWidth > innerWidth + 1)) throw new Error("Console issues overflow mobile viewport");
  await page.evaluate(() => { window.consoleIssueFixture.entries = [window.consoleIssueFixture.makeLine(1001, "warning: replacement buffer")]; });
  await panel.getByText("1 / 1000 retained lines; 1 group in this window", { exact: true }).waitFor();
  await panel.getByText("This occurrence is no longer in the retained console window.", { exact: true }).waitFor();
  await search.fill("");
  await panel.getByRole("button", { name: "Close issue context" }).click();
  if ((await panel.textContent()).includes("inventory")) throw new Error("Previous server groups survived the reset");
  await page.evaluate(() => { window.consoleIssueFixture.hold = true; });
  await page.waitForFunction(() => !!window.consoleIssueFixture.pending);
  await page.getByRole("tab", { name: "txData Logs", exact: true }).click();
  await page.evaluate(() => window.consoleIssueFixture.pending());
  await page.getByText("raw log preserved", { exact: true }).waitFor();
  const calls = await page.evaluate(() => window.consoleIssueFixture.calls.filter((call) => call.command === "get_fxserver_terminal").length);
  await page.waitForTimeout(1150);
  if (await page.evaluate(() => window.consoleIssueFixture.calls.filter((call) => call.command === "get_fxserver_terminal").length) !== calls) throw new Error("Disposed issue view continued polling");
  const leaked = await page.evaluate(() => Object.keys(localStorage).some((key) => /private-issue-fixture|unique-69|missing item/.test(localStorage.getItem(key) || "")));
  if (leaked) throw new Error("Issue data was persisted");
  if (errors.length) throw new Error(errors.join("\n"));
  return "Console issues: desktop/mobile, grouping, search, severity, pagination, context, redaction, reset and disposal passed (mocked console).";
}
