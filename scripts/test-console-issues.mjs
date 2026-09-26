import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import ts from "typescript";

const read = (path) => readFileSync(new URL(`../${path}`, import.meta.url), "utf8");
const compile = (path) => ts.transpileModule(read(path), { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 } }).outputText;
const dataUrl = (code) => `data:text/javascript;base64,${Buffer.from(code).toString("base64")}`;
const redaction = dataUrl(compile("src/lib/core/incidentModel.ts"));
const modelUrl = dataUrl(compile("src/lib/features/fxserver/consoleIssues.ts").replace('"../../core/incidentModel"', JSON.stringify(redaction)));
const { redactIncidentText } = await import(redaction);
const { ConsoleIssueIndex, parseIssueLine, normalizeIssueMessage, filterConsoleIssues, CONSOLE_ISSUE_LIMIT, CONSOLE_ISSUE_PAGE_SIZE } = await import(modelUrl);
const { createConsoleIssueFeed, CONSOLE_ISSUE_POLL_MS } = await import(dataUrl(compile("src/lib/features/fxserver/consoleIssueFeed.ts").replace('"./consoleIssues"', JSON.stringify(modelUrl))));

const line = (id, text = "[script:inventory] SCRIPT ERROR: @inventory/server.lua:42: failed to load item", overrides = {}) => ({ id, line: text, plainLine: text, timestamp: String(1700000000 + id), stream: "stdout", ...overrides });
const deferred = () => {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
};

test("normalization ignores color, timestamps, whitespace and volatile durations/IDs", () => {
  const index = new ConsoleIssueIndex();
  index.update([
    line(1, "\x1b[31m[12:30:01] [ script:inventory ] WARNING: player 12 delayed 123 ms\x1b[0m"),
    line(2, "^3[12:30:02] [script:inventory] warning:   player 99 delayed 456 ms^7"),
  ], "A");
  const [issue] = index.issues();
  assert.equal(index.issues().length, 1);
  assert.equal(issue.resource, "inventory");
  assert.equal(issue.level, "warn");
  assert.equal(issue.count, 2);
  assert.equal(issue.first, new Date(1700000001000).toISOString());
  assert.equal(issue.last, new Date(1700000002000).toISOString());
  assert.equal(normalizeIssueMessage("failed request id=999 at 0xA12B"), "failed request id=<id> at <address>");
  assert.equal(normalizeIssueMessage("failed 550e8400-e29b-41d4-a716-446655440000"), "failed <id>");
});

test("resources, severity, file lines, status codes and real message differences remain separate", () => {
  const index = new ConsoleIssueIndex();
  index.update([
    line(1, "[script:one] error: @one/a.lua:10 code 401"),
    line(2, "[script:two] error: @one/a.lua:10 code 401"),
    line(3, "[script:one] warning: @one/a.lua:10 code 401"),
    line(4, "[script:one] error: @one/a.lua:11 code 401"),
    line(5, "[script:one] error: @one/a.lua:10 code 403"),
    line(6, "[script:one] error: disconnected"),
    line(7, "[script:one] error: @one/a.lua:10 code 401"),
  ], "A");
  assert.equal(index.issues().length, 6);
  assert.equal(index.issues()[0].count, 2);
  assert.equal(parseIssueLine(line(1, "SCRIPT ERROR: @qb-core/server/main.lua:7 failed")).resource, "qb-core");
  assert.equal(parseIssueLine(line(1, "[resources:my-resource] warning: missing file")).resource, "my-resource");
  assert.equal(parseIssueLine(line(1, "[citizen-server-impl] warning: hitch")).resource, "Server");
  assert.equal(parseIssueLine(line(1, "[script:error-resource] ready")).level, null);
  assert.equal(parseIssueLine(line(1, "0 errors; rewards granted")).level, null);
  assert.equal(parseIssueLine(line(1, "error counter", { severity: "info" })).level, null);
  assert.equal(parseIssueLine(line(1, "slow operation", { severity: "warning", resource: "bridge" })).level, "warn");
});

test("qbx explicit WARN stays a warning when its message says could not", () => {
  const text = "[script:qbx_core] [WARN] Could not find gameName for vehicle adder";
  const parsed = parseIssueLine(line(1, text));
  assert.equal(parsed.level, "warn");
  assert.equal(parsed.resource, "qbx_core");
  const index = new ConsoleIssueIndex();
  index.update([line(1, text), line(2, text)], "qbx");
  assert.equal(filterConsoleIssues(index.issues(), "gameName", "warn")[0].count, 2);
  assert.equal(filterConsoleIssues(index.issues(), "gameName", "error").length, 0);
});

