async (page) => {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.setViewportSize({ width: 1440, height: 1000 });
  const origin = new URL(page.url()).origin;
  await page.route("**/*", (route) => {
    const url = new URL(route.request().url());
    if (url.pathname.endsWith("/jsonv2")) return route.fulfill({ json: { recommendedArtifact: "10000", brokenArtifacts: [] } });
    if (url.origin === origin) return route.continue();
    if (url.hostname === "raw.githubusercontent.com") return route.fulfill({ json: { version: "0.3.2" } });
    return route.abort();
  });
  await page.addInitScript(() => {
    localStorage.clear();
    sessionStorage.clear();
    localStorage.setItem("installPath", "C:/mock/artifacts");
    localStorage.setItem("fxserver.manage.env", JSON.stringify({ TXHOST_DATA_PATH: "C:/mock/txData" }));
    localStorage.setItem("fxserver.manage.serverProfile", "default");
    const callbacks = new Map();
    const events = new Map();
    const state = window.configHistoryTest = {
      current: 'set fixture_token "current-fixture-token"\r\nsv_hostname "Fixture current"\r\n',
      previous: 'set fixture_token "history-only-fixture-token"\r\nsv_hostname "Fixture previous"\r\n',
      path: "C:/mock/data/server.cfg", running: false, sequence: 0, versions: [], writes: [],
      calls: [], unknown: [], logs: [], pending: {}, diagnosticIssues: true, resourceRevision: 1,
    };
    const snapshot = (content, reason) => {
      const sequence = ++state.sequence;
      const version = { id: `version-${sequence}`, createdAt: 1700000000000 + sequence * 1000,
        reason, size: new TextEncoder().encode(content).length, digest: `mock-digest-${sequence}` };
      state.versions.unshift({ version, content });
      return version.id;
    };
    state.previousId = snapshot(state.previous, "before-save");
    state.currentId = snapshot(state.current, "save");
    const file = () => ({ name: "server.cfg", path: state.path, content: state.current,
      size: new TextEncoder().encode(state.current).length, modified: 1700000000,
      hasRconPassword: false, hasRconlog: state.current.includes("ensure rconlog") });
    const target = (request) => {
      if (request?.txDataPath !== "C:/mock/txData" || request?.profile !== "default"
          || (request.path !== undefined && request.path !== state.path)) throw new Error("Unexpected config target");
    };
    const stale = () => new Error("CONFIG_CHANGED: Configuration or resource evidence changed after review. Review again.");
    const write = (expected, content, reason) => {
      if (expected !== state.current) throw stale();
      snapshot(state.current, `before-${reason}`);
      state.writes.push({ before: state.current, after: content, reason });
      state.current = content;
      snapshot(content, reason);
      return file();
    };
    const guidance = (page, label, steps, patchAvailable = false) => ({ page, label, steps, patchAvailable });
    const report = () => {
      const checks = [];
      if (state.diagnosticIssues) {
        checks.push({ category: "Dependencies", code: "dependency-missing", severity: "error",
          title: "Required dependency missing", detail: "fixture_job requires fixture_library.",
          resource: "fixture_job", file: "resources/[jobs]/fixture_job", line: null,
          guidance: guidance("resource-manager", "Open resources", ["Inspect the manifest dependency and obtain the missing library from its trusted project source.", "Rerun checks after reviewing the installed resource."]) });
        checks.push({ category: "Configuration", code: "exec-unresolved", severity: "warning",
          title: "Included config not resolved", detail: "An exec target is missing, dynamic, or outside dataPath.",
          resource: null, file: "server.cfg", line: 7,
          guidance: guidance("server-configure", "Open configuration", ["Review server.cfg line 7 and the included file path.", "Keep exec targets inside the selected dataPath."]) });
      }
      checks.push({ category: "RCON", code: "rcon-not-configured", severity: "warning", title: "RCON password missing",
        detail: "No non-empty RCON credential was found.", resource: null, file: null, line: null,
        guidance: guidance("server-configure", "Open RCON configuration", ["Review the executed cfg files and later overrides.", "Set credentials explicitly in the configuration editor; diagnostics never generates or rotates them."]) });
      if (!state.current.includes("ensure rconlog")) checks.push({ category: "RCON", code: "rconlog-not-started", severity: "warning",
        title: "RCON logging not configured", detail: "The installed rconlog has no startup entry.", resource: null, file: null, line: null,
        guidance: guidance("server-configure", "Open configuration", ["Review the installed rconlog and its startup configuration.", "The reviewed patch adds only ensure rconlog; it changes no credentials or services."], !state.diagnosticIssues) });
      return { checkedAt: Math.floor(Date.now() / 1000), blocking: state.diagnosticIssues,
        errorCount: Number(state.diagnosticIssues), warningCount: checks.filter((item) => item.severity === "warning").length,
        resourceCount: 3, configCount: 1, checks };
    };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
      transformCallback: (callback) => { const id = ++state.sequence; callbacks.set(id, callback); return id; },
      invoke: async (command, args = {}) => {
        state.calls.push({ command, args: structuredClone(args) });
        switch (command) {
          case "fetch_latest_app_release": return { version: "0.3.2", tagName: "v0.3.2", htmlUrl: "https://github.com/zoxile/fxserver-installer/releases/tag/v0.3.2", installerUrl: "https://github.com/zoxile/fxserver-installer/releases/download/v0.3.2/FXServer.Installer_0.3.2_windows_x64-setup.exe" };
          case "plugin:window|title": return "FXServer Installer";
          case "plugin:app|version": return "0.3.2";
          case "plugin:event|listen": events.set(args.event, args.handler); return args.handler;
          case "plugin:event|unlisten": return;
          case "plugin:dialog|message": return "Yes";
          case "read_app_logs": return { path: "mock.log", entries: [] };
          case "append_app_log": state.logs.push(args.entry); return;
          case "initialize_health_workspace": return;
          case "configure_live_bridge": return { workspaceId: args.target.workspaceId, enabled: false, connected: false, snapshot: null, receivedAt: null };
          case "get_windows_artifact_metadata": return { recommendedArtifact: "10000", windowsDownloadLink: "https://example.invalid/artifact.zip", brokenArtifacts: [] };
          case "get_installed_windows_artifact_info": return { installed: true, version: "10000", hasFxserverExecutable: true, detectionSource: "marker" };
          case "list_txdata_profiles": return { dataPath: args.dataPath, profiles: ["default"], hasRootLogs: false };
          case "get_fxserver_status": return { running: state.running, pid: state.running ? 123 : null, startedAt: null, resources: null };
          case "read_server_config":
            target(args.request);
            return { txDataPath: "C:/mock/txData", profile: "default", profileConfigPath: "C:/mock/txData/default/config.json",
              dataPath: "C:/mock/data", files: [file()], rconPasswordFound: false, rconPasswordFile: null,
              rconPasswordLine: null, rconlogFound: file().hasRconlog, rconlogLine: null };
          case "read_config_history_file": target(args.request); return file();
          case "list_config_history": target(args.request); return state.versions.map((item) => structuredClone(item.version));
          case "read_config_history_version": {
            target(args.request);
            const version = state.versions.find((item) => item.version.id === args.versionId);
            if (!version) throw new Error("Version no longer exists. Refresh history.");
            return structuredClone(version);
          }
          case "save_server_config":
            target({ txDataPath: args.txDataPath, profile: args.profile, path: args.request.path });
            return write(args.expectedContent, args.request.content, "save");
          case "restore_config_history_version": {
            target(args.request);
            if (state.running) throw new Error("Stop FXServer before changing managed server files.");
            const version = state.versions.find((item) => item.version.id === args.versionId);
            if (!version) throw new Error("Version no longer exists. Refresh history.");
            return write(args.expectedContent, version.content, "restore");
          }
          case "run_fxserver_preflight": target(args.request); return report();
          case "preview_diagnostic_config_patch": {
            target(args.request);
            if (args.request.credentials || args.request.checkPorts !== false) throw new Error("Patch preview included database credentials or port probing");
            if (state.diagnosticIssues || state.current.includes("ensure rconlog")) throw new Error("No safe patch available.");
            const id = `patch-${++state.sequence}`;
            const newline = state.current.includes("\r\n") ? "\r\n" : "\n";
            const preview = { id, path: state.path, expiresAt: Math.floor(Date.now() / 1000) + 900,
              before: state.current, after: `${state.current}${state.current.endsWith("\n") ? "" : newline}ensure rconlog${newline}` };
            state.pending[id] = { ...preview, resourceRevision: state.resourceRevision };
            return preview;
          }
          case "apply_diagnostic_config_patch": {
            if (Object.keys(args).length !== 1 || !args.previewId) throw new Error("Patch apply must use only the reviewed preview ID");
            if (state.running) throw new Error("Stop FXServer before changing managed server files.");
            const preview = state.pending[args.previewId];
            delete state.pending[args.previewId];
            if (!preview) throw new Error("Repair preview expired or was already used.");
            if (preview.resourceRevision !== state.resourceRevision) throw stale();
            return write(preview.before, preview.after, "patch");
          }
          default: state.unknown.push(command); throw new Error(`Unmocked command: ${command}`);
        }
      },
    };
  });
  await page.reload({ waitUntil: "domcontentloaded", timeout: 120000 });
  const nav = page.getByRole("navigation", { name: "Workspace navigation" });
  const navigate = async (title) => {
    const parent = nav.getByTitle("FXServer", { exact: true });
    if (await parent.getAttribute("aria-expanded") !== "true") await parent.click();
    await nav.getByTitle(title, { exact: true }).click();
  };
  const check = (condition, message) => { if (!condition) throw new Error(message); };
  const history = page.getByRole("region", { name: "Configuration history", exact: true });
  const editor = page.getByTitle("Server config editor", { exact: true });
  const chooseVersion = async (key) => {
    const label = await page.evaluate((key) => {
      const state = window.configHistoryTest;
      const version = state.versions.find((item) => item.version.id === state[key]).version;
      return `${new Date(version.createdAt).toLocaleString()} / ${version.reason.replaceAll("-", " ")} / ${version.size} B`;
    }, key);
    await history.getByRole("button", { name: "Configuration version", exact: true }).click();
    await page.getByRole("option", { name: label, exact: true }).click();
    await history.getByRole("checkbox", { name: "Reveal config contents, including secrets", exact: true }).waitFor();
  };
  const screenshots = async (section, prefix) => {
    for (const [label, width, height] of [["desktop", 1440, 1000], ["narrow", 760, 1000]]) {
      await page.setViewportSize({ width, height });
      await section.scrollIntoViewIfNeeded();
      await page.screenshot({ path: `output/playwright/${prefix}-${label}.png`, fullPage: true });
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth > innerWidth + 1);
      check(!overflow, `${prefix} has horizontal overflow at ${width}px`);
      const bounds = await section.boundingBox();
      check(bounds && bounds.width > 200 && bounds.x >= 0 && bounds.x + bounds.width <= width + 1, `${prefix} is clipped at ${width}px`);
    }
    await page.setViewportSize({ width: 1440, height: 1000 });
  };

  await navigate("Configure Server");
  await history.getByText("Encrypted on this Windows account.", { exact: false }).waitFor();
  await chooseVersion("previousId");
  check(await page.evaluate(() => window.configHistoryTest.calls.filter((call) => call.command === "list_config_history").length === 1), "History load retriggered from task-state updates");
  const restore = history.getByRole("button", { name: "Restore selected file", exact: true });
  const historyReveal = history.getByRole("checkbox", { name: "Reveal config contents, including secrets", exact: true });
  const historyReview = history.getByRole("checkbox", { name: "I reviewed this file replacement.", exact: true });
  check(!await historyReveal.isChecked(), "Historical secrets were revealed by default");
  check(!((await history.textContent()) || "").includes("history-only-fixture-token"), "Hidden history secret rendered in the DOM");
  check(await restore.isDisabled() && await historyReview.isDisabled(), "Restore review gate was bypassed");
  await historyReveal.check();
  await history.getByRole("region", { name: "Selected version", exact: true }).waitFor();
  check(((await history.textContent()) || "").includes("history-only-fixture-token"), "Revealed history did not show the selected content");
  check(await restore.isDisabled(), "Reveal alone enabled restore");
  await historyReview.check();
  await screenshots(history, "config-history");
  await page.evaluate(() => { window.configHistoryTest.running = true; });
  await restore.click();
  await history.getByText("Stop FXServer before changing managed server files.", { exact: false }).waitFor();
  check(await page.evaluate(() => window.configHistoryTest.writes.length === 0), "Running-server restore changed the mock file");
  await page.evaluate(() => { window.configHistoryTest.running = false; });
  await restore.click();
  await page.waitForFunction(() => window.configHistoryTest.writes.some((write) => write.reason === "restore"));
  await page.getByText("server.cfg restored. The previous content is in history.", { exact: true }).waitFor();
  check(await editor.inputValue() === await page.evaluate(() => window.configHistoryTest.previous.replaceAll("\r\n", "\n")), "Restore did not update the editor");
  check(await page.evaluate(() => window.configHistoryTest.versions.some((item) => item.version.reason === "before-restore" && item.content.includes("current-fixture-token"))), "Restore did not preserve the previous file in history");

  await chooseVersion("currentId");
  await editor.fill(`${await editor.inputValue()}# unsaved draft\n`);
  await history.getByText("Save or revert your draft before restoring a version.", { exact: true }).waitFor();
  check(await restore.isDisabled(), "Dirty draft did not block restore");
  await page.getByRole("button", { name: "Revert", exact: true }).click();
  await editor.fill(`${await editor.inputValue()}# reviewed draft\n`);
  await page.evaluate(() => { window.configHistoryTest.current += "# external editor change\n"; });
  const save = page.getByRole("button", { name: "Save", exact: true });
  await save.click();
  await page.getByText("This file changed on disk. Saving is paused.", { exact: true }).waitFor();
  check(await save.isDisabled(), "Stale-save conflict did not pause saving");
  const keepDraft = page.getByRole("button", { name: "Keep reviewed draft", exact: true });
  check(await keepDraft.isDisabled(), "External changes did not require explicit review");
  await page.getByRole("checkbox", { name: "I reviewed the external changes and my draft.", exact: true }).check();
  await keepDraft.click();
  await save.click();
  await page.getByText("server.cfg saved with encrypted history.", { exact: true }).waitFor();
  check(await page.evaluate(() => window.configHistoryTest.writes.filter((write) => write.reason === "save").length === 1
    && window.configHistoryTest.versions.some((item) => item.version.reason === "before-save" && item.content.includes("# external editor change"))), "Reviewed stale-save did not preserve the external source snapshot");

  await navigate("Diagnostics");
  await page.getByRole("button", { name: "Run checks", exact: true }).click();
  await page.getByText("Required dependency missing", { exact: true }).waitFor();
  const dependency = page.getByText("Required dependency missing", { exact: true }).locator("../..");
  await dependency.getByText("Recommended next steps", { exact: true }).click();
  await dependency.getByText("Inspect the manifest dependency", { exact: false }).waitFor();
  check(await dependency.getByRole("button", { name: "Open resources", exact: true }).isVisible(), "Missing dependency has no actionable destination");
  const exec = page.getByText("Included config not resolved", { exact: true }).locator("../..");
  await exec.getByText("server.cfg:7", { exact: true }).waitFor();
  check(await page.getByRole("button", { name: "Review rconlog patch", exact: true }).count() === 0, "Incomplete diagnostics offered a patch");
  await exec.getByRole("button", { name: "Open configuration", exact: true }).click();
  await page.getByRole("heading", { name: "Configure Server", exact: true }).waitFor();
  await page.evaluate(() => { window.configHistoryTest.diagnosticIssues = false; });
  await navigate("Diagnostics");
  await page.getByRole("button", { name: "Run checks", exact: true }).click();
  await page.getByRole("button", { name: "Review rconlog patch", exact: true }).click();
  const patch = page.getByRole("region", { name: "Review configuration repair", exact: true });
  const patchReveal = patch.getByRole("checkbox", { name: "Reveal config contents, including secrets", exact: true });
  const patchReview = patch.getByRole("checkbox", { name: "I reviewed this exact file change.", exact: true });
  const apply = patch.getByRole("button", { name: "Apply reviewed patch", exact: true });
  await patch.waitFor();
  check(!await patchReveal.isChecked() && await patchReview.isDisabled() && await apply.isDisabled(), "Patch review/reveal defaults were unsafe");
  check(!((await patch.textContent()) || "").includes("history-only-fixture-token"), "Patch secret rendered before reveal");
  await patchReveal.check();
  await patch.getByRole("region", { name: "Reviewed repair", exact: true }).waitFor();
  check(await apply.isDisabled(), "Reveal alone enabled patch apply");
  await patchReview.check();
  await screenshots(patch, "guided-diagnostics");
  await page.evaluate(() => { window.configHistoryTest.resourceRevision++; });
  await apply.click();
  await page.getByText("CONFIG_CHANGED:", { exact: false }).first().waitFor();
  check(await page.evaluate(() => !window.configHistoryTest.writes.some((write) => write.reason === "patch")), "Stale patch evidence changed the mock file");
  await page.getByRole("button", { name: "Run checks", exact: true }).click();
  await page.getByRole("button", { name: "Review rconlog patch", exact: true }).click();
  await patchReveal.check();
  await patchReview.check();
  await apply.click();
  await page.getByText("server.cfg patched.", { exact: false }).waitFor();
  await page.getByRole("button", { name: "Run checks", exact: true }).waitFor();
  check(await page.evaluate(() => {
    const state = window.configHistoryTest;
    const writes = state.writes.filter((write) => write.reason === "patch");
    return writes.length === 1 && writes[0].after === `${writes[0].before}ensure rconlog\n`
      && state.versions.some((item) => item.version.reason === "before-patch" && item.content === writes[0].before);
  }), "Patch changed more than the reviewed startup line or omitted the recovery snapshot");

  const outcome = await page.evaluate(() => {
    const state = window.configHistoryTest;
    const persisted = JSON.stringify([Object.entries(localStorage), Object.entries(sessionStorage), state.logs]);
    return { unknown: state.unknown, calls: state.calls.map((call) => call.command),
      secretLeak: ["history-only-fixture-token", "current-fixture-token"].some((secret) => persisted.includes(secret)) };
  });
  check(!outcome.secretLeak, "Configuration secrets leaked into browser storage or application logs");
  check(!outcome.calls.some((command) => /^(start|stop|restart)_fxserver$|rcon_password$|mariadb|service/.test(command)), "Smoke unexpectedly changed credentials/services or accessed a database");
  check(!outcome.unknown.length, `Missing mock IPC handlers: ${outcome.unknown.join(", ")}`);
  check(!errors.length, errors.join("\n"));
  await page.evaluate(() => { window.configHistoryTest.completed = true; });
  console.log("PASS (mock IPC only): history reveal/diff/review, stopped restore, dirty/stale-save guards, guided evidence/actions, stale patch rejection, reviewed patch/history, secret persistence checks, desktop/narrow screenshots. DPAPI encryption is covered by Rust fixtures, not this UI smoke.");
}
