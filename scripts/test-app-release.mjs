import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import ts from "typescript";

const source = readFileSync(new URL("../src/lib/modules/appRelease.ts", import.meta.url), "utf8");
const compiled = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS, esModuleInterop: true } }).outputText;
const policy = JSON.parse(readFileSync(new URL("../release-policy.json", import.meta.url), "utf8"));
const fixture = (version = "0.3.2", prerelease = false) => ({ version, tagName: `v${version}`, prerelease,
  htmlUrl: `https://github.com/zoxile/fxserver-installer/releases/tag/v${version}`,
  installerUrl: `https://github.com/zoxile/fxserver-installer/releases/download/v${version}/FXServer.Installer_${version}_windows_x64-setup.exe` });

function load(current = "0.3.2", release = fixture(), native = true) {
  const calls = [];
  const module = { exports: {} };
  const require = (name) => {
    if (name.endsWith("release-policy.json")) return policy;
    if (name === "@tauri-apps/api/app") return { getVersion: async () => current };
    if (name === "@tauri-apps/api/core") return { invoke: async (...args) => { calls.push(args); if (release instanceof Error) throw release; return release; } };
    throw new Error(`Unexpected import: ${name}`);
  };
  new Function("require", "module", "exports", "window", "fetch", compiled)(require, module, module.exports,
    native ? { __TAURI_INTERNALS__: {} } : {}, () => { throw new Error("Must not fetch the main version manifest"); });
  return { ...module.exports, calls };
}

test("desktop requires a verified native release and preserves the HomePage API", async () => {
  const app = load();
  assert.deepEqual(await app.fetchLatestAppRelease(), fixture());
  assert.deepEqual(app.calls, [["fetch_latest_app_release", { force: false }]]);
  assert.equal(await app.getCurrentAppVersion(), "0.3.2");
  await app.fetchLatestAppRelease(true);
  assert.deepEqual(app.calls[1], ["fetch_latest_app_release", { force: true }]);
});

test("failed release lookup and browser previews never invent an installer", async () => {
  await assert.rejects(load("0.3.2", new Error("Release not published")).fetchLatestAppRelease, /not published/);
  const preview = load("dev", fixture(), false);
  await assert.rejects(preview.fetchLatestAppRelease, /desktop app/);
  assert.deepEqual(preview.calls, []);
  assert.equal(await preview.getCurrentAppVersion(), "dev");
});

test("numeric policy beta and GitHub prereleases are offered only to current beta users", async () => {
  assert.ok(policy.betaVersions.includes("0.4.0"));
  await assert.rejects(load("0.3.2", fixture("0.4.0")).fetchLatestAppRelease, /release channel/);
  await assert.rejects(load("0.3.2", fixture("0.5.0", true)).fetchLatestAppRelease, /release channel/);
  assert.equal((await load("0.4.0", fixture("0.5.0", true)).fetchLatestAppRelease()).version, "0.5.0");
  assert.equal((await load("0.4.0", fixture("0.5.0")).fetchLatestAppRelease()).version, "0.5.0");
});

test("canonical installer and release URLs cannot be replaced by arbitrary destinations", async () => {
  for (const changes of [{ installerUrl: "https://evil.test/setup.exe" }, { htmlUrl: "https://github.com/other/repo/releases/tag/v0.3.2" },
    { installerUrl: `${fixture().installerUrl}?redirect=evil` }, { tagName: "v0.4.0" }, { version: "0.3.2-beta" }]) {
    await assert.rejects(load("0.3.2", { ...fixture(), ...changes }).fetchLatestAppRelease, /verified/);
  }
  await assert.rejects(load("0.3.2", null).fetchLatestAppRelease, /verified/);
});

test("version comparison rejects partial and suffixed versions instead of advertising updates", () => {
  const app = load();
  assert.ok(app.compareVersions("0.4.0", "v0.3.2") > 0);
  assert.ok(app.compareVersions("0.3.10", "0.3.2") > 0);
  assert.equal(app.compareVersions("0.4.0-beta", "0.3.2"), 0);
  assert.equal(app.compareVersions("99.0.0", "Unknown"), 0);
  assert.equal(app.compareVersions("1.2.3extra", "0.3.2"), 0);
});
