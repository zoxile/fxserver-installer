async (page) => {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.setViewportSize({ width: 1440, height: 1000 });
  const epoch = Date.parse("2026-10-18T10:00:00Z");
  await page.clock.setFixedTime(epoch);
  await page.addInitScript(() => {
    localStorage.clear();
    localStorage.setItem("installPath", "C:/fixture/artifacts");
    const state = window.chartFixture = { running: false, cpu: 0, memory: 0, unknown: [], nextId: 0 };
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
    window.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
      transformCallback: () => ++state.nextId,
      invoke: async (command, args = {}) => {
        switch (command) {
          case "plugin:window|title": return "FXServer Installer";
          case "plugin:app|version": return "0.5.2";
          case "plugin:event|listen": return ++state.nextId;
          case "plugin:event|unlisten":
          case "append_app_log":
          case "initialize_health_workspace": return;
          case "read_app_logs": return { path: "fixture.log", entries: [] };
          case "get_app_update_status": return { phase: "disabled", currentVersion: "0.5.2", availableVersion: null, releaseUrl: null, downloadedBytes: 0, totalBytes: null, message: "Updates disabled in fixture.", supported: false };
          case "fetch_latest_app_release": return { version: "0.5.2", tagName: "v0.5.2", htmlUrl: "https://github.com/zoxile/fxserver-installer/releases/tag/v0.5.2", installerUrl: null };
          case "get_fxserver_rcon_password": return "";
          case "load_database_login": return null;
          case "list_txdata_profiles": return { dataPath: args.dataPath, profiles: ["default"], hasRootLogs: false, exists: true };
          case "configure_live_bridge": return { workspaceId: args.target.workspaceId, enabled: false, connected: false, snapshot: null };
          case "get_windows_artifact_metadata": return { recommendedArtifact: "10000", windowsDownloadLink: "https://fixture.invalid/artifact.zip", brokenArtifacts: [] };
          case "get_installed_windows_artifact_info": return { installed: true, version: "10000", destination: "C:/fixture/artifacts", hasFxserverExecutable: true, detectionSource: "marker" };
          case "get_fxserver_status": return {
            running: state.running, pid: 123, startedAt: "1792317600",
            resources: { cpuPercent: state.cpu, memoryPercent: state.memory, memoryBytes: state.memory * 10485760, totalMemoryBytes: 1048576000, threadCount: 16, handleCount: 80 },
          };
          case "get_fxserver_terminal": return { entries: [] };
          default: state.unknown.push(command); throw new Error(`Unmocked desktop command: ${command}`);
        }
      },
    };
  });
  await page.reload({ waitUntil: "domcontentloaded" });
  const nav = page.getByRole("navigation", { name: "Workspace navigation" });
  const navigate = async (title) => {
    if (title !== "Home") {
      const parent = nav.getByTitle("FXServer", { exact: true });
      if (await parent.getAttribute("aria-expanded") !== "true") await parent.click();
    }
    const target = nav.getByTitle(title, { exact: true });
    await target.click();
    await target.and(nav.locator('[aria-current="page"]')).waitFor();
  };
  const manage = async () => {
    await navigate("Manage Server");
    await page.getByText("Performance", { exact: true }).waitFor();
  };
  await manage();
  const charts = page.locator("[data-chart]");
  if (await charts.count() !== 2) throw new Error("Expected CPU and RAM charts");
  await page.getByText("Start FXServer to collect CPU samples.", { exact: true }).waitFor();
  await page.getByText("Start FXServer to collect RAM samples.", { exact: true }).waitFor();

  const samples = [
    { seconds: 0, cpu: 10, memory: 20 },
    { seconds: 1800, cpu: 25, memory: 35 },
    { seconds: 3000, cpu: 40, memory: 45 },
    { seconds: 3300, cpu: 55, memory: 60 },
    { seconds: 3530, cpu: 75, memory: 70 },
    { seconds: 3570, cpu: 30, memory: 55 },
    { seconds: 3590, cpu: 65, memory: 80 },
  ];
  for (const sample of samples) {
    await page.clock.setFixedTime(epoch + sample.seconds * 1000);
    await page.evaluate((sample) => Object.assign(window.chartFixture, sample, { running: true }), sample);
    await page.getByRole("button", { name: "Status", exact: true }).click();
    await page.getByText(`${sample.cpu.toFixed(2)}%`, { exact: true }).first().waitFor();
  }

  const sampleCounts = () => page.getByText("Samples", { exact: true }).evaluateAll((labels) => labels.map((label) => Number(label.nextElementSibling.textContent)));
  const paths = () => charts.locator("path.lc-area-path").evaluateAll((nodes) => nodes.map((node) => node.getAttribute("d")));
  const rangeTimes = [];
  await page.evaluate(() => {
    window.chartLongTasks = [];
    new PerformanceObserver((list) => window.chartLongTasks.push(...list.getEntries().map((entry) => entry.duration))).observe({ type: "longtask" });
  });
  let previousPaths;
  for (const [range, expected] of [["30s", 2], ["5m", 4], ["10m", 5], ["30m", 6], ["1h", 7]]) {
    const start = performance.now();
    await page.getByTitle(`Show the last ${range}`, { exact: true }).click();
    await page.waitForFunction((expected) => [...document.querySelectorAll("p")].filter((node) => node.textContent === "Samples").every((node) => Number(node.nextElementSibling.textContent) === expected), expected);
    const currentPaths = await paths();
    if (currentPaths.length !== 2 || currentPaths.some((path) => !path || /NaN|Infinity/.test(path))) throw new Error(`Invalid chart paths for ${range}`);
    if (previousPaths && currentPaths.some((path, index) => path === previousPaths[index])) throw new Error(`Chart did not update for ${range}`);
    previousPaths = currentPaths;
    rangeTimes.push({ range, milliseconds: Math.round(performance.now() - start) });
  }

  const checkChart = async (chart, label, expected) => {
    await chart.scrollIntoViewIfNeeded();
    const details = await chart.evaluate((root) => {
      const area = root.querySelector("path.lc-area-path");
      const line = root.querySelector("path.lc-area-line");
      const ticks = [...root.querySelectorAll(".lc-axis-tick-label")];
      const lines = [...root.querySelectorAll(".lc-grid-x-rule, .lc-grid-y-rule")];
      const rect = (node) => { const b = node.getBoundingClientRect(); return { x: b.x, y: b.y, width: b.width, height: b.height }; };
      return {
        fill: area && getComputedStyle(area).fill,
        stroke: line && getComputedStyle(line).stroke,
        bounds: rect(root),
        ticks: ticks.map((node) => ({ text: node.textContent, fill: getComputedStyle(node).fill, ...rect(node) })),
        grid: lines.map(rect),
      };
    });
    if (!details.fill || details.fill === "none" || details.fill === "rgb(0, 0, 0)") throw new Error(`${label} area lost its color`);
    if (!details.stroke || details.stroke === "none") throw new Error(`${label} line is missing`);
    if (!details.grid.some((line) => line.width < 2 && line.height > 100) || !details.grid.some((line) => line.height < 2 && line.width > 100)) throw new Error(`${label} needs horizontal and vertical grid lines`);
    if (details.ticks.length < 2 || details.ticks.some((tick) => !/\d\d:\d\d:\d\d/.test(tick.text) || tick.fill === "rgb(0, 0, 0)")) throw new Error(`${label} timestamps are missing or unreadable`);
    if (details.ticks.some((tick) => tick.x < details.bounds.x || tick.x + tick.width > details.bounds.x + details.bounds.width)) throw new Error(`${label} timestamps are clipped`);
    for (let index = 1; index < details.ticks.length; index++) {
      const before = details.ticks[index - 1];
      if (before.x + before.width > details.ticks[index].x + 1) throw new Error(`${label} timestamps overlap`);
    }
    const chartBounds = await chart.boundingBox();
    const plotBounds = await chart.locator("path.lc-area-path").boundingBox();
    await page.mouse.move(plotBounds.x + plotBounds.width - 0.1, chartBounds.y + chartBounds.height / 2);
    const tooltip = chart.locator(".lc-tooltip-root");
    await tooltip.waitFor();
    await tooltip.getByText(`${label} Usage`, { exact: true }).waitFor();
    await tooltip.getByText(`${expected.toFixed(2)}%`, { exact: true }).waitFor();
    const tooltipBounds = await tooltip.boundingBox();
    if (!tooltipBounds || tooltipBounds.x < chartBounds.x - 1 || tooltipBounds.x + tooltipBounds.width > chartBounds.x + chartBounds.width + 1) throw new Error(`${label} tooltip is clipped`);
    await chart.screenshot({ path: `output/playwright/performance-${label.toLowerCase()}-${Math.round(chartBounds.width)}.png`, animations: "disabled" });
    return details;
  };
  for (const width of [1440, 900]) {
    await page.setViewportSize({ width, height: 1000 });
    const cpu = await checkChart(charts.nth(0), "CPU", 65);
    const ram = await checkChart(charts.nth(1), "RAM", 80);
    if (cpu.fill === ram.fill) throw new Error("CPU and RAM must use distinct colors");
    if (await page.evaluate(() => document.documentElement.scrollWidth > innerWidth + 1)) throw new Error("Charts overflow the window");
  }

  await navigate("Home");
  await manage();
  await page.getByTitle("Show the last 1h", { exact: true }).click();
  const counts = await sampleCounts();
  if (counts.some((count) => count !== 7)) throw new Error(`History was lost across navigation: ${counts}`);

  await page.clock.setSystemTime(epoch + 3_590_000);
  for (let sample = 0; sample < 1440; sample++) await page.clock.fastForward(2500);
  const denseCounts = await sampleCounts();
  if (denseCounts.some((count) => count < 1400 || count > 1441)) throw new Error(`Unexpected full-hour history: ${denseCounts}`);
  for (const range of ["30s", "1h", "5m", "30m", "10m"]) {
    const start = performance.now();
    await page.getByTitle(`Show the last ${range}`, { exact: true }).click();
    const currentPaths = await paths();
    if (currentPaths.length !== 2 || currentPaths.some((path) => !path || /NaN|Infinity/.test(path))) throw new Error(`Invalid dense chart for ${range}`);
    rangeTimes.push({ range: `${range} (full history)`, milliseconds: Math.round(performance.now() - start) });
  }
  const issues = await page.evaluate(() => ({ unknown: window.chartFixture.unknown, longestTask: Math.max(0, ...window.chartLongTasks) }));
  if (errors.length || issues.unknown.length) throw new Error([...errors, ...issues.unknown].join("\n"));
  if (issues.longestTask >= 500) throw new Error(`Chart interactions stalled for ${issues.longestTask}ms`);
  console.log(JSON.stringify({ rangeTimes, denseCounts, longestTask: issues.longestTask }));
  console.log("PASS: empty charts, all ranges, full-hour sampling, cached history, CPU/RAM colors, tooltips, both grid directions, timestamp visibility and non-overlap at desktop/minimum width.");
}
