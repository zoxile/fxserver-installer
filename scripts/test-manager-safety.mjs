import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import ts from "typescript";
import { compile, compileModule } from "svelte/compiler";

const read = (path) => readFileSync(new URL(`../src/lib/${path}`, import.meta.url), "utf8");
const url = (source) => `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`;
function build(path, dependencies = {}) {
  let code = ts.transpile(read(path), { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 });
  for (const [name, dependency] of Object.entries(dependencies)) code = code.replaceAll(JSON.stringify(name), JSON.stringify(dependency));
  if (path.endsWith(".svelte.ts")) code = compileModule(code, { filename: path, generate: "client" }).js.code;
  code = code.replace(/(from\s+|import\s+)["'](svelte\/[^"']+)["']/g, (_, prefix, name) => `${prefix}${JSON.stringify(import.meta.resolve(name))}`);
  return url(code);
}
const storage = new Map();
let failWrites = false;
globalThis.localStorage = {
  getItem: (key) => storage.get(key) ?? null,
  setItem: (key, value) => { if (failWrites) throw new Error("Storage quota exceeded"); storage.set(key, value); },
  removeItem: (key) => storage.delete(key),
};
globalThis.window = new EventTarget();
const calls = [];
let profileResult;
globalThis.managerSafety = { invoke: async (command, args) => { calls.push({ command, args }); }, profiles: () => new Promise((resolve) => { profileResult = resolve; }) };
const transport = url("export const invoke = (...args) => globalThis.managerSafety.invoke(...args);");
const incidents = build("core/incidentModel.ts");
const { redactIncidentText } = await import(incidents);
const once = redactIncidentText("https://fixture.invalid/path?key=secret-value");
assert.equal(redactIncidentText(once), once, "Repeated log persistence must not expand URL redaction markers");
assert.equal(redactIncidentText('password="first\\"second"'), "[redacted credential]", "Escaped quotes must not leave a password suffix visible");
const loggerUrl = build("core/logger.svelte.ts", { "@tauri-apps/api/core": transport, "./incidentModel": incidents });
const logger = await import(loggerUrl);
const consoleMessages = [];
const originalInfo = console.info;
const originalError = console.error;
console.info = (...args) => consoleMessages.push(args);
console.error = (...args) => consoleMessages.push(args);
const oldLog = { id: "old", timestamp: new Date().toISOString(), level: "error", scope: "sql", message: "password=old-secret", detail: "mysql://root:db-secret@localhost/db" };
storage.set("fxserver-installer.logs", JSON.stringify([JSON.stringify(oldLog), "null", JSON.stringify({ ...oldLog, timestamp: "invalid" })]));
await logger.initializeLogger();
logger.log("TXHOST_DEFAULT_DBPASS=env-secret", { detail: "CREATE USER 'fixture' IDENTIFIED BY 'sql-secret'; https://host.invalid/?auth=link-secret" });
for (const secret of ["old-secret", "db-secret", "env-secret", "sql-secret", "link-secret"]) {
  assert.ok(!storage.get("fxserver-installer.logs").includes(secret), `Persisted credential: ${secret}`);
  assert.ok(!JSON.stringify(logger.logs).includes(secret), `Visible credential: ${secret}`);
}
const taskUrl = build("core/tasks.svelte.ts", { "@tauri-apps/api/core": transport, "./logger.svelte": loggerUrl, "./incidents.svelte": url("export const appendTaskIncident = () => {};") });
const tasks = await import(taskUrl);
failWrites = true;
assert.equal(await tasks.trackTask("fixture", "Fixture task", async () => 42), 42, "Logger storage failure must not fail a completed operation");
assert.equal(tasks.taskSession.items[0].status, "completed");
failWrites = false;
storage.set("fxserver-installer.logs", "{}");
assert.doesNotThrow(() => logger.log("Malformed history recovered"));
logger.log("x".repeat(20_000), { detail: "y".repeat(20_000) });
assert.equal(logger.logs.at(-1).message.length, 2000);
assert.equal(logger.logs.at(-1).detail.length, 8000);
assert.ok(!JSON.stringify(consoleMessages).includes("sql-secret"), "Console output must be redacted too");
console.info = originalInfo;
console.error = originalError;

