import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";
import ts from "typescript";

function load(relativePath, dependencies = {}, globals = {}) {
  const source = readFileSync(new URL(relativePath, import.meta.url), "utf8");
  const code = ts.transpile(source, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS });
  const exports = {};
  vm.runInNewContext(code, {
    exports, $state: (value) => value, Event, Date, console,
    require: (name) => {
      assert.ok(name in dependencies, `Unexpected dependency: ${name}`);
      return dependencies[name];
    },
    ...globals,
  });
  return exports;
}

const linuxInfo = { os: "linux", arch: "x86_64", artifactPlatform: "linux", clientLogsSupported: false, secureStorageSupported: false };
const windowsInfo = { os: "windows", arch: "x86_64", artifactPlatform: "windows", clientLogsSupported: true, secureStorageSupported: true };

function loadPlatform({ native = false, info = linuxInfo, navigatorPlatform = "Linux x86_64", reject = false } = {}) {
  const calls = [];
  const platform = load("../lib/core/platform.svelte.ts", {
    "@tauri-apps/api/core": { invoke: async (command) => { calls.push(command); if (reject) throw new Error("Unavailable"); return info; } },
  }, { window: native ? { __TAURI_INTERNALS__: {} } : {}, navigator: { platform: navigatorPlatform, userAgent: "fixture" } });
  return { ...platform, calls };
}

test("browser platform fallback never invokes native probes", async () => {
  const fixture = loadPlatform();
  await fixture.initializePlatform();
  assert.equal(fixture.platform.os, "linux");
  assert.equal(fixture.platform.secureStorageSupported, false);
  assert.equal(fixture.platform.clientLogsSupported, false);
  assert.deepEqual(fixture.calls, []);
  assert.equal(fixture.serverPathPlaceholder(), "/home/fivem/server");
  assert.equal(fixture.txDataPathPlaceholder(), "/home/fivem/txData");
  assert.match(fixture.artifactListingUrl("enhanced"), /platform=enhanced&os=linux$/);
});

test("native platform is fetched once and overrides the browser OS", async () => {
  const fixture = loadPlatform({ native: true, navigatorPlatform: "Win32" });
  const first = fixture.initializePlatform();
  assert.equal(fixture.initializePlatform(), first);
  await first;
  await fixture.initializePlatform();
  assert.deepEqual(fixture.calls, ["get_platform_info"]);
  assert.equal(fixture.platform.os, "linux");
  assert.equal(fixture.platformLabel(), "Linux");
});

test("failed or malformed native responses keep the navigator fallback", async () => {
  for (const options of [{ reject: true }, { info: null }, { info: { os: "linux" } }]) {
    const fixture = loadPlatform({ native: true, navigatorPlatform: "Win32", ...options });
    await fixture.initializePlatform();
    await fixture.initializePlatform();
    assert.equal(fixture.platform.os, "windows");
    assert.equal(fixture.serverPathPlaceholder(), "C:\\FXServer\\server");
    assert.equal(fixture.calls.length, 1);
  }
});

function loadArtifacts(info, metadata) {
  const calls = [];
  const invoke = async (command, args) => {
    calls.push({ command, args });
    return command === "get_windows_artifact_metadata" ? metadata : { version: "12345", destination: "/home/fivem/server" };
  };
  const artifact = load("../lib/modules/artifact.ts", {
    "$lib/core/tasks.svelte": { taskInvoke: invoke, trackTask: (_command, _label, action) => action() },
    "@tauri-apps/api/core": { invoke },
    "$lib/core/paths.svelte": { getInstallPath: () => "/home/fivem/server" },
    "$lib/core/logger.svelte": { log: () => {} },
    "$lib/core/platform.svelte": { initializePlatform: async () => info, platform: info, platformLabel: () => info.os === "linux" ? "Linux" : "Windows" },
  }, { window: { __TAURI_INTERNALS__: {} } });
  return { artifact, calls };
}

