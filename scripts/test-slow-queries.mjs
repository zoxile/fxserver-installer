import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import ts from "typescript";

const dataUrl = (source) => `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`;
const compile = (path) => ts.transpile(readFileSync(new URL(path, import.meta.url), "utf8"), { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 });
const moduleUrl = dataUrl(compile("../src/lib/modules/slowQueries.ts"));
const { SLOW_QUERY_LIMITS: limits, parseSlowQueryLog: parse, normalizeSlowQuery: normalize, slowQueryPage, slowQueryIndexAdvice } = await import(moduleUrl);
const entry = (query, time = "1.25", rows = "100", schema = "fixture") => `# Time: 260923 10:00:00\n# User@Host: private-user[private-user] @ private-host [127.0.0.1]\n# Thread_id: 1 Schema: ${schema} QC_hit: No\n# Query_time: ${time} Lock_time: 0.000001 Rows_sent: 1 Rows_examined: ${rows}\nSET timestamp=1790150400;\n${query}\n`;

test("groups by normalized literals and database, preserving identifier case", () => {
  const report = parse(entry("select * from Players where id=42;", "1.5", "100") + entry("SELECT * FROM Players WHERE id = 99;", "2.5", "300") + entry("SELECT * FROM players WHERE id = 1;") + entry("SELECT * FROM Players WHERE id = 2;", "0", "0", "other"));
  assert.equal(report.accepted, 4);
  assert.equal(report.groups.length, 3);
  const group = report.groups[0];
  assert.equal(group.fingerprint, "SELECT * FROM Players WHERE id = ?");
  assert.deepEqual([group.count, group.totalSeconds, group.averageSeconds, group.maxSeconds], [2, 4, 2, 2.5]);
  assert.deepEqual([group.rowsExamined, group.averageRowsExamined, group.maxRowsExamined], [400, 200, 300]);
  assert.equal(report.totalSeconds, 5.25);
  assert.equal(report.rowsExamined, 500);
  assert.ok(!JSON.stringify(report).includes("private-user"));
  assert.ok(!JSON.stringify(report).includes("private-host"));
});

test("redacts quoted, introduced, escaped, numeric, binary and hexadecimal literals", () => {
  for (const literal of ["'secret'", '"secret"', "'it''s secret'", String.raw`'it\'s secret'`, "_utf8mb4'secret'", "N'secret'", "X'534543524554'", "B'010101'", "0x534543524554", "0b010101", "1234.567", ".567", "1.25e-12", "TRUE", "FALSE"]) {
    const result = normalize(`SELECT * FROM users WHERE value=${literal};`);
    assert.equal(result?.fingerprint, "SELECT * FROM users WHERE value = ?", literal);
    assert.ok(!JSON.stringify(result).includes("secret"));
  }
  assert.equal(normalize("SELECT `mixed``case` FROM `Users` WHERE id=-12;")?.fingerprint, "SELECT `mixed``case` FROM `Users` WHERE id = - ?");
  assert.equal(normalize("SELECT a/* comment secret */FROM users -- secret\nWHERE id=9;")?.fingerprint, "SELECT a FROM users WHERE id = ?");
  assert.equal(normalize("SELECT * FROM t WHERE id IN (1,2,3);")?.fingerprint, "SELECT * FROM t WHERE id IN ( ? , ? , ? )");
});

test("multiline literals and comments cannot forge log record boundaries", () => {
  const fake = "SELECT 'secret\n# Time: fake\n# Query_time: 900 Lock_time: 0 Rows_sent: 0 Rows_examined: 999\n# Thread_id: 1 Schema: stolen QC_hit: No\nsecret' FROM users;";
  const report = parse(entry(fake, "2", "10") + entry("SELECT * FROM users;", "3", "20"));
  assert.equal(report.accepted, 2);
  assert.equal(report.totalSeconds, 5);
  assert.equal(report.rowsExamined, 30);
  assert.ok(!JSON.stringify(report).includes("secret"));
  assert.ok(!JSON.stringify(report).includes("stolen"));
  const comment = parse(entry("SELECT /*\n# Query_time: 9 Lock_time: 0 Rows_sent: 0 Rows_examined: 99\n*/ * FROM users;"));
  assert.equal(comment.groups[0].fingerprint, "SELECT * FROM users");
});

test("supports BOM, CRLF, verbose statistics, USE scope and multiple timestamp formats", () => {
  const log = "\uFEFF/usr/sbin/mariadbd, Version: fixture\nTcp port: 3306\nTime Id Command Argument\n" + entry("use `my``db`;\n# Full_scan: Yes Full_join: No\n# explain: secret diagnostics\nSET timestamp=1790150400.123;\nSELECT * FROM users;", "0.000001", "0", "") + entry("SELECT * FROM users;", "0.000002", "1", "other");
  const report = parse(log.replaceAll("\n", "\r\n"));
  assert.equal(report.accepted, 2);
  assert.equal(report.groups[0].database, "my`db");
  assert.equal(report.groups[0].maxSeconds, 0.000001);
  assert.equal(report.groups[1].database, "other");
  assert.ok(!JSON.stringify(report).includes("secret"));
  assert.equal(parse(entry("SELECT 1;", "1", "1", "")).groups[0].database, null);
});

