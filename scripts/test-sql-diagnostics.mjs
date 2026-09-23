import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import ts from "typescript";

const dataUrl = (source) => `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`;
let code = ts.transpile(readFileSync(new URL("../src/lib/modules/sqlDiagnostics.ts", import.meta.url), "utf8"), { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 });
code = code.replace('"@tauri-apps/api/core"', JSON.stringify(dataUrl("export const invoke = (...args) => globalThis.sqlDiagnosticCalls.push(args);")));
code = code.replace('"$lib/core/tasks.svelte"', JSON.stringify(dataUrl("export const trackTask = (_id, _name, run) => run();")));
const { diagnoseSqlError, inspectDatabaseSql, sqlInspectionAvailability, summarizeSqlInspection } = await import(dataUrl(code));
globalThis.sqlDiagnosticCalls = [];

test("errors produce bounded review-only advice, never SQL execution", () => {
  for (const error of ["ERROR 1005 (HY000): cannot create table", "ERROR 1267 (HY000): illegal mix", "ERROR 1452: constraint", "ERROR 1213: deadlock", "ERROR 2013: lost connection"]) {
    const result = diagnoseSqlError(error);
    assert.ok(result.checks.length >= 2);
    assert.ok(result.explanation.length > 30);
    assert.ok(!result.title.includes("Review the SQL error"));
  }
  assert.equal(sqlDiagnosticCalls.length, 0);
  assert.equal(diagnoseSqlError("password=this-is-not-an-error").title, "Review the SQL error");
  assert.equal(diagnoseSqlError("x".repeat(32768) + " ERROR 1267").title, "Review the SQL error");
  assert.match(diagnoseSqlError("unknown failure").checks.join(" "), /partially applied/);
});

test("guidance explains limits and does not treat inspection settings as the failed session", () => {
  const collation = diagnoseSqlError("ERROR 1267: incompatible");
  assert.match(collation.explanation, /compares or combines text/);
  assert.match(collation.checks.join(" "), /own connection/);
  assert.match(collation.checks.join(" "), /not automatically a problem/);
  assert.match(diagnoseSqlError("ERROR 1273: unsupported").checks.join(" "), /does not list every supported option/);
  assert.match(diagnoseSqlError("ERROR 1062: duplicate").checks.join(" "), /does not show row data or unique indexes/);
  assert.match(diagnoseSqlError("ERROR 1064: syntax").explanation, /cannot validate SQL syntax/);
  assert.match(diagnoseSqlError("unrecognized").explanation, /No recognized MariaDB error code/);
  assert.equal(sqlDiagnosticCalls.length, 0);
});

test("unvalidated, busy, global and missing database states have distinct next steps", () => {
  assert.equal(sqlInspectionAvailability("fixture", false).state, "unvalidated");
  assert.match(sqlInspectionAvailability("fixture", false).message, /Validate the connection above/);
  assert.equal(sqlInspectionAvailability("fixture", true, true).state, "busy");
  assert.equal(sqlInspectionAvailability("__global__", true).state, "global");
  assert.match(sqlInspectionAvailability("__global__", true).message, /USE statement/);
  assert.equal(sqlInspectionAvailability("  ", true).state, "no-database");
  assert.deepEqual(sqlInspectionAvailability("fixture", true), { state: "ready", message: "" });
});

const environment = { version: "fixture", sqlMode: "", charset: "utf8mb4", collation: "utf8mb4_general_ci", schemaCollation: null };
const emptyReport = { environment, columns: [], foreignKeys: [], truncated: false };
const column = { table: "players", column: "identifier", columnType: "varchar(64)", charset: "utf8mb4", collation: "utf8mb4_unicode_ci", engine: "InnoDB" };

test("empty inspection distinguishes table lookup from a database with no visible tables", () => {
  const all = summarizeSqlInspection(emptyReport, "  ");
  assert.equal(all.title, "No visible table columns found");
  assert.equal(all.description, "0 visible columns across 0 tables.");
  assert.match(all.nextStep, /empty or its tables may be hidden/);
  const named = summarizeSqlInspection(emptyReport, "missing");
  assert.match(named.nextStep, /exact table name/);
  assert.match(named.nextStep, /clear the optional table field/);
  assert.match(named.nextStep, /view/);
});

test("inspection summary counts visible tables and deduplicates metadata without declaring SQL valid", () => {
  const summary = summarizeSqlInspection({ ...emptyReport, columns: [column, { ...column, column: "name" }, { ...column, table: "jobs", charset: null, collation: null }] }, "");
  assert.equal(summary.title, "Schema inspection complete");
  assert.equal(summary.description, "3 visible columns across 2 tables.");
  assert.deepEqual(summary.charsets, ["utf8mb4"]);
  assert.deepEqual(summary.collations, ["utf8mb4_unicode_ci"]);
  assert.match(summary.nextStep, /not a pass\/fail test/);
  assert.equal(summarizeSqlInspection({ ...emptyReport, columns: [column] }, "players").description, "1 visible column across 1 table.");
});

test("truncated and non-text results cannot look like exhaustive compatibility checks", () => {
  const summary = summarizeSqlInspection({ ...emptyReport, columns: [{ ...column, charset: null, collation: null }], truncated: true }, "players");
  assert.equal(summary.title, "Partial schema inspection");
  assert.match(summary.description, /in this limited result/);
  assert.deepEqual(summary.charsets, []);
  assert.deepEqual(summary.collations, []);
});

test("inspection calls only the dedicated read-only backend", async () => {
  sqlDiagnosticCalls.length = 0;
  const credentials = { host: "localhost", username: "fixture", password: "fixture", port: 3306 };
  await inspectDatabaseSql(credentials, "fixture", "");
  assert.equal(sqlDiagnosticCalls[0][0], "inspect_database_sql");
  assert.equal(sqlDiagnosticCalls[0][1].table, null);
  assert.ok(!("query" in sqlDiagnosticCalls[0][1]));
  assert.notEqual(sqlDiagnosticCalls[0][1].credentials, credentials);
  await inspectDatabaseSql(credentials, "fixture", "  players  ");
  assert.equal(sqlDiagnosticCalls[1][1].table, "players");
  assert.deepEqual(sqlDiagnosticCalls.map(([command]) => command), ["inspect_database_sql", "inspect_database_sql"]);
});

test("global or missing database never invokes inspection", async () => {
  sqlDiagnosticCalls.length = 0;
  for (const database of ["", "  ", "__global__"]) {
    await assert.rejects(inspectDatabaseSql({ host: "localhost", username: "fixture", password: "fixture", port: 3306 }, database, ""), /Choose a database/);
  }
  assert.equal(sqlDiagnosticCalls.length, 0);
});