test("explicit severity markers outrank message heuristics while supplied severity stays authoritative", () => {
  for (const [text, expected] of [
    ["[12:30:01] [script:qbx_core] [ WARN ] Could not find gameName", "warn"],
    ["[script:qbx_core] WARNING: failed to find gameName", "warn"],
    ["[script:qbx_core] warn: error while checking gameName", "warn"],
    ["[script:qbx_core] [ERROR] warning handler failed", "error"],
    ["[script:qbx_core] SCRIPT ERROR: warning handler", "error"],
    ["[script:qbx_core] [INFO] Could not reproduce error", null],
    ["[script:qbx_core] DEBUG: failed request retry", null],
    ["[script:qbx_core] Could not find gameName", "error"],
    ["[script:qbx_core] failed while handling a literal [WARN] value", "error"],
  ]) assert.equal(parseIssueLine(line(1, text)).level, expected, text);
  assert.equal(parseIssueLine(line(1, "[WARN] Could not find gameName", { severity: "error" })).level, "error");
  assert.equal(parseIssueLine(line(1, "ERROR: failed", { severity: "info" })).level, null);
  assert.equal(parseIssueLine(line(1, "[WARN] failed", { stream: "command" })).level, null);
});

test("retention decrements counts, advances first/last, drops groups and old representatives", () => {
  const index = new ConsoleIssueIndex(4);
  const rows = [line(1, "warning: player 1 delayed 10 ms"), line(2, "info"), line(3, "warning: player 2 delayed 20 ms"), line(4, "error: distinct")];
  index.update(rows, "A");
  const warningId = index.issues().find((issue) => issue.level === "warn").id;
  rows.shift(); rows.push(line(5, "info")); index.update(rows, "A");
  const warning = index.issues().find((issue) => issue.id === warningId);
  assert.equal(warning.count, 1);
  assert.equal(warning.firstId, 3);
  assert.equal(warning.lastId, 3);
  assert.match(warning.message, /player 2/);
  rows.splice(0, 2); rows.push(line(6, "info"), line(7, "info")); index.update(rows, "A");
  assert.equal(index.context(warningId), null);
  assert.equal(index.issues().length, 1);
  assert.equal(index.size, 4);
  index.update([line(5, "info"), line(6, "info"), line(7, "info")], "A");
  assert.equal(index.issues().length, 0);
});

test("scope changes, empty buffers, monotonic native clears and rewound IDs reset the index", () => {
  const index = new ConsoleIssueIndex();
  index.update([line(5), line(6)], "workspace-A/server-A");
  const oldId = index.issues()[0].id;
  index.update([line(5, "warning: new workspace")], "workspace-B/server-A");
  assert.equal(index.context(oldId), null);
  assert.equal(index.issues()[0].count, 1);
  assert.match(index.issues()[0].message, /new workspace/);
  index.update([], "workspace-B/server-A"); assert.equal(index.size, 0);
  index.update([line(50)], "workspace-B/server-A");
  index.update([line(99, "warning: restarted")], "workspace-B/server-A");
  assert.equal(index.size, 1); assert.match(index.issues()[0].message, /restarted/);
  index.update([line(0, "error: rewound")], "workspace-B/server-A");
  assert.equal(index.size, 1); assert.match(index.issues()[0].message, /rewound/);
});

test("context includes at most three nearby lines on each side and navigates only retained occurrences", () => {
  const index = new ConsoleIssueIndex(10);
  const rows = Array.from({ length: 10 }, (_, id) => line(id, [0, 4, 9].includes(id) ? "error: repeated" : `nearby ${id}`));
  index.update(rows, "A");
  const issue = index.issues()[0];
  const middle = index.context(issue.id, 4);
  assert.deepEqual(middle.lines.map((item) => item.id), [1, 2, 3, 4, 5, 6, 7]);
  assert.equal(middle.previousId, 0); assert.equal(middle.nextId, 9);
  assert.equal(middle.position, 2); assert.equal(middle.count, 3);
  assert.equal(index.context(issue.id, 0).lines.length, 4);
  assert.equal(index.context(issue.id).anchorId, 9);
  assert.equal(index.context(issue.id, 3), null);
  index.update(rows.slice(1), "A");
  assert.equal(index.context(issue.id, 0), null);
  assert.equal(index.context(issue.id, 4).previousId, null);
});

