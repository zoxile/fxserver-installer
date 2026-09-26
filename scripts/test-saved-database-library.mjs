import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import ts from "typescript";

const url = (source) => `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`;
const compile = (path, imports = {}) => {
  let code = ts.transpile(readFileSync(new URL(`../src/lib/${path}.ts`, import.meta.url), "utf8"), { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 });
  for (const [name, target] of Object.entries(imports)) code = code.replaceAll(JSON.stringify(name), JSON.stringify(target));
  return url(code);
};
const parametersUrl = compile("features/mariadb/savedQueryParameters");
const libraryUrl = compile("features/mariadb/databaseLibrary", { "$lib/core/incidentModel": compile("core/incidentModel"), "./savedQueryParameters": parametersUrl });
const cacheUrl = compile("core/databaseBrowserCache");
const navigationUrl = compile("features/mariadb/databaseBookmarkNavigation", { "./databaseLibrary": libraryUrl, "$lib/core/databaseBrowserCache": cacheUrl });
const { queryParameters, prepareQueryDraft } = await import(parametersUrl);
const { DATABASE_LIBRARY_KEY, LIBRARY_LIMITS, containsLibrarySecret, emptyDatabaseLibrary, validateDatabaseLibrary, readDatabaseLibrary, writeDatabaseLibrary, resetDatabaseLibrary, databaseLibraryScope, bookmarkRequest } = await import(libraryUrl);
const { DatabaseBrowserCache, getDatabaseBrowserCache, invalidateDatabaseBrowserCommand } = await import(cacheUrl);
const { loadDatabaseBookmark } = await import(navigationUrl);
const scope = "a".repeat(64);
const query = (extra = {}) => ({ id: crypto.randomUUID(), scope, name: "Player lookup", folderId: null, database: "game", sql: "SELECT * FROM players WHERE id = {{id:number}}", ...extra });
const bookmark = (extra = {}) => ({ id: crypto.randomUUID(), scope, name: "Active players", database: "game", table: "players", view: "rows", filters: [{ column: "id", operator: "eq", value: "42" }], sortColumn: "id", descending: true, offset: 25, pageSize: 25, ...extra });
const storage = () => {
  const values = new Map();
  return { getItem: (key) => values.get(key) ?? null, setItem: (key, value) => values.set(key, value), removeItem: (key) => values.delete(key) };
};

test("typed parameters are deduplicated; literals cannot introduce SQL syntax", () => {
  const sql = "SELECT {{text:text}}, {{n:number}}, {{flag:boolean}}, {{nil:null}}, {{text:text}}";
  assert.deepEqual(queryParameters(sql), [{ name: "text", type: "text" }, { name: "n", type: "number" }, { name: "flag", type: "boolean" }, { name: "nil", type: "null" }]);
  for (const payload of ["'; DROP TABLE players; --", "\\'; SELECT 1; #", "`users`", "x\nGO\nDELETE FROM players", "{{n:number}}", "\" OR 1=1", "Zoë 日本 🟢", ""]) {
    const draft = prepareQueryDraft(sql, { text: payload, n: "-12.50", flag: "false" });
    assert.equal(draft.match(/CONVERT\(X'[a-f\d]*' USING utf8mb4\)/g).length, 2);
    const hex = draft.match(/X'([a-f\d]*)'/)[1];
    assert.equal(Buffer.from(hex, "hex").toString("utf8"), payload);
    assert.match(draft, / -12\.50 , \(FALSE\), \(NULL\)/);
    assert.ok(!draft.includes("DROP") && !draft.includes("DELETE"));
  }
});

test("numeric parameters are unwrapped literal tokens in LIMIT and OFFSET", () => {
  const values = { maximum: "20", offset: "40" };
  assert.equal(
    prepareQueryDraft("SELECT id FROM players LIMIT {{maximum:number}} OFFSET {{offset:number}};", values),
    "SELECT id FROM players LIMIT  20  OFFSET  40 ;",
  );
  assert.equal(
    prepareQueryDraft("SELECT id FROM players LIMIT {{offset:number}},{{maximum:number}};", values),
    "SELECT id FROM players LIMIT  40 , 20 ;",
  );
  assert.equal(prepareQueryDraft("SELECT id FROM players LIMIT {{maximum:number}};", { maximum: "0" }), "SELECT id FROM players LIMIT  0 ;");
  for (const value of ["20 OFFSET 0", "20; DROP TABLE players", "20--", "20/*x*/", "+20", " 20", "20 ", "2e1", "0x14", "NaN", "Infinity", "9".repeat(66)]) {
    assert.throws(() => prepareQueryDraft("SELECT id FROM players LIMIT {{maximum:number}};", { maximum: value }));
  }
  const largest = "9".repeat(65);
  assert.equal(prepareQueryDraft("SELECT {{n:number}};", { n: largest }), `SELECT  ${largest} ;`);
});