test("malformed, nonfinite, negative and unsafe-integer metrics cannot poison totals", () => {
  const invalid = ["NaN", "Infinity", "-1", "1e999", "1e2", "1.2junk", "9".repeat(400), "9007199254740992"];
  for (const value of invalid) {
    const result = parse(entry("SELECT 1;", value) + entry("SELECT 2;"));
    assert.equal(result.accepted, 1, value);
    assert.equal(result.skipped, 1, value);
    assert.equal(result.totalSeconds, 1.25);
  }
  for (const rows of [...invalid, "1.25"]) assert.equal(parse(entry("SELECT 1;", "1", rows)).skipped, 1);
  const aggregate = parse(entry("SELECT 1;", "1", String(Number.MAX_SAFE_INTEGER)) + entry("SELECT 2;", "1", "1"));
  assert.equal(aggregate.accepted, 1);
  assert.equal(aggregate.skipped, 1);
  assert.equal(aggregate.rowsExamined, Number.MAX_SAFE_INTEGER);
});

test("malformed SQL, incomplete tails, executable comments and stacked statements are omitted", () => {
  for (const query of ["SELECT 'secret;", "SELECT /* incomplete", "SELECT `incomplete;", "SELECT 1", "SELECT 1; DROP TABLE users;", "SELECT 1;;", "SELECT /*!50000 SLEEP(9) */ 1;", "SELECT /*M! SLEEP(9) */ 1;", "SELECT 0xsecret;", "SELECT 1e-secret;", "SELECT \\!whoami;", "DELIMITER $$\nSELECT 1$$"]) {
    assert.equal(normalize(query), null, query);
    assert.equal(parse(entry(query)).accepted, 0, query);
  }
  assert.equal(parse(entry("SELECT 1") + entry("SELECT 2;")).accepted, 1);
  assert.deepEqual(parse("plain text secret"), { groups: [], accepted: 0, skipped: 0, truncated: false, totalSeconds: 0, rowsExamined: 0 });
});

test("EXPLAIN is review-only and excludes functions, writes, locks, output and variables", () => {
  for (const query of ["DELETE FROM users;", "SELECT SLEEP(1);", "SELECT BENCHMARK(1, 1);", "SELECT LOAD_FILE('/secret');", "SELECT evil_udf(1);", "SELECT db.evil_function(1);", "SELECT `evil_udf`(1);", "SELECT COUNT(*);", "SELECT 1 INTO OUTFILE '/tmp/file';", "SELECT 1 INTO DUMPFILE '/tmp/file';", "SELECT * FROM users FOR UPDATE;", "SELECT * FROM users LOCK IN SHARE MODE;", "SELECT @secret := 1;", "SELECT @@global.secret;", "SELECT * FROM users PROCEDURE evil();", "WITH q AS (SELECT 1) SELECT * FROM q;", "ANALYZE SELECT 1;", 'SELECT "sql mode";', String.raw`SELECT 'back\slash';`]) {
    const result = normalize(query);
    assert.ok(result, query);
    assert.equal(result.draft, null, query);
    assert.ok(result.draftReason.length > 10, query);
  }
  const result = normalize("SELECT * FROM users WHERE id=42;");
  assert.match(result.draft, /REVIEW ONLY/);
  assert.match(result.draft, /\nEXPLAIN\nSELECT \* FROM users WHERE id = \?;/);
  assert.ok(!result.draft.includes("42"));
  assert.ok(!result.draft.includes("ANALYZE"));
});

test("a group with an ambiguous quoted member loses its draft regardless of ordering", () => {
  for (const queries of [["SELECT 'value';", 'SELECT "value";'], ['SELECT "value";', "SELECT 'value';"]]) {
    const report = parse(queries.map((query) => entry(query)).join(""));
    assert.equal(report.groups.length, 1);
    assert.equal(report.groups[0].draft, null);
    assert.match(report.groups[0].draftReason, /SQL mode/);
  }
});

test("input bytes, records, groups, statements, tokens and fingerprints have hard limits", () => {
  assert.throws(() => parse(" ".repeat(limits.bytes + 1)), /4 MiB/);
  assert.throws(() => parse("\u00e9".repeat(limits.bytes / 2 + 1)), /4 MiB/);
  for (const bad of ["\0", "\u001b", "\u202e", "\u2066"]) assert.throws(() => parse(entry(`SELECT '${bad}';`)), /control characters/);
  assert.equal(normalize(`SELECT '${"s".repeat(limits.queryCharacters)}';`), null);
  assert.equal(normalize(`SELECT ${"?,".repeat(limits.tokens)}?;`), null);
  assert.equal(normalize(`SELECT ${"a".repeat(limits.fingerprintCharacters)} FROM t;`), null);
  const minimal = "# Query_time: 1 Lock_time: 0 Rows_sent: 1 Rows_examined: 0\nSELECT 1;\n";
  const records = parse(minimal.repeat(limits.records + 1));
  assert.equal(records.accepted, limits.records);
  assert.equal(records.truncated, true);
  const groups = parse(Array.from({ length: limits.groups + 1 }, (_, i) => entry(`SELECT * FROM t${i};`)).join(""));
  assert.equal(groups.groups.length, limits.groups);
  assert.equal(groups.skipped, 1);
  assert.equal(groups.truncated, true);
});

