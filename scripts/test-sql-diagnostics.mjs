import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import ts from "typescript";

const dataUrl = (source) => `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`;
let code = ts.transpile(readFileSync(new URL("../src/lib/modules/sqlDiagnostics.ts", import.meta.url), "utf8"), { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 });
code = code.replace('"@tauri-apps/api/core"', JSON.stringify(dataUrl("export const invoke = (...args) => globalThis.sqlDiagnosticCalls.push(args);")));
code = code.replace('"$lib/core/tasks.svelte"', JSON.stringify(dataUrl("export const trackTask = (_id, _name, run) => run();")));
const { diagnoseSqlError, inspectDatabaseSql } = await import(dataUrl(code));
globalThis.sqlDiagnosticCalls = [];

test("errors produce bounded review-only advice, never SQL execution", () => {
  for (const error of ["ERROR 1005 (HY000): cannot create table", "ERROR 1267 (HY000): illegal mix", "ERROR 1452: constraint", "ERROR 1213: deadlock", "ERROR 2013: lost connection"]) {
    const result = diagnoseSqlError(error);
    assert.ok(result.checks.length >= 2);
    assert.ok(!result.title.includes("Review the SQL error"));
  }
  assert.equal(sqlDiagnosticCalls.length, 0);
  assert.equal(diagnoseSqlError("password=this-is-not-an-error").title, "Review the SQL error");
  assert.equal(diagnoseSqlError("x".repeat(32768) + " ERROR 1267").title, "Review the SQL error");
  assert.match(diagnoseSqlError("unknown failure").checks.join(" "), /partially applied/);
});

test("inspection calls only the dedicated read-only backend", async () => {
  await inspectDatabaseSql({ host: "localhost", username: "fixture", password: "fixture", port: 3306 }, "fixture", "");
  assert.equal(sqlDiagnosticCalls[0][0], "inspect_database_sql");
  assert.equal(sqlDiagnosticCalls[0][1].table, null);
  assert.ok(!("query" in sqlDiagnosticCalls[0][1]));
});
