import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import ts from "typescript";
import { compileModule } from "svelte/compiler";

const dataUrl = (code) => `data:text/javascript;base64,${Buffer.from(code).toString("base64")}`;
const transport = dataUrl("export const invoke = (...args) => globalThis.loginFixture.invoke(...args);");
const source = readFileSync(new URL("../src/lib/core/databaseSession.svelte.ts", import.meta.url), "utf8");
let code = ts.transpile(source, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 }).replace('"@tauri-apps/api/core"', JSON.stringify(transport));
code = compileModule(code, { filename: "databaseSession.svelte.ts", generate: "client" }).js.code.replace(/(from\s+)["'](svelte\/[^"']+)["']/g, (_, prefix, name) => `${prefix}${JSON.stringify(import.meta.resolve(name))}`);
globalThis.window = new EventTarget();
window.__TAURI_INTERNALS__ = {};
const calls = [];
globalThis.loginFixture = { invoke: async (command, args) => { calls.push({ command, args }); return null; } };
const session = await import(dataUrl(code));
const credentials = { host: "localhost", port: 3306, username: "fixture", password: "test-secret", database: "fixture" };
const settle = () => new Promise((resolve) => setImmediate(resolve));

test("saving is opt-in and stale workspace validation cannot persist", async () => {
  session.databaseSession.revision = 1;
  session.rememberDatabaseCredentials(credentials);
  await settle();
  assert.equal(calls.length, 0);
  assert.equal(session.rememberDatabaseCredentials(credentials, 0), false);
  session.setRememberDatabaseLogin(true, { ...credentials, password: "unvalidated" });
  await settle();
  assert.equal(calls.length, 0, "Unvalidated edits must not be written");
});

test("forget waits for in-flight save and cannot resurrect a password", async () => {
  let finish;
  calls.length = 0;
  loginFixture.invoke = async (command, args) => {
    calls.push({ command, args });
    if (command === "save_database_login") await new Promise((resolve) => { finish = resolve; });
  };
  session.setRememberDatabaseLogin(true, credentials);
  await settle();
  session.setRememberDatabaseLogin(false);
  await settle();
  assert.deepEqual(calls.map((entry) => entry.command), ["save_database_login"]);
  finish();
  await settle();
  assert.deepEqual(calls.map((entry) => entry.command), ["save_database_login", "clear_database_login"]);
  assert.equal(session.databaseSession.savingLogin, false);
});

test("restoration is workspace-bound and never overwrites a newer session", async () => {
  let finish;
  loginFixture.invoke = () => new Promise((resolve) => { finish = resolve; });
  const pending = session.restoreDatabaseLogin("old");
  await settle();
  session.databaseSession.revision++;
  session.databaseSession.credentials = null;
  finish(credentials);
  await pending;
  assert.equal(session.databaseSession.credentials, null);
  loginFixture.invoke = async () => credentials;
  await session.restoreDatabaseLogin("new");
  assert.equal(session.databaseSession.credentials.password, "test-secret");
  assert.equal(session.databaseSession.rememberLogin, true);
  assert.equal(session.databaseSession.restoringLogin, false);
});

test("storage failures are visible without exposing backend secrets", async () => {
  loginFixture.invoke = async () => { throw new Error("sensitive backend text"); };
  session.setRememberDatabaseLogin(false);
  await settle();
  assert.match(session.databaseSession.loginError, /Could not forget/);
  assert.ok(!session.databaseSession.loginError.includes("sensitive"));
  await session.restoreDatabaseLogin("new");
  assert.match(session.databaseSession.loginError, /could not be unlocked/);
  assert.equal(session.databaseSession.restoringLogin, false);
});

test("connection strings escape secrets and bracket IPv6", () => {
  assert.equal(session.formatMariaDBConnectionString({ ...credentials, host: "::1", password: "a@:/" }), "mysql://fixture:a%40%3A%2F@[::1]:3306/fixture");
});