test("Linux metadata and installs use only the Linux URL with legacy IPC names", async () => {
  const url = "https://runtime.fivem.net/artifacts/fivem/build_proot_linux/master/12345-fixture/fx.tar.xz";
  const { artifact, calls } = loadArtifacts(linuxInfo, { recommendedArtifact: "12345", linuxDownloadLink: url, windowsDownloadLink: "https://example.invalid/windows.zip", brokenArtifacts: [] });
  const metadata = await artifact.fetchArtifactMetadata();
  assert.equal(metadata.downloadLink, url);
  await artifact.installWindowsArtifact(metadata);
  assert.equal(calls[1].command, "install_windows_artifact");
  assert.equal(calls[1].args.request.url, url);
  assert.match(artifact.getArtifactHealthStatus(metadata, null).description, /Linux/);
});

test("a missing Linux URL cannot fall back to a Windows artifact", async () => {
  const { artifact, calls } = loadArtifacts(linuxInfo, { recommendedArtifact: "12345", windowsDownloadLink: "https://example.invalid/windows.zip", brokenArtifacts: [] });
  await assert.rejects(artifact.fetchArtifactMetadata(), /linuxDownloadLink/);
  assert.equal(calls.length, 1);
});

test("Windows metadata does not require a Linux URL", async () => {
  const url = "https://runtime.fivem.net/artifacts/fivem/build_server_windows/master/12345-fixture/server.7z";
  const { artifact } = loadArtifacts(windowsInfo, { recommendedArtifact: "12345", windowsDownloadLink: url, brokenArtifacts: [] });
  const metadata = await artifact.fetchArtifactMetadata();
  assert.equal(metadata.downloadLink, url);
});

test("slow log imports accept POSIX and Windows local paths without accepting network or device paths", () => {
  const { isLocalSlowLogPath } = load("../lib/modules/slowQueryImport.ts", {
    "@tauri-apps/plugin-dialog": {}, "./system": {}, "./slowQueries": {},
  });
  for (const path of ["/home/fivem/slow.log", "/var/log/mysql/slow.LOG", "/home/my server/query:slow.log", "C:\\logs\\slow.log", "D:/logs/slow.log"]) {
    assert.equal(isLocalSlowLogPath(path), true, path);
  }
  for (const path of ["slow.log", "~/slow.log", "../slow.log", "https://example.org/slow.log", "file:///home/fivem/slow.log", "//host/slow.log", "\\\\host\\slow.log", "\\\\?\\C:\\slow.log", "C:/slow.log:secret", "/home/slow.log\0", "/home/slow.log\n", "/home/slow.sql"]) {
    assert.equal(isLocalSlowLogPath(path), false, path);
  }
});

test("unsupported secure storage performs no save, restore, or forget IPC", async () => {
  const calls = [];
  const { databaseSession, restoreDatabaseLogin, setRememberDatabaseLogin, forgetDatabaseLogin } = load("../lib/core/databaseSession.svelte.ts", {
    "@tauri-apps/api/core": { invoke: async (...args) => { calls.push(args); } },
    "$lib/modules/mariadb": { validateMariaDBCredentials: async () => {} },
    "./databaseBrowserCache": { resetDatabaseBrowserCache: () => {} },
    "./platform.svelte": { initializePlatform: async () => linuxInfo, platform: linuxInfo },
  }, { window: Object.assign(new EventTarget(), { __TAURI_INTERNALS__: {} }) });
  await restoreDatabaseLogin("fixture");
  setRememberDatabaseLogin(true);
  setRememberDatabaseLogin(false);
  await forgetDatabaseLogin("fixture");
  assert.equal(databaseSession.rememberLogin, false);
  assert.equal(databaseSession.restoringLogin, false);
  assert.deepEqual(calls, []);
});

test("unsupported client logs do not invoke the backend", async () => {
  const { readClientLogs } = load("../lib/modules/clientLogs.ts", {
    "@tauri-apps/api/core": { invoke: () => assert.fail("Unsupported client log IPC") },
    "$lib/core/platform.svelte": { platform: linuxInfo },
  }, { window: { __TAURI_INTERNALS__: {} } });
  await assert.rejects(readClientLogs({}), /unavailable/);
});