test("search is literal and bounded, sorting is stable, rendering is paginated without mutating the report", () => {
  const report = parse(Array.from({ length: 60 }, (_, i) => entry(`SELECT * FROM table_${i};`, String(i), String(i))).join(""));
  const first = slowQueryPage(report.groups, "", "totalSeconds", true, 0);
  assert.equal(first.groups.length, 25);
  assert.equal(first.groups[0].maxSeconds, 59);
  assert.equal(first.pages, 3);
  assert.equal(slowQueryPage(report.groups, "", "count", true, 0).groups[0].id, 1);
  assert.equal(slowQueryPage(report.groups, "", "rowsExamined", false, 1).groups[0].rowsExamined, 25);
  assert.equal(slowQueryPage(report.groups, "", "averageSeconds", true, 99).groups.length, 10);
  assert.equal(slowQueryPage(report.groups, "TABLE_59", "maxSeconds", true, 99).groups.length, 1);
  assert.equal(slowQueryPage(report.groups, ".*", "count", true, 0).total, 0);
  assert.equal(slowQueryPage(report.groups, "fixture", "count", true, 0).total, 60);
  assert.equal(report.groups[0].id, 1);
});

test("index advice is conditional and never produces DDL", () => {
  const group = parse(entry("SELECT * FROM users WHERE id=1 ORDER BY id;", "1", "10000")).groups[0];
  const advice = slowQueryIndexAdvice(group).join(" ");
  assert.match(advice, /High examined-row/);
  assert.match(advice, /composite index/);
  assert.match(advice, /grouping or ordering/);
  assert.match(advice, /does not prove an index is missing/);
  assert.match(advice, /migration review/);
  assert.doesNotMatch(advice, /CREATE INDEX|ALTER TABLE/);
});

let importCode = compile("../src/lib/modules/slowQueryImport.ts");
importCode = importCode.replace('"@tauri-apps/plugin-dialog"', JSON.stringify(dataUrl("export const open = (options) => globalThis.slowLogImport.open(options);")));
importCode = importCode.replace('"./system"', JSON.stringify(dataUrl("export const readTextFile = (...args) => globalThis.slowLogImport.read(...args);")));
importCode = importCode.replace('"./slowQueries"', JSON.stringify(moduleUrl));
const { isLocalSlowLogPath, readSelectedSlowLog } = await import(dataUrl(importCode));

test("dialog import accepts only a local log, enforces the backend byte limit and sanitizes failures", async () => {
  for (const path of ["C:\\logs\\slow.LOG", "D:/logs/slow.log"]) assert.equal(isLocalSlowLogPath(path), true);
  for (const path of ["slow.log", "../slow.log", "https://host/slow.log", "file:///C:/slow.log", "\\\\server\\slow.log", "\\\\?\\C:\\slow.log", "C:/slow.log:secret", "C:/slow.sql", "C:/slow.log\n", "C:/slow.log "]) assert.equal(isLocalSlowLogPath(path), false, path);
  const calls = [];
  globalThis.slowLogImport = { open: async (options) => { calls.push(options); return "C:/fixture/slow.log"; }, read: async (...args) => { calls.push(args); return "fixture"; } };
  assert.equal(await readSelectedSlowLog(() => true), "fixture");
  assert.deepEqual(calls[0].filters, [{ name: "Slow query logs", extensions: ["log"] }]);
  assert.deepEqual(calls[1], ["C:/fixture/slow.log", limits.bytes]);
  calls.length = 0;
  assert.equal(await readSelectedSlowLog(() => false), null);
  assert.equal(calls.length, 1);
  slowLogImport.open = async () => null;
  assert.equal(await readSelectedSlowLog(() => true), null);
  slowLogImport.open = async () => "\\\\host\\private.log";
  await assert.rejects(readSelectedSlowLog(() => true), /Choose a local/);
  slowLogImport.open = async () => "C:/fixture/slow.log";
  slowLogImport.read = async () => { throw new Error("private query and private path"); };
  await assert.rejects(readSelectedSlowLog(() => true), (error) => !error.message.includes("private") && /UTF-8/.test(error.message));
});

test("analyzer has no persistence or database execution channel", () => {
  const files = ["../src/lib/features/mariadb/SlowQueryAnalyzer.svelte", "../src/lib/features/mariadb/slowQuery.worker.ts", "../src/lib/modules/slowQueries.ts", "../src/lib/modules/slowQueryImport.ts"];
  const source = files.map((file) => readFileSync(new URL(file, import.meta.url), "utf8")).join("\n");
  assert.doesNotMatch(source, /localStorage|sessionStorage|indexedDB|console\.(?:log|error)|trackTask|executeMariaDBQuery|execute_mariadb_query|append_app_log|{@html/);
  assert.match(source, /worker\?\.terminate\(\)/);
  assert.match(source, /request === generation/);
});