const environmentUrl = build("features/fxserver/fxserverEnv.ts");
const workspaceModelUrl = build("core/workspaceSettings.ts", { "$lib/features/fxserver/fxserverEnv": environmentUrl });
const { emptyWorkspace, publicEnvironment } = await import(workspaceModelUrl);
assert.deepEqual(publicEnvironment({ TXHOST_DATA_PATH: "C:/fixture", txhost_default_dbpass: "hidden", txhost_default_account: "hidden", MYSQL_CONNECTION_STRING: "hidden", API_KEY: "hidden" }), { TXHOST_DATA_PATH: "C:/fixture" });
const databaseUrl = build("core/databaseSession.svelte.ts");
const database = await import(databaseUrl);
const settingsUrl = build("features/fxserver/fxserverSettings.svelte.ts", {
  "$lib/core/logger.svelte": loggerUrl, "$lib/core/workspaceSettings": workspaceModelUrl,
  "$lib/modules/fxserver": url("export const listTxDataProfiles = () => globalThis.managerSafety.profiles();"),
});
const settings = await import(settingsUrl);
const pathsUrl = url('let path = ""; export const getInstallPath = () => path; export const setInstallPath = (value) => { path = value; };');
const workspaceUrl = build("core/workspaces.svelte.ts", {
  "@tauri-apps/api/core": transport, "./databaseSession.svelte": databaseUrl, "./paths.svelte": pathsUrl,
  "./tasks.svelte": taskUrl, "$lib/features/fxserver/fxserverSettings.svelte": settingsUrl, "./workspaceSettings": workspaceModelUrl,
});
const workspaces = await import(workspaceUrl);
const first = emptyWorkspace("default", "First");
const second = emptyWorkspace("11111111-1111-1111-1111-111111111111", "Second");
first.txDataPath = second.txDataPath = "C:/fixture/txData";
first.profile = second.profile = "default";
first.database.password = "legacy-password";
first.environment.TXHOST_DEFAULT_DBPASS = "legacy-password";
storage.set("fxserver.manage.env", JSON.stringify(first.environment));
storage.set("fxserver-installer.workspaces.v1", JSON.stringify({ activeId: first.id, items: [first, second] }));
workspaces.initializeWorkspaces();
assert.ok(!storage.get("fxserver-installer.workspaces.v1").includes("legacy-password"));
assert.ok(!storage.get("fxserver.manage.env").includes("legacy-password"));
settings.writeSavedEnvironment({ TXHOST_API_TOKEN: "write-secret", TXHOST_DATA_PATH: first.txDataPath });
assert.ok(!storage.get("fxserver.manage.env").includes("write-secret"));
const originalRevision = database.databaseSession.revision;
const credentials = { host: "localhost", port: 3306, username: "root", password: "session-only", database: "fixture" };
database.rememberDatabaseCredentials(credentials, originalRevision);
const pendingProfiles = settings.refreshTxDataProfiles();
await workspaces.switchWorkspace(second.id);
profileResult({ profiles: ["obsolete-profile"], hasRootLogs: true });
await pendingProfiles;
assert.equal(settings.fxserverSettings.profile, "default", "Same-path workspace switches must reject stale profile results");
assert.equal(settings.fxserverSettings.profiles.length, 0);
assert.equal(database.rememberDatabaseCredentials(credentials, originalRevision), false);
assert.equal(database.databaseSession.credentials, null, "Stale validation must not populate the new workspace");
assert.ok(![...storage.values()].join("").includes("session-only"));
const edited = { ...first, environment: { TXHOST_DATA_PATH: "C:/before" }, database: { ...first.database } };
await workspaces.saveWorkspace(edited);
edited.database.host = "mutated-after-save";
assert.equal(workspaces.workspaceSession.items[0].database.host, "localhost");
assert.ok(!storage.get("fxserver-installer.workspaces.v1").includes("legacy-password"));
tasks.taskSession.switching = true;
await assert.rejects(workspaces.saveWorkspace(first), /switch/);
tasks.taskSession.switching = false;

