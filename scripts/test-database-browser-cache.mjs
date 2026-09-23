import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import ts from "typescript";

const source = readFileSync(new URL("../src/lib/core/databaseBrowserCache.ts", import.meta.url), "utf8");
const code = ts.transpile(source, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 });
const { DatabaseBrowserCache, getDatabaseBrowserCache, resetDatabaseBrowserCache, invalidateDatabaseBrowserCommand } = await import(`data:text/javascript;base64,${Buffer.from(code).toString("base64")}`);
const location = (database = "game") => ({ database, table: "players", filters: [], draftFilters: [], view: "rows", sortColumn: "id", descending: true, offset: 25, pageSize: 25 });

test("recent reads are reused and concurrent navigation shares the pending request", async () => {
  const cache = new DatabaseBrowserCache();
  let finish, calls = 0;
  const load = () => { calls++; return new Promise((resolve) => { finish = resolve; }); };
  const first = cache.read(["rows", location()], load);
  assert.equal(first, cache.read(["rows", location()], load));
  await Promise.resolve();
  finish({ rows: [["first"]] });
  await first;
  assert.deepEqual(await cache.read(["rows", location()], load), { rows: [["first"]] });
  assert.equal(calls, 1);
});

test("expiry, failed reads, and distinct query shapes never reuse the wrong page", async () => {
  const cache = new DatabaseBrowserCache();
  let calls = 0;
  const load = async () => ++calls;
  await cache.read(["rows", location()], load, -1);
  assert.equal(await cache.read(["rows", location()], load), 2);
  assert.equal(await cache.read(["rows", { ...location(), offset: 50 }], load), 3);
  await assert.rejects(cache.read(["failure"], async () => { throw new Error("denied"); }), /denied/);
  assert.equal(await cache.read(["failure"], load), 4);
});

test("clear invalidates pending reads without discarding database navigation", async () => {
  const cache = new DatabaseBrowserCache();
  cache.saveLocation(location());
  let finish;
  const old = cache.read(["rows"], () => new Promise((resolve) => { finish = resolve; }));
  await Promise.resolve();
  cache.clear();
  assert.equal(await cache.read(["rows"], async () => "new"), "new");
  finish("old");
  await old;
  assert.equal(await cache.read(["rows"], async () => "wrong"), "new");
  assert.deepEqual(cache.location(), location());
});

test("LRU entry count, retained byte budget, oversized results and navigation are bounded", async () => {
  const cache = new DatabaseBrowserCache();
  let calls = 0;
  const load = async () => ++calls;
  for (let i = 0; i < 48; i++) await cache.read([i], load);
  await cache.read([0], load);
  await cache.read([48], load);
  assert.equal(await cache.read([0], load), 1, "Recently used entries should survive eviction");
  assert.equal(await cache.read([1], load), 50);
  cache.clear();
  await cache.read(["first"], async () => "x".repeat(3 * 1024 * 1024));
  await cache.read(["second"], async () => "y".repeat(3 * 1024 * 1024));
  assert.equal(await cache.read(["first"], async () => "evicted"), "evicted");
  await cache.read(["huge"], async () => "x".repeat(5 * 1024 * 1024));
  assert.equal(await cache.read(["huge"], async () => "uncached"), "uncached");
  for (let i = 0; i < 9; i++) cache.saveLocation(location(String(i)));
  assert.equal(cache.location("0"), undefined);
  assert.equal(cache.location().database, "8");
  const copy = cache.location();
  copy.offset = 200;
  assert.equal(cache.location().offset, 25);
});

test("login scope and session reset discard all cached results and navigation", async () => {
  resetDatabaseBrowserCache();
  const login = {};
  const first = getDatabaseBrowserCache(login);
  first.saveLocation(location());
  await first.read(["rows"], async () => "private");
  assert.equal(getDatabaseBrowserCache(login), first);
  const other = getDatabaseBrowserCache({});
  assert.notEqual(other, first);
  assert.equal(other.location(), undefined);
  assert.equal(await other.read(["rows"], async () => "other"), "other");
  resetDatabaseBrowserCache();
  assert.notEqual(getDatabaseBrowserCache(login), first);
});

test("database-changing commands invalidate results, previews and reads do not", async () => {
  const cache = getDatabaseBrowserCache({});
  let calls = 0;
  const read = () => cache.read(["rows"], async () => ++calls);
  await read();
  for (const command of ["get_database_browser_rows", "preview_database_admin_action", "backup_mariadb"]) {
    invalidateDatabaseBrowserCommand(command);
    assert.equal(await read(), 1);
  }
  for (const command of ["execute_mariadb_query", "apply_database_browser_change", "apply_database_admin_action", "restore_backup_snapshot", "update_mariadb_user", "restart_mariadb_service"]) {
    const before = calls;
    invalidateDatabaseBrowserCommand(command);
    assert.equal(await read(), before + 1);
  }
});
