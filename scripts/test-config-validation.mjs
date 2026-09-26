import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import ts from "typescript";

const url = (source) => `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`;
let code = ts.transpile(readFileSync(new URL("../src/lib/modules/configValidation.ts", import.meta.url), "utf8"), { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 });
code = code.replace('"@tauri-apps/api/core"', JSON.stringify(url("export const invoke = (...args) => globalThis.validationCalls.push(args);")));
const { validateConfigChange, reviewMatches, canSaveReview } = await import(url(code));
const target = { txDataPath: "C:/txData", profile: "default", path: "C:/data/server.cfg" };
const clean = { revision: "test-revision", fileCount: 1, issues: [] };

test("validation invokes only its read-only command and retains exact draft bytes", async () => {
    globalThis.validationCalls = [];
    const content = 'set fixture_token "fixture-secret"\r\n';
    await validateConfigChange(target, content);
    assert.deepEqual(validationCalls, [["validate_config_change", { request: target, content }]]);
    assert.notEqual(validationCalls[0][1].request, target);
});

test("only reviewed advisories can be overridden; errors and missing revision cannot", () => {
    assert.equal(canSaveReview(clean, false, true), false);
    assert.equal(canSaveReview(clean, true, false), true);
    assert.equal(canSaveReview({ ...clean, revision: "" }, true, true), false);
    const warning = { ...clean, issues: [{ severity: "warning" }] };
    assert.equal(canSaveReview(warning, true, false), false);
    assert.equal(canSaveReview(warning, true, true), true);
    const errors = { ...clean, issues: [{ severity: "warning" }, { severity: "error" }] };
    assert.equal(canSaveReview(errors, true, true), false);
});

test("review is bound to profile, exact target, baseline and draft including newlines", () => {
    const review = { target, before: "before\r\n", after: "after\r\n", report: clean };
    assert.equal(reviewMatches(review, { ...target }, review.before, review.after), true);
    for (const changed of [null, { ...target, profile: "other" }, { ...target, txDataPath: "D:/txData" }, { ...target, path: "C:/data/permissions.cfg" }]) {
        assert.equal(reviewMatches(review, changed, review.before, review.after), false);
    }
    assert.equal(reviewMatches(review, target, "disk changed", review.after), false);
    assert.equal(reviewMatches(review, target, review.before, "after\n"), false);
});

test("editor keeps the guarded history write and review uses the existing exact diff", () => {
    const page = readFileSync(new URL("../src/lib/features/fxserver/ConfigureServerPage.svelte", import.meta.url), "utf8");
    const panel = readFileSync(new URL("../src/lib/features/config-validation/ConfigChangeReview.svelte", import.meta.url), "utf8");
    assert.match(page, /await validateConfigChange\(target, submitted\)/);
    assert.match(page, /current\.revision !== review\.report\.revision/);
    assert.match(page, /saveConfigWithHistory\(target, expected, submitted\)/);
    assert.match(page, /generation !== validationGeneration/);
    assert.match(panel, /\{#if revealed\}<ConfigDiff before=\{review.before\} after=\{review.after\}/);
    assert.match(panel, /Errors cannot be overridden/);
    assert.doesNotMatch(code, /localStorage|sessionStorage|console\.|trackTask/);
});