// Run the actual component functions with deferred, in-memory UI dependencies.
function componentContext(path, names, bindings) {
  const source = read(path).match(/<script lang="ts">([\s\S]*?)<\/script>/)[1];
  const ast = ts.createSourceFile(path, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const functions = ast.statements.filter((node) => ts.isFunctionDeclaration(node) && names.includes(node.name?.text));
  assert.equal(functions.length, names.length);
  const context = vm.createContext({ console, ...bindings });
  vm.runInContext(ts.transpile(functions.map((node) => node.getText(ast)).join("\n"), { target: ts.ScriptTarget.ES2022 }), context);
  return context;
}
let finishDialog;
let mutations = 0;
const browser = componentContext("features/mariadb/DatabaseBrowserPage.svelte", ["exportCsv"], {
  active: true, table: "fixture", credentials, request: () => ({ database: "fixture", table: "row" }),
  action: (fn) => fn(), save: () => new Promise((resolve) => { finishDialog = resolve; }),
  exportBrowserCsv: () => { mutations++; },
});
const csv = browser.exportCsv();
browser.active = false;
finishDialog("C:/fixture/export.csv");
await csv;
assert.equal(mutations, 0, "A dialog from an unmounted workspace must not export data");
const diagnostics = componentContext("features/diagnostics/DiagnosticsPage.svelte", ["exportZip"], {
  active: true, preview: { id: "review-1", createdAt: 1700000000 }, exporting: false, error: "", message: "",
  save: () => new Promise((resolve) => { finishDialog = resolve; }), exportDiagnosticZip: () => { mutations++; },
});
const zip = diagnostics.exportZip();
diagnostics.preview = null;
finishDialog("C:/fixture/export.zip");
await zip;
assert.equal(mutations, 0, "An invalidated review must not export after the dialog resolves");
const selections = [];
const history = componentContext("features/config-history/ConfigHistoryPanel.svelte", ["choose"], {
  generation: 0, request: {}, selected: null, selectedId: "", revealed: false, reviewed: false, error: "", readConfigHistoryVersion: () => new Promise((resolve) => selections.push(resolve)),
});
const a = history.choose("a");
const b = history.choose("b");
const latest = history.choose("a");
selections[2]({ content: "latest" }); await latest;
selections[0]({ content: "obsolete" }); await a;
selections[1]({ content: "other" }); await b;
assert.equal(history.selected.content, "latest", "A-B-A selections must not accept an older request");
const editor = componentContext("features/fxserver/ConfigureServerPage.svelte", ["escapeHtml", "highlightCfgLine", "highlightCfgValue", "cfgValueClass"], {});
for (const line of ['set name "<img src=x onerror=alert(1)>"', '# <svg onload="alert(1)">', '<script>alert(1)</script>', '&lt;img src=x&gt;']) {
  const html = editor.highlightCfgLine(line);
  assert.ok(!/<(?:img|svg|script)\b/i.test(html), "Untrusted config text became HTML");
}
const cfgSource = read("features/fxserver/ConfigureServerPage.svelte");
const richExpression = cfgSource.match(/const richEditor = \$derived\((.*)\);/)[1];
for (const editorContent of ["x\n".repeat(20_000), "x".repeat(200_001)]) {
  assert.equal(vm.runInNewContext(richExpression, { editorContent }), false, "Large configurations must bypass highlighted DOM");
}

const sqlSource = read("features/mariadb/SqlRunnerPage.svelte").match(/<script lang="ts">([\s\S]*?)<\/script>/)[1];
const sqlAst = ts.createSourceFile("sql.ts", sqlSource, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
const sqlVariables = new Set(["credentials", "credentialsReady", "backupTables", "selectedBackupTable", "backupMode", "backupDatabaseName", "backupOptions", "backupTableRequestId", "active", "error"]);
const sqlFixture = sqlAst.statements.filter((node) =>
  ts.isVariableStatement(node) && node.declarationList.declarations.some((entry) => sqlVariables.has(entry.name.getText(sqlAst)))
  || ts.isFunctionDeclaration(node) && node.name?.text === "refreshBackupTables"
  || ts.isExpressionStatement(node) && ts.isCallExpression(node.expression) && node.expression.expression.getText(sqlAst) === "$effect",
).map((node) => node.getText(sqlAst)).join("\n");
let tableRequests = 0;
globalThis.managerSafety.tables = async () => { tableRequests++; throw new Error("Offline table list"); };
const internalUrl = import.meta.resolve("svelte/internal/client");
const fixtureSource = `import { untrack } from ${JSON.stringify(internalUrl)};
const databaseSession = { credentials: null, defaults: { host: "localhost", port: 3306, username: "root", database: "fixture" } };
const listMariaDBTables = (...args) => globalThis.managerSafety.tables(...args);
export function createFixture() {
${sqlFixture}
return { select(database) { backupDatabaseName = database; credentialsReady = true; backupMode = "tables"; }, rows() { return backupTables; } };
}`;
let fixtureCode = compileModule(ts.transpile(fixtureSource, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 }), { filename: "sql-effects.svelte.js", generate: "client" }).js.code;
fixtureCode = fixtureCode.replace(/(from\s+|import\s+)["'](svelte\/[^"']+)["']/g, (_, prefix, name) => `${prefix}${JSON.stringify(import.meta.resolve(name))}`);
const { createFixture } = await import(url(fixtureCode));
const { effect_root, render_effect, flush: flushSync } = await import(internalUrl);
let sql;
const destroy = effect_root(() => render_effect(() => { sql = createFixture(); }));
flushSync();
sql.select("fixture");
for (let i = 0; i < 5; i++) { flushSync(); await new Promise(setImmediate); }
assert.equal(tableRequests, 1, "A failed table list must not retry through its own effect");
globalThis.managerSafety.tables = async () => { tableRequests++; return ["fixture_table"]; };
sql.select("other");
for (let i = 0; i < 3; i++) { flushSync(); await new Promise(setImmediate); }
assert.equal(tableRequests, 2);
assert.deepEqual([...sql.rows()], ["fixture_table"]);
destroy();

// Svelte compilation checks only touched views; the parent owns full checks/build/UI.
for (const path of ["mariadb/ConnectionCard", "mariadb/SqlRunnerPage", "mariadb/MariaDBPanel", "mariadb/DatabaseBrowserPage", "mariadb/DatabaseRowEditor", "diagnostics/DiagnosticsPage", "config-history/ConfigHistoryPanel", "fxserver/ConfigureServerPage", "fxserver/ManageServerPage"]) {
  compile(read(`features/${path}.svelte`), { filename: `${path}.svelte`, generate: "client" });
}
console.log("Manager safety fixtures passed: redaction, storage failures, credential isolation, same-path switches, stale export dialogs/history reads, escaped HTML, bounded config rendering, reactive retry prevention, touched-view compilation.");
