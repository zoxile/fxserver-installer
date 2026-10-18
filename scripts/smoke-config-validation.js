async (page) => {
  const failures = [];
  page.on("pageerror", (error) => failures.push(error.message));
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.addInitScript(() => {
    localStorage.clear();
    sessionStorage.clear();
    localStorage.setItem("installPath", "C:/mock/artifacts");
    localStorage.setItem("fxserver.manage.env", JSON.stringify({ TXHOST_DATA_PATH: "C:/mock/txData" }));
    localStorage.setItem("fxserver.manage.serverProfile", "default");
    const state = window.validationTest = { content: "sv_hostname old\r\n", writes: [], logs: [], validations: 0, revision: 1, issue: "warning", delay: 0, running: false, fail: false };
    const file = () => ({ name: "server.cfg", path: "C:/mock/data/server.cfg", content: state.content, size: state.content.length, modified: 1700000000, hasRconPassword: false, hasRconlog: false });
    let sequence = 0;
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
      transformCallback: () => ++sequence,
      invoke: async (command, args = {}) => {
        switch (command) {
          case "get_platform_info": return { os: "windows", arch: "x86_64", artifactPlatform: "windows", clientLogsSupported: true, secureStorageSupported: true };
          case "plugin:app|version": return "0.5.1";
          case "plugin:window|title": return "FXServer Installer";
          case "plugin:event|listen": return ++sequence;
          case "plugin:event|unlisten": return;
          case "plugin:dialog|message": return "Yes";
          case "fetch_latest_app_release": return { version: "0.5.1", tagName: "v0.5.1", htmlUrl: "https://github.com/zoxile/fxserver-installer/releases/tag/v0.5.1", installerUrl: null };
          case "read_app_logs": return { path: "mock.log", entries: [] };
          case "append_app_log": state.logs.push(args.entry); return;
          case "initialize_health_workspace": return;
          case "load_database_login": return null;
          case "clear_database_login": return;
          case "configure_live_bridge": return { workspaceId: args.target.workspaceId, enabled: false, connected: false, snapshot: null, receivedAt: null };
          case "get_windows_artifact_metadata": return { recommendedArtifact: "10000", windowsDownloadLink: "https://example.invalid/artifact.zip", brokenArtifacts: [] };
          case "get_installed_windows_artifact_info": return { installed: true, version: "10000", hasFxserverExecutable: true, detectionSource: "marker" };
          case "list_txdata_profiles": return { dataPath: args.dataPath, profiles: ["default"], hasRootLogs: false };
          case "get_fxserver_status": return { running: state.running, pid: null, startedAt: null, resources: null };
          case "read_server_config": return { txDataPath: "C:/mock/txData", profile: "default", profileConfigPath: "C:/mock/txData/default/config.json", dataPath: "C:/mock/data", files: [file()], rconPasswordFound: false, rconPasswordFile: null, rconPasswordLine: null, rconlogFound: false, rconlogLine: null };
          case "read_config_history_file": return file();
          case "list_config_history": return [];
          case "validate_config_change": {
            state.validations++;
            const report = { revision: `${state.revision}:${args.content}`, fileCount: 2, issues: state.issue ? [{ code: "fixture", severity: state.issue, file: "Edited file", line: 1, message: state.issue === "error" ? "Blocking fixture error." : "Potentially sensitive setting. Values are omitted." }] : [] };
            await new Promise((resolve) => setTimeout(resolve, state.delay));
            if (state.fail) throw new Error("fixture-secret must never appear in validation errors");
            return report;
          }
          case "save_server_config":
            if (state.running) throw new Error("Stop FXServer before changing managed server files.");
            if (state.content !== args.expectedContent) throw new Error("CONFIG_CHANGED: Reload and review the current file.");
            state.writes.push({ before: args.expectedContent, after: args.request.content });
            state.content = args.request.content;
            return file();
          default: throw new Error(`Unmocked command: ${command}`);
        }
      },
    };
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  const nav = page.getByRole("navigation", { name: "Workspace navigation" });
  const parent = nav.getByTitle("FXServer", { exact: true });
  if (await parent.getAttribute("aria-expanded") !== "true") await parent.click();
  await nav.getByTitle("Configure Server", { exact: true }).click();
  const editor = page.getByTitle("Server config editor", { exact: true });
  await editor.waitFor();
  const check = (value, message) => { if (!value) throw new Error(message); };
  const review = page.getByRole("region", { name: "Configuration change review", exact: true });
  const save = page.getByRole("button", { name: "Save", exact: true });
  const apply = review.getByRole("button", { name: "Save reviewed change", exact: true });
  const reveal = review.getByRole("checkbox", { name: "Reveal exact diff, including secrets", exact: true });
  const acceptDiff = review.getByRole("checkbox", { name: "I reviewed this exact file change.", exact: true });
  const acceptWarnings = review.getByRole("checkbox", { name: "I reviewed the advisory warnings and accept them for this save only.", exact: true });
  const approve = async () => { await reveal.check(); await acceptDiff.check(); await acceptWarnings.check(); };
  await editor.fill('sv_hostname new\nset fixture_token "fixture-secret"\n');
  await save.click();
  await review.waitFor();
  check(!((await review.textContent()) || "").includes("fixture-secret"), "Secret was rendered before exact-diff reveal");
  check(await apply.isDisabled(), "Unreviewed draft could save");
  await reveal.check();
  await acceptDiff.check();
  check(await apply.isDisabled(), "Warnings were bypassed without acknowledgement");
  await acceptWarnings.check();
  for (const width of [1440, 760]) {
    await page.setViewportSize({ width, height: 1000 });
    await review.scrollIntoViewIfNeeded();
    await page.screenshot({ path: `output/playwright/config-validation-${width}.png`, fullPage: true });
    check(!await page.evaluate(() => document.documentElement.scrollWidth > innerWidth + 1), "Horizontal overflow");
  }
  await page.evaluate(() => { window.validationTest.revision++; });
  await apply.click();
  await page.getByText("Configuration dependencies changed during review.", { exact: false }).waitFor();
  check(!await acceptDiff.isChecked() && await apply.isDisabled(), "Changed dependencies retained review consent");
  check(await page.evaluate(() => window.validationTest.writes.length === 0), "Changed dependencies were saved");
  await approve();
  await page.evaluate(() => { window.validationTest.running = true; });
  await apply.click();
  await page.getByText("Stop FXServer before changing managed server files.", { exact: false }).waitFor();
  check(await page.evaluate(() => window.validationTest.writes.length === 0), "Running-server guard bypassed");
  await page.evaluate(() => { window.validationTest.running = false; window.validationTest.content += "# external\n"; });
  await apply.click();
  await page.getByText("This file changed on disk. Saving is paused.", { exact: true }).waitFor();
  check(await save.isDisabled(), "External changes did not block Save");
  await page.getByRole("checkbox", { name: "I reviewed the external changes and my draft.", exact: true }).check();
  await page.getByRole("button", { name: "Keep reviewed draft", exact: true }).click();
  await save.click();
  await approve();
  await apply.click();
  await page.getByText("server.cfg saved with encrypted history.", { exact: true }).waitFor();
  check(await page.evaluate(() => window.validationTest.writes.length === 1 && window.validationTest.writes[0].before.endsWith("# external\n")), "Guarded history write lost its reviewed baseline");

  await editor.fill("set x one; set x two\n");
  await page.evaluate(() => { window.validationTest.issue = "error"; });
  await save.click();
  await review.getByText("Blocking fixture error.", { exact: true }).waitFor();
  await reveal.check();
  check(await acceptDiff.isDisabled() && await apply.isDisabled(), "Hard error was overridable");
  await editor.fill("set x changed\n");
  await review.waitFor({ state: "detached" });
  await page.evaluate(() => { window.validationTest.issue = "warning"; window.validationTest.delay = 500; });
  await save.click();
  await editor.fill("set x edited-during-validation\n");
  await page.waitForTimeout(700);
  check(await review.count() === 0, "Stale validation result attached to newer draft");
  await page.evaluate(() => { window.validationTest.delay = 0; window.validationTest.fail = true; });
  await save.click();
  await page.getByText("Configuration validation could not complete.", { exact: false }).waitFor();
  check(!((await page.locator("body").textContent()) || "").includes("fixture-secret must never"), "Raw validation exception leaked");
  const leaked = await page.evaluate(() => JSON.stringify([Object.entries(localStorage), Object.entries(sessionStorage), window.validationTest.logs]).includes("fixture-secret"));
  check(!leaked, "Secret leaked into logs or browser storage");
  check(!failures.length, failures.join("\n"));
  return "PASS (mock IPC): exact diff/reveal, advisory consent, hard errors, stale drafts/dependencies, original write guards, secret-safe failures, desktop/narrow screenshots.";
}