test("summaries, signatures, resources, search, context and tokens do not expose known credentials", () => {
  const input = line(1, "[script:inventory] error: token=super-sensitive-fixture", { segments: [
    { text: "[script:inventory] error: tok", color: "#ff0000" },
    { text: "en=super-sensitive-fixture", color: "#ffffff" },
  ] });
  const original = JSON.stringify(input);
  const index = new ConsoleIssueIndex();
  index.update([input, line(2, "rcon_password another-sensitive-fixture", { stream: "command" }),
    line(3, "error: mysql://user:password@host/database"), line(4, "error: cfxk_privatefixture")], "A");
  assert.equal(JSON.stringify(input), original);
  for (const issue of index.issues()) {
    const output = JSON.stringify([issue, index.context(issue.id)]);
    for (const secret of ["super-sensitive-fixture", "another-sensitive-fixture", "mysql://", "cfxk_privatefixture"]) assert.equal(output.includes(secret), false, secret);
  }
  assert.equal(filterConsoleIssues(index.issues(), "super-sensitive-fixture").length, 0);
  assert.equal(parseIssueLine(input).segments.length, 1);
  assert.equal(parseIssueLine(line(1, "error: okay", { resource: "cfxk_privatefixture" })).resource.includes("privatefixture"), false);
});

test("parsed plain text and safe console tokens are reused, with bounded text and no CSS injection", () => {
  const input = line(1, "raw fallback", { plainLine: "warning: slow", segments: [{ text: "warning: slow", color: "#ff0000", emphasis: true }] });
  const parsed = parseIssueLine(input);
  assert.equal(parsed.text, "warning: slow"); assert.equal(parsed.segments[0].color, "#ff0000");
  assert.equal(parsed.segments[0].emphasis, true);
  assert.equal(parseIssueLine(line(1, "error: okay", { segments: [{ text: "error: okay", color: "red;display:none" }] })).segments[0].color, null);
  assert.equal(parseIssueLine(line(1, "error: fallback", { plainLine: undefined, segments: [] })).level, "error");
  assert.equal(parseIssueLine(line(1, "error: " + "x".repeat(20000))).text.length, 4000);
  assert.equal(parseIssueLine(line(1, "info", { timestamp: "secret timestamp" })).timestamp, "");
});

test("the cheap redaction gate preserves every shared credential-redaction family", () => {
  for (const value of [
    "password=fixture", "passwd=fixture", "dbpass=fixture", "pwd=fixture", "secret=fixture", "token=fixture",
    "sv_licenseKey cfxk_fixture", "api_key=fixture", "connection_string=fixture", "Authorization: Bearer fixture",
    "Cookie: fixture", "webhook=fixture", "credential=fixture", "default_account=fixture", "IDENTIFIED BY 'fixture'",
    "Bearer fixture", "ghp_fixture", "github_pat_fixture", "sk-1234567890123", "eyJabc.def.ghi", "https://user:pass@host/path?q=fixture",
    "mysql://user:pass@host/db", "-----BEGIN RSA PRIVATE KEY-----\nfixture\n-----END RSA PRIVATE KEY-----",
  ]) {
    const text = `error: ${value}`;
    assert.equal(parseIssueLine(line(1, text)).text, redactIncidentText(text, 4000), value);
  }
});

test("search never reparses history and unchanged snapshots reuse the same summary array", () => {
  let parsed = 0;
  const index = new ConsoleIssueIndex(1000, (entry) => { parsed++; return parseIssueLine(entry); });
  const rows = Array.from({ length: 1000 }, (_, id) => line(id));
  index.update(rows, "A");
  const issues = index.issues();
  assert.equal(parsed, 1000);
  for (let count = 0; count < 100; count++) {
    assert.equal(index.update(rows.map((entry) => ({ ...entry })), "A"), false);
    assert.equal(index.issues(), issues);
    filterConsoleIssues(issues, count % 2 ? "INVENTORY failed" : "missing", "all");
  }
  assert.equal(parsed, 1000);
  assert.equal(filterConsoleIssues(issues, "INVENTORY failed").length, 1);
  assert.equal(filterConsoleIssues(issues, "inventory", "warn").length, 0);
  const retained = rows.slice(1).map((entry) => ({ ...entry, get plainLine() { throw new Error("Retained line was reparsed"); } }));
  index.update([...retained, line(1000)], "A");
  assert.equal(parsed, 1001);
  assert.equal(index.issues()[0].count, 1000);
});

test("sustained batches parse each new line once and retain at most the configured bounded window", () => {
  const index = new ConsoleIssueIndex(50000);
  let rows = [];
  for (let batch = 0; batch < 100; batch++) {
    rows = [...rows, ...Array.from({ length: 200 }, (_, offset) => line(batch * 200 + offset, `error: unique-${batch * 200 + offset}`))].slice(-CONSOLE_ISSUE_LIMIT);
    index.update(rows, "A");
    assert.equal(index.size, Math.min((batch + 1) * 200, CONSOLE_ISSUE_LIMIT));
    assert.ok(index.issues().length <= CONSOLE_ISSUE_LIMIT);
  }
  assert.equal(index.parsedCount, 20000);
  assert.equal(index.issues().length, 1000);
  assert.equal(index.issues()[0].lastId, 19999);
  assert.equal(CONSOLE_ISSUE_PAGE_SIZE, 50);
});