test("padded numeric tokens cannot merge with adjacent operators into comments", () => {
  for (const [sql, expected] of [
    ["SELECT 1-{{n:number}};", "SELECT 1- -20 ;"],
    ["SELECT -{{n:number}};", "SELECT - -20 ;"],
    ["SELECT {{n:number}}-1;", "SELECT  -20 -1;"],
    ["SELECT {{n:number}}-{{n:number}};", "SELECT  -20 - -20 ;"],
    ["SELECT 1+{{n:number}};", "SELECT 1+ -20 ;"],
    ["SELECT 1*{{n:number}};", "SELECT 1* -20 ;"],
    ["SELECT 1/{{n:number}};", "SELECT 1/ -20 ;"],
    ["SELECT {{n:number}}/-2;", "SELECT  -20 /-2;"],
  ]) {
    const draft = prepareQueryDraft(sql, { n: "-20" });
    assert.equal(draft, expected);
    assert.doesNotMatch(draft, /--|\/\*|\*\//);
  }
  assert.equal(prepareQueryDraft("SELECT {{n:number}}+{{n:number}};", { n: "-12.50" }), "SELECT  -12.50 + -12.50 ;");
  for (const sql of ["SELECT -{{n:number}}", "SELECT 1/{{n:number}}", "SELECT {{n:number}}-1"]) {
    for (const value of ["-20 -- comment", "-20\nDROP TABLE players", "-20/*", "--20", "- 20"]) assert.throws(() => prepareQueryDraft(sql, { n: value }));
  }
});

test("ambiguous syntax, identifier interpolation and invalid values fail closed", () => {
  for (const sql of ["SELECT '{{x:text}}'", 'SELECT "{{x:text}}"', "SELECT `{{x:text}}`", "SELECT x.{{x:text}}", "SELECT {{x:text}}suffix", "SELECT @{{x:text}}", "SELECT {{x}}", "SELECT {{x:identifier}}", "SELECT {{x:text}}, {{x:number}}", "SELECT {{x:text}} /* executable comment */", "SELECT {{x:text}} -- comment", "SELECT {{x:text}}, 'back\\slash'", "SELECT {{x:text}}, 'unclosed"]) {
    assert.throws(() => prepareQueryDraft(sql, { x: "42" }), undefined, sql);
  }
  for (const value of ["1 OR 1=1", "1;DROP TABLE x", "NaN", "Infinity", "1e3", "0x42", "01", "", "9".repeat(66)]) assert.throws(() => prepareQueryDraft("SELECT {{n:number}}", { n: value }));
  assert.throws(() => prepareQueryDraft("SELECT {{b:boolean}}", { b: "true;DELETE" }));
  assert.throws(() => prepareQueryDraft("SELECT {{x:text}}", {}));
  assert.throws(() => prepareQueryDraft("SELECT {{x:text}}", { x: "\0" }));
  assert.throws(() => prepareQueryDraft("SELECT {{x:text}}", { x: "x".repeat(4097) }));
  assert.throws(() => queryParameters(Array.from({ length: 17 }, (_, i) => `{{p${i}:text}}`).join(",")));
  assert.equal(prepareQueryDraft("SELECT 'literal'", {}), "SELECT 'literal'");
  assert.equal(prepareQueryDraft("SELECT {{constructor:number}}", { constructor: "42" }), "SELECT  42 ");
});

test("library round trip is allowlisted and contains no runtime credentials, parameters or results", () => {
  const local = storage();
  const folder = { id: crypto.randomUUID(), scope, name: "Daily reports" };
  const data = { ...emptyDatabaseLibrary(), folders: [folder], queries: [query({ folderId: folder.id })], bookmarks: [bookmark()] };
  writeDatabaseLibrary(local, data, ["fixture-private-login"]);
  assert.deepEqual(readDatabaseLibrary(local).library, data);
  for (const key of ["credentials", "password", "parameterValues", "results", "history", "editing"]) {
    assert.throws(() => writeDatabaseLibrary(local, { ...data, [key]: "must never persist" }));
    assert.throws(() => writeDatabaseLibrary(local, { ...data, queries: [{ ...data.queries[0], [key]: "must never persist" }] }));
  }
  assert.deepEqual(readDatabaseLibrary(local).library, data, "Rejected writes must not replace existing entries");
});

test("known passwords, URL credentials and secret formats are rejected in every saved field", () => {
  const secret = "can't-store\\this";
  for (const text of [secret, encodeURIComponent(secret), secret.replaceAll("'", "''"), secret.replaceAll("\\", "\\\\").replaceAll("'", "\\'"), `X'${Buffer.from(secret).toString("hex")}'`, `0x${Buffer.from(secret).toString("hex")}`, "mysql://admin:private@localhost/game", "api_key = 'a-private-value'", "Bearer abc123", "cfxk_private_license", "CREATE USER a IDENTIFIED BY 'new-password'", "ALTER USER a IDENTIFIED VIA ed25519 USING 'hash'", "SET PASSWORD = 'hash'", "-----BEGIN PRIVATE KEY-----\nprivate\n-----END PRIVATE KEY-----"]) {
    assert.ok(containsLibrarySecret(text, [secret]), text);
    for (const item of [query({ sql: `SELECT '${text}'` }), query({ name: text.slice(0, 80) }), query({ database: text })]) assert.equal(validateDatabaseLibrary({ ...emptyDatabaseLibrary(), queries: [item] }, [secret]), false);
    assert.equal(validateDatabaseLibrary({ ...emptyDatabaseLibrary(), bookmarks: [bookmark({ filters: [{ column: "id", operator: "eq", value: text }] })] }, [secret]), false);
  }
  for (const column of ["password", "api_token", "mysql_connection_string"]) assert.equal(validateDatabaseLibrary({ ...emptyDatabaseLibrary(), bookmarks: [bookmark({ filters: [{ column, operator: "eq", value: "unrecognized-secret" }] })] }), false);
  assert.equal(containsLibrarySecret("SELECT id FROM players WHERE id = {{id:number}}"), false);
});

test("corrupt, future, oversized, prototype-shaped and sensitive stored data remains intact and read-only", () => {
  const local = storage();
  for (const raw of ["", "{", "null", "[]", '{"version":2}', '{"__proto__":{"password":"private"}}', " ".repeat(LIBRARY_LIMITS.bytes + 1), JSON.stringify({ ...emptyDatabaseLibrary(), queries: [query({ sql: "SELECT 'fixture-private-login'" })] })]) {
    local.setItem(DATABASE_LIBRARY_KEY, raw);
    const result = readDatabaseLibrary(local, ["fixture-private-login"]);
    assert.deepEqual(result.library, emptyDatabaseLibrary());
    assert.equal(result.readOnly, true);
    assert.match(result.warning, /remains unchanged/);
    assert.equal(local.getItem(DATABASE_LIBRARY_KEY), raw);
    assert.throws(() => writeDatabaseLibrary(local, result.library, ["fixture-private-login"]), /read-only/);
    assert.throws(() => writeDatabaseLibrary(local, { ...emptyDatabaseLibrary(), queries: [query()] }, ["fixture-private-login"]), /read-only/);
    assert.equal(local.getItem(DATABASE_LIBRARY_KEY), raw, "Empty fallback and new edits must not overwrite unrecognized data");
  }
});

test("reset is explicit, confirmed, and limited to the database library key", () => {
  const local = storage();
  const raw = '{"version":99,"valuable":"existing content"}';
  local.setItem(DATABASE_LIBRARY_KEY, raw);
  local.setItem("unrelated-workspace-setting", "keep");
  for (const confirmation of ["", "reset", "NO", "RESET "]) {
    assert.throws(() => resetDatabaseLibrary(local, confirmation), /Type RESET/);
    assert.equal(local.getItem(DATABASE_LIBRARY_KEY), raw);
  }
  resetDatabaseLibrary(local, "RESET");
  assert.equal(local.getItem(DATABASE_LIBRARY_KEY), null);
  assert.equal(local.getItem("unrelated-workspace-setting"), "keep");
  assert.equal(readDatabaseLibrary(local).readOnly, false);
  writeDatabaseLibrary(local, { ...emptyDatabaseLibrary(), queries: [query()] });
  assert.equal(readDatabaseLibrary(local).library.queries.length, 1);
  local.removeItem = () => { throw new Error("adapter failure"); };
  assert.throws(() => resetDatabaseLibrary(local, "RESET"), /could not be reset/);
});

test("short passwords do not match arbitrary SQL/identifier substrings; credential SQL still fails closed", () => {
  for (const secret of ["a", "id", "42", "SELECT", "pass"]) {
    assert.equal(containsLibrarySecret("SELECT name, id FROM players WHERE id = 42", [secret]), false);
    assert.equal(containsLibrarySecret(secret, [secret]), true);
    assert.equal(containsLibrarySecret(`SELECT '${secret}'`, [secret]), true);
    assert.equal(containsLibrarySecret(`SELECT X'${Buffer.from(secret).toString("hex")}'`, [secret]), true);
    for (const sql of ["ALTER USER a IDENTIFIED BY 'a'", "SET PASSWORD = 'a'", "UPDATE users SET `password` = 'a'", "INSERT INTO users (password) VALUES ('a')"]) assert.equal(containsLibrarySecret(sql, [secret]), true);
  }
  const local = storage();
  const data = { ...emptyDatabaseLibrary(), queries: [query()] };
  writeDatabaseLibrary(local, data);
  const raw = local.getItem(DATABASE_LIBRARY_KEY);
  assert.equal(readDatabaseLibrary(local, ["a"]).readOnly, false, "A short rotated password must not invalidate unrelated SQL or opaque IDs");
  assert.equal(local.getItem(DATABASE_LIBRARY_KEY), raw);
  const rotated = { ...emptyDatabaseLibrary(), queries: [query({ sql: "SELECT 'future-login-password'" })] };
  writeDatabaseLibrary(local, rotated);
  const beforeRotation = local.getItem(DATABASE_LIBRARY_KEY);
  assert.equal(readDatabaseLibrary(local, ["future-login-password"]).readOnly, true);
  assert.equal(local.getItem(DATABASE_LIBRARY_KEY), beforeRotation, "Password rotation cannot silently discard existing work");
  assert.equal(readDatabaseLibrary(local).readOnly, false, "The untouched data must remain recoverable");
});

test("counts, bytes, duplicate names, folder references, filter enums and paging are bounded", () => {
  const local = storage();
  const tooMany = Array.from({ length: 101 }, (_, index) => query({ name: `Query ${index}` }));
  assert.throws(() => writeDatabaseLibrary(local, { ...emptyDatabaseLibrary(), queries: tooMany }));
  assert.throws(() => writeDatabaseLibrary(local, { ...emptyDatabaseLibrary(), bookmarks: Array.from({ length: 101 }, (_, index) => bookmark({ name: `Bookmark ${index}` })) }));
  assert.throws(() => writeDatabaseLibrary(local, { ...emptyDatabaseLibrary(), folders: Array.from({ length: 33 }, (_, index) => ({ id: crypto.randomUUID(), scope, name: `Folder ${index}` })) }));
  assert.throws(() => writeDatabaseLibrary(local, { ...emptyDatabaseLibrary(), queries: Array.from({ length: 40 }, (_, index) => query({ name: `Query ${index}`, sql: "SELECT '" + "a".repeat(16000) + "'" })) }), /512 KiB/);
  for (const extra of [{ folderId: crypto.randomUUID() }, { sql: "x".repeat(16_385) }, { name: " " }, { database: "" }]) assert.equal(validateDatabaseLibrary({ ...emptyDatabaseLibrary(), queries: [query(extra)] }), false);
  assert.equal(validateDatabaseLibrary({ ...emptyDatabaseLibrary(), queries: [query(), query({ name: "PLAYER LOOKUP" })] }), false);
  for (const extra of [{ offset: -1 }, { offset: 1_000_001 }, { offset: 0.5 }, { pageSize: 0 }, { pageSize: 201 }, { descending: "true" }, { view: "admin" }, { filters: [{ column: "id", operator: "rawSQL", value: "1 OR 1=1" }] }, { filters: [{ column: "id", operator: "eq", value: "x".repeat(513) }] }, { filters: [{ column: "id", operator: "isNull", value: "hidden-data" }] }, { filters: Array(9).fill({ column: "id", operator: "eq", value: "42" }) }]) assert.equal(validateDatabaseLibrary({ ...emptyDatabaseLibrary(), bookmarks: [bookmark(extra)] }), false);
});

test("storage failures do not report success or leak content", () => {
  const local = storage();
  local.setItem = () => { throw new Error("credential detail from adapter"); };
  assert.throws(() => writeDatabaseLibrary(local, { ...emptyDatabaseLibrary(), queries: [query()] }), /Storage may be full or disabled/);
});

test("scope partitions workspace, host, port and user without persisting or hashing password", async () => {
  const login = { host: "LOCALHOST", port: 3306, username: "fixture", password: "first" };
  const first = await databaseLibraryScope("workspace-a", login);
  assert.match(first, /^[a-f0-9]{64}$/);
  assert.equal(await databaseLibraryScope("workspace-a", { ...login, password: "rotated", host: "localhost" }), first);
  assert.notEqual(await databaseLibraryScope("workspace-b", login), first);
  for (const change of [{ host: "another.invalid" }, { port: 3307 }, { username: "another" }]) assert.notEqual(await databaseLibraryScope("workspace-a", { ...login, ...change }), first);
});

function loaders(cache = new DatabaseBrowserCache()) {
  const calls = [];
  return { scope, cache, current: () => true, calls,
    databases: async () => { calls.push("databases"); return ["game"]; },
    tables: async () => { calls.push("tables"); return ["players"]; },
    metadata: async () => { calls.push("metadata"); return { columns: [{ name: "id", columnType: "int", nullable: false, defaultValue: null, extra: "", binary: false }], indexes: [] }; },
    rows: async (request) => { calls.push("rows"); return { rows: [[request.filters[0]?.value ?? "all"]], hasMore: true, truncatedCells: false }; },
  };
}

test("bookmark restores exact applied state through shared cache; navigation and mutations stay correct", async () => {
  const load = loaders(getDatabaseBrowserCache({}));
  const saved = bookmark();
  const result = await loadDatabaseBookmark(saved, load);
  assert.deepEqual(result.request, bookmarkRequest(saved));
  assert.deepEqual(load.calls, ["databases", "tables", "metadata", "rows"]);
  load.cache.saveLocation({ ...result.request, view: result.view, draftFilters: result.request.filters });
  await loadDatabaseBookmark(saved, load);
  assert.equal(load.calls.length, 4, "Returning to the same bookmark should reuse the current cache");
  const copy = bookmarkRequest(saved); copy.filters[0].value = "changed";
  assert.equal(saved.filters[0].value, "42");
  assert.equal(load.cache.location().filters[0].value, "42");
  await loadDatabaseBookmark(bookmark({ offset: 50 }), load);
  assert.equal(load.calls.at(-1), "rows"); assert.equal(load.calls.length, 5);
  invalidateDatabaseBrowserCommand("execute_mariadb_query");
  await loadDatabaseBookmark(saved, load);
  assert.equal(load.calls.length, 9);
  assert.equal(load.cache.location().offset, 25, "Query invalidation must preserve navigation");
  const other = loaders(getDatabaseBrowserCache({}));
  await loadDatabaseBookmark(saved, other);
  assert.equal(other.calls.length, 4, "A replacement validated session cannot reuse old data");
});

test("missing database/table/columns and mismatched scope never silently broaden a bookmark", async () => {
  for (const extra of [{ database: "missing" }, { table: "missing" }, { sortColumn: "missing" }, { filters: [{ column: "missing", operator: "eq", value: "42" }] }, { scope: "b".repeat(64) }]) {
    const load = loaders();
    await assert.rejects(loadDatabaseBookmark(bookmark(extra), load));
    assert.ok(!load.calls.includes("rows"));
    assert.equal(load.cache.location(), undefined);
  }
  for (const view of ["columns", "indexes"]) {
    const load = loaders(); const result = await loadDatabaseBookmark(bookmark({ view }), load);
    assert.equal(result.view, view); assert.ok(!load.calls.includes("rows"));
  }
});

test("stale navigation is discarded at every asynchronous boundary", async () => {
  for (const stage of ["databases", "tables", "metadata", "rows"]) {
    let current = true;
    const load = loaders(); const original = load[stage];
    load.current = () => current;
    load[stage] = async (...args) => { const result = await original(...args); current = false; return result; };
    assert.equal(await loadDatabaseBookmark(bookmark(), load), null);
    assert.equal(load.calls.at(-1), stage);
    assert.equal(load.cache.location(), undefined);
  }
});

test("execution remains reviewed, never bypasses parent query path, and restores the template", () => {
  const source = readFileSync(new URL("../src/lib/features/mariadb/QueryConsole.svelte", import.meta.url), "utf8");
  assert.match(source, /confirmation !== "RUN"/);
  assert.match(source, /reviewCurrent/);
  assert.match(source, /selected\.session !== databaseSession\.validated/);
  assert.match(source, /await onExecute\(\)/);
  assert.match(source, /query = selected\.source/);
  assert.match(source, /parameterValues = \{\}/);
  assert.doesNotMatch(source, /<select\b/);
  assert.match(source, /items=\{booleanOptions\}/);
  assert.ok(!source.includes("invoke(") && !source.includes("executeMariaDBQuery(") && !source.includes("localStorage"));
});
