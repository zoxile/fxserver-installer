import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import ts from "typescript";
import { compileModule } from "svelte/compiler";

const dataUrl = (code) => `data:text/javascript;base64,${Buffer.from(code).toString("base64")}`;
const transport = dataUrl("export const invoke = (...args) => globalThis.loginFixture.invoke(...args);");
const mariadb = dataUrl("export const validateMariaDBCredentials = (credentials) => globalThis.loginFixture.invoke('validate_mariadb_credentials', { credentials });");
const source = readFileSync(new URL("../src/lib/core/databaseSession.svelte.ts", import.meta.url), "utf8");
let code = ts.transpile(source, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 })
  .replace('"@tauri-apps/api/core"', JSON.stringify(transport))
  .replace('"$lib/modules/mariadb"', JSON.stringify(mariadb));
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
  session.databaseSession.rememberLogin = false;
  await session.ensureDatabaseSession(credentials);
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
  assert.equal(session.isDatabaseSessionValidated(credentials), false, "Restored passwords must be checked once per session");
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

test("database tabs share one validation, including concurrent mounts and changed database scope", async () => {
  session.invalidateDatabaseSession();
  session.databaseSession.rememberLogin = false;
  calls.length = 0;
  let finish;
  loginFixture.invoke = async (command, args) => {
    calls.push({ command, args });
    return new Promise((resolve) => { finish = resolve; });
  };
  const first = session.ensureDatabaseSession(credentials);
  const second = session.ensureDatabaseSession({ ...credentials, database: "other" });
  assert.equal(first, second, "Concurrent tab mounts must share the pending request");
  assert.equal(calls.length, 1);
  assert.equal(calls[0].args.credentials.database, null, "Authentication must not depend on a default database");
  finish();
  assert.equal(await first, true);
  assert.equal(session.isDatabaseSessionValidated({ ...credentials, database: "other" }), true);
  assert.equal(await session.ensureDatabaseSession(credentials), true);
  assert.equal(calls.length, 1);
  assert.equal(session.isDatabaseSessionValidated({ ...credentials, password: "edited" }), false);
  assert.equal(session.databaseSession.validating, false);
});

test("explicit failed rechecks and connection errors invalidate validation, SQL errors do not", async () => {
  loginFixture.invoke = async () => { throw new Error("ERROR 1045 (28000): Access denied"); };
  await assert.rejects(session.ensureDatabaseSession(credentials, true), /1045/);
  assert.equal(session.isDatabaseSessionValidated(credentials), false);
  assert.equal(session.databaseSession.validating, false);
  loginFixture.invoke = async () => null;
  await session.ensureDatabaseSession(credentials);
  session.handleDatabaseConnectionError(credentials, "ERROR 1142 (42000): SELECT command denied");
  assert.equal(session.isDatabaseSessionValidated(credentials), true);
  session.handleDatabaseConnectionError({ ...credentials, username: "other" }, "ERROR 2003: Connection refused");
  assert.equal(session.isDatabaseSessionValidated(credentials), true, "Another login must not invalidate this session");
  session.handleDatabaseConnectionError(credentials, "ERROR 2003: Connection refused");
  assert.equal(session.isDatabaseSessionValidated(credentials), false);
});

test("workspace switches and invalidation reject in-flight authentication results", async () => {
  let finish;
  loginFixture.invoke = () => new Promise((resolve) => { finish = resolve; });
  const stale = session.ensureDatabaseSession(credentials);
  session.databaseSession.revision++;
  session.invalidateDatabaseSession();
  session.databaseSession.credentials = null;
  finish();
  assert.equal(await stale, false);
  assert.equal(session.databaseSession.credentials, null);
  assert.equal(session.databaseSession.validated, null);
  assert.equal(session.databaseSession.validating, false);
});

test("remembered logins are saved once after validation, not rewritten on tab navigation", async () => {
  calls.length = 0;
  loginFixture.invoke = async (command, args) => { calls.push({ command, args }); return null; };
  session.setRememberDatabaseLogin(true, credentials);
  await session.ensureDatabaseSession(credentials);
  await settle();
  await session.ensureDatabaseSession(credentials);
  await session.ensureDatabaseSession({ ...credentials, database: "other" });
  await session.ensureDatabaseSession(credentials, true);
  await settle();
  assert.equal(calls.filter(({ command }) => command === "save_database_login").length, 1);
  assert.equal(calls.filter(({ command }) => command === "validate_mariadb_credentials").length, 2);
});