function feedFixture(read, yieldToBrowser = async () => {}) {
  const events = { published: [], busy: [], errors: [], scheduled: [], cancelled: [], current: true };
  const feed = createConsoleIssueFeed({ scope: "A", isCurrent: () => events.current, read, yieldToBrowser,
    publish: (index) => events.published.push(index.issues()), onBusy: (busy) => events.busy.push(busy), onError: (failed) => events.errors.push(failed),
    schedule: (callback, delay) => { events.scheduled.push({ callback, delay }); return events.scheduled.length; }, cancel: (id) => events.cancelled.push(id) });
  return { feed, events };
}

test("polling coalesces concurrent refreshes and publishes only one update per changed batch", async () => {
  const pending = deferred(); let calls = 0;
  const { feed, events } = feedFixture((limit) => { calls++; assert.equal(limit, 1000); return pending.promise; });
  const first = feed.refresh(); await feed.refresh();
  assert.equal(calls, 1); assert.equal(events.scheduled.length, 0);
  pending.resolve({ entries: [line(1), line(2)] }); await first;
  assert.equal(events.published.length, 1);
  assert.equal(events.published[0][0].count, 2);
  assert.deepEqual(events.busy, [true, false]);
  assert.equal(events.scheduled[0].delay, CONSOLE_ISSUE_POLL_MS);
  await feed.refresh();
  assert.equal(events.published.length, 1);
  feed.dispose(); assert.equal(events.cancelled.at(-1), 2);
});

test("disposed or switched feeds ignore late successes/failures and never schedule another poll", async () => {
  for (const dispose of [false, true]) for (const failure of [false, true]) {
    const pending = deferred(); const { feed, events } = feedFixture(() => pending.promise);
    const refresh = feed.refresh();
    if (dispose) feed.dispose(); else events.current = false;
    if (failure) pending.reject(new Error("private stale response")); else pending.resolve({ entries: [line(1)] });
    await refresh;
    assert.equal(events.published.length, 0);
    assert.equal(events.errors.length, 0);
    assert.equal(events.scheduled.length, 0);
    assert.deepEqual(events.busy, [true]);
  }
});

test("full buffers yield after each 50 new lines and publish once; cancellation stops parsing", async () => {
  const rows = Array.from({ length: 1000 }, (_, id) => line(id));
  let yields = 0;
  const { feed, events } = feedFixture(async () => ({ entries: rows }), async () => { yields++; });
  await feed.refresh();
  assert.equal(yields, 19);
  assert.equal(events.published.length, 1);
  assert.equal(events.published[0][0].count, 1000);
  await feed.refresh();
  assert.equal(yields, 19);
  feed.dispose();

  const waiting = deferred(); let cancellationYields = 0;
  const cancelled = feedFixture(async () => ({ entries: rows }), () => { cancellationYields++; return waiting.promise; });
  const pending = cancelled.feed.refresh(); await new Promise(setImmediate);
  assert.equal(cancellationYields, 1);
  cancelled.feed.dispose(); waiting.resolve(); await pending;
  assert.equal(cancelled.events.published.length, 0);
  assert.equal(cancelled.events.scheduled.length, 0);
});

test("feed errors contain no backend text and the next successful read clears failure state", async () => {
  let fail = true;
  const { feed, events } = feedFixture(async () => { if (fail) throw new Error("secret backend value"); return { entries: [] }; });
  await feed.refresh(); fail = false; await feed.refresh();
  assert.deepEqual(events.errors, [true, false]);
  assert.equal(events.scheduled.length, 2);
  feed.dispose();
});

test("grouping owns no persistence, keeps existing raw log views, and caps rendered groups", () => {
  const page = read("src/lib/features/fxserver/FxserverLogsPage.svelte");
  assert.match(page, /<GroupedConsoleIssues/);
  assert.match(page, /each visibleEntries as entry/);
  for (const name of ["consoleIssues.ts", "consoleIssueFeed.ts", "GroupedConsoleIssues.svelte", "ConsoleIssueContext.svelte"]) {
    assert.doesNotMatch(read(`src/lib/features/fxserver/${name}`), /localStorage|sessionStorage|indexedDB|console\.(?:log|error)|\blog\(/);
  }
  const component = read("src/lib/features/fxserver/GroupedConsoleIssues.svelte");
  assert.match(component, /filtered\.slice\(currentPage \* CONSOLE_ISSUE_PAGE_SIZE/);
  assert.match(component, /each visible as issue/);
  assert.match(component, /nextFeed\.dispose\(\)/);
});
