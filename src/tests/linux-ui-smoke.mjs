import assert from "node:assert/strict";
import { mkdir } from "node:fs/promises";
import { chromium } from "playwright";

const baseUrl = process.env.UI_BASE_URL || "http://127.0.0.1:5176";
const output = "output/playwright";
await mkdir(output, { recursive: true });
const browser = await chromium.launch({ headless: true });
try {
  for (const os of ["linux", "windows"]) {
    const page = await browser.newPage({ viewport: { width: 1440, height: 1000 } });
    const errors = [];
    page.on("pageerror", (error) => errors.push(error.message));
    await page.addInitScript((os) => {
      localStorage.clear();
      localStorage.setItem("installPath", os === "linux" ? "/home/fivem/server" : "C:/fixture/server");
      // Deliberately disagree with the native Linux response to test its authority.
      Object.defineProperty(navigator, "platform", { value: "Win32" });
      const state = window.platformFixture = { calls: [], nextId: 0 };
      window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
      window.__TAURI_INTERNALS__ = {
        metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
        transformCallback: () => ++state.nextId,
        invoke: async (command, args = {}) => {
          state.calls.push({ command, args });
          switch (command) {
            case "get_platform_info": return { os, arch: "x86_64", artifactPlatform: os, clientLogsSupported: os === "windows", secureStorageSupported: os === "windows" };
            case "plugin:window|title": return "FXServer Installer";
            case "plugin:app|version": return "0.5.2";
            case "plugin:event|listen": return ++state.nextId;
            case "read_app_logs": return { path: "fixture.log", entries: [] };
            case "get_windows_artifact_metadata": return { recommendedArtifact: "12345", windowsDownloadLink: "https://fixture.invalid/windows.7z", linuxDownloadLink: "https://fixture.invalid/linux.tar.xz", brokenArtifacts: [] };
            case "get_windows_artifact_catalog": return { builds: [], fetchedAt: 0, stale: false, warning: null };
            case "get_installed_windows_artifact_info": return { installed: false, destination: args.destination, hasFxserverExecutable: false, detectionSource: "none" };
            case "get_enhanced_artifact_catalog": throw new Error(`No official Enhanced ${os} listing is available.`);
            case "get_mariadb_status": return { installed: false, running: false };
            case "get_mariadb_package_info": return { latestVersion: "10.11.6", updateAvailable: false };
            case "list_mariadb_series": return [{ series: os === "linux" ? "10.11" : "11.4", support: os === "linux" ? "Distribution repository" : "LTS", eol: os === "linux" ? "" : "2029-05-29" }];
            case "list_mariadb_releases": return [{ version: os === "linux" ? "1:10.11.6-1" : "11.4.10", date: "" }];
            case "get_backup_manager": return { schedules: [], snapshots: [], restoreTests: [], busy: false };
            case "configure_live_bridge": return { workspaceId: "default", enabled: false, connected: false, snapshot: null };
            case "read_client_logs": return { directory: "C:/FiveM/logs", files: [], content: "", lineCount: 0 };
            default: return null;
          }
        },
      };
    }, os);
    await page.goto(baseUrl, { waitUntil: "domcontentloaded" });
    const nav = page.getByRole("navigation", { name: "Workspace navigation" });
    await nav.waitFor();
    const capture = async (name) => {
      await page.waitForTimeout(350);
      await page.screenshot({ path: `${output}/${os}-${name}.png` });
    };
    const navigate = async (parent, child) => {
      const toggle = nav.getByTitle(parent, { exact: true });
      if (await toggle.getAttribute("aria-expanded") !== "true") await toggle.click();
      await nav.getByTitle(child, { exact: true }).click();
    };
    if (os === "linux") {
      await page.getByRole("button", { name: "Releases", exact: true }).waitFor();
      assert.equal(await page.getByRole("button", { name: "Installer", exact: true }).count(), 0);
    }
    await navigate("Artifacts", "Install Artifact");
    await page.getByRole("button", { name: `Install ${os === "linux" ? "Linux" : "Windows"} Artifact`, exact: true }).waitFor();
    await page.getByText(`https://fixture.invalid/${os === "linux" ? "linux.tar.xz" : "windows.7z"}`, { exact: true }).waitFor();
    await capture("artifacts-desktop");
    await page.getByRole("button", { name: "Enhanced", exact: true }).click();
    await page.getByText(`No official Enhanced ${os} listing is available.`, { exact: false }).waitFor();
    assert.equal(await page.getByRole("button", { name: "Install Enhanced Server", exact: true }).isDisabled(), true);
    assert.equal(await page.getByLabel("Official Download URL Override").count(), os === "windows" ? 1 : 0);
    await capture("enhanced-desktop");
    await navigate("MariaDB", "Manage MariaDB");
    await page.getByRole("heading", { name: "Manage MariaDB", exact: true }).waitFor();
    await page.getByText("Install Configuration", { exact: true }).waitFor();
    assert.equal(await page.getByRole("checkbox", { name: "Remember login for this workspace" }).isDisabled(), os === "linux");
    assert.equal(await page.getByText("Root Password", { exact: true }).count(), os === "windows" ? 1 : 0);
    assert.equal(await page.getByText("HeidiSQL", { exact: true }).count(), os === "windows" ? 1 : 0);
    await capture("mariadb-desktop");
    assert.equal(await page.getByRole("button", { name: "Install", exact: true }).isDisabled(), false);
    await navigate("Logs", "Client Logs");
    await page.getByRole("heading", { name: "Client Logs", exact: true }).waitFor();
    if (os === "linux") await page.getByText(/client log discovery is unavailable/).waitFor();
    await page.waitForTimeout(1700);
    const calls = await page.evaluate(() => window.platformFixture.calls.map((call) => call.command));
    assert.equal(calls.filter((command) => command === "get_platform_info").length, 1);
    if (os === "linux") {
      assert.ok(!calls.includes("read_client_logs"));
      assert.ok(!calls.includes("load_database_login"));
      assert.ok(!calls.includes("fetch_latest_app_release"));
    }
    await capture("client-logs-desktop");
    await navigate("Artifacts", "Install Artifact");
    await page.setViewportSize({ width: 900, height: 600 });
    await capture("artifacts-minimum");
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth + 1), false);
    assert.deepEqual(errors, []);
    await page.close();
    console.log(`${os}: native platform authority, artifact URLs, Enhanced gating, installer options, storage, client logs, and minimum desktop width passed.`);
  }
} finally {
  await browser.close();
}
