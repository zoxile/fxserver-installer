import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import ts from "typescript";

const url = (source) => `data:text/javascript;base64,${Buffer.from(source).toString("base64")}`;
const calls = [];
const logs = [];
globalThis.window = { __TAURI_INTERNALS__: {} };
globalThis.jooatFixture = { invoke: async (command, args) => { calls.push({ command, args }); return { installedShards: calls.filter((call) => call.command === "save_jooat_resolver_shard").length }; }, log: (...args) => logs.push(args) };
let code = ts.transpile(readFileSync(new URL("../src/lib/modules/jooat.ts", import.meta.url), "utf8"), { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 });
code = code.replace('"@tauri-apps/api/core"', JSON.stringify(url("export const invoke = (...args) => globalThis.jooatFixture.invoke(...args);")))
  .replace('"$lib/core/logger.svelte"', JSON.stringify(url("export const log = (...args) => globalThis.jooatFixture.log(...args);")));
const { installJooatResolverDatabase: install, downloadResolverText } = await import(url(`${code}\nexport { downloadResolverText };`));
const manifest = () => ({ version: "fixture", shards: Array.from({ length: 256 }, (_, index) => ({ prefix: index.toString(16).padStart(2, "0"), path: `shards/${index}.json` })) });
const source = "https://fixture.invalid/manifest.json?key=private-query";
const sourceUrl = new URL(source);
const deadline = () => Date.now() + 60_000;
const fetchOptions = [];
globalThis.fetch = async (request, options) => { fetchOptions.push(options); return new Response(new URL(request).pathname.endsWith("manifest.json") ? JSON.stringify(manifest()) : "{}"); };
await install({ manifestUrl: source });
assert.equal(calls.length, 257);
assert.ok(fetchOptions.every((options) => options.credentials === "omit" && options.referrerPolicy === "no-referrer" && options.redirect === "error" && options.signal instanceof AbortSignal));
assert.ok(!JSON.stringify(logs).includes("private-query"));
calls.length = 0;
for (const manifestUrl of ["http://fixture.invalid/manifest.json", "https://user:password@fixture.invalid/manifest.json", "data:application/json,{}", "bad-url"]) {
  await assert.rejects(install({ manifestUrl }), /HTTPS/);
}
for (const bad of [null, { version: 3, shards: [] }, { ...manifest(), shards: [null] }, { ...manifest(), shards: manifest().shards.map((shard) => ({ ...shard, path: "https://other.invalid/shard" })) }]) {
  globalThis.fetch = async () => new Response(JSON.stringify(bad));
  await assert.rejects(install({ manifestUrl: source }));
}
assert.equal(calls.length, 0, "Validate all URLs and manifest entries before native preparation");
let cancelled = false;
globalThis.fetch = async () => new Response(new ReadableStream({
  pull(controller) { controller.enqueue(new Uint8Array(5)); }, cancel() { cancelled = true; },
}));
await assert.rejects(downloadResolverText(sourceUrl, 4, deadline()), /size limit/);
assert.equal(cancelled, true, "Oversized streams must be cancelled without reading the entire response");
globalThis.fetch = async () => new Response("{}", { headers: { "content-length": "999999" } });
await assert.rejects(downloadResolverText(sourceUrl, 4, deadline()), /size limit/);
globalThis.fetch = async () => { throw new Error(`Network error for ${source}`); };
await assert.rejects(downloadResolverText(sourceUrl, 4, deadline()), (error) => !error.message.includes("private-query") && /download failed/.test(error.message));
await assert.rejects(downloadResolverText(sourceUrl, 4, Date.now() - 1), /timed out/);
const realSetTimeout = globalThis.setTimeout;
let expire;
globalThis.setTimeout = (callback) => { expire = callback; return 1; };
globalThis.fetch = (_request, options) => new Promise((_resolve, reject) => options.signal.addEventListener("abort", () => reject(new DOMException("Aborted", "AbortError"))));
const pending = downloadResolverText(sourceUrl, 4, deadline());
expire();
await assert.rejects(pending, /timed out/);
globalThis.setTimeout = realSetTimeout;
let finish;
globalThis.fetch = () => new Promise((resolve) => { finish = resolve; });
const first = install({ manifestUrl: source });
await assert.rejects(install({ manifestUrl: source }), /already in progress/);
finish(new Response("bad-json"));
await assert.rejects(first, /valid JSON/);
globalThis.fetch = async () => new Response("bad-json");
await assert.rejects(install({ manifestUrl: source }), /valid JSON/, "Failed installations must release the singleton guard");
console.log("JOOAT download fixtures passed: bounded streaming, timeout/abort, manifest/URL validation, credential-safe errors/logs, cookie/referrer omission, duplicate-install guard. No real network or native writes.");
