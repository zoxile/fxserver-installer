import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import ts from "typescript";

const source = readFileSync(new URL("../src/lib/modules/resourceDependencies.ts", import.meta.url), "utf8");
const compiled = ts.transpile(source, { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ES2022 });
const { buildResourceGraph, dependencyImpact, dependencyStartupOrder, resourcePathKey, dependencyPage, dependencyNames } = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString("base64")}`);
const resource = (name, dependencies = [], extra = {}) => ({ name, path: `/resources/${name}`, dependencies, provides: [], groups: [], dynamic: false, origin: "data", ...extra });
const graph = (resources, extra = {}) => buildResourceGraph({ resources, resourcesIncomplete: false, configsIncomplete: false, startup: [], warnings: [], ...extra });
const names = (ids) => ids.map((id) => id.split("/").at(-1));

test("direct, transitive and reverse relationships; deterministic dependency-first startup", () => {
  const g = graph([resource("job", ["core"]), resource("core", ["db"]), resource("db"), resource("ui", ["core", "db"])]);
  const impact = dependencyImpact(g, "/resources/db");
  assert.deepEqual(names(impact.direct), ["core", "ui"]);
  assert.deepEqual(names(impact.transitive), ["job"]);
  assert.deepEqual(names(dependencyImpact(g, "/resources/job").indirectDependencies), ["db"]);
  assert.deepEqual(names(dependencyStartupOrder(g).order), ["db", "core", "job", "ui"]);
  assert.deepEqual(dependencyStartupOrder(g).blocked, []);
});

test("cycles and self loops are identified without including downstream nodes", () => {
  const g = graph([resource("a", ["b"]), resource("b", ["a"]), resource("c", ["a"]), resource("self", ["self"]), resource("ok")]);
  assert.deepEqual(g.cycles.map(names).sort(), [["a", "b"], ["self"]]);
  assert.deepEqual(names(dependencyStartupOrder(g).order), ["ok"]);
  assert.deepEqual(names(dependencyStartupOrder(g).blocked), ["a", "b", "c", "self"]);
  assert.deepEqual(names(dependencyImpact(g, "/resources/a").transitive), []);
  assert.ok(!dependencyImpact(g, "/resources/a").direct.includes("/resources/a"));
});

test("missing dependencies block downstream order, incomplete scans say unresolved", () => {
  const resources = [resource("broken", ["missing"]), resource("consumer", ["broken"]), resource("ok")];
  const g = graph(resources);
  assert.equal(g.declarations.get("/resources/broken")[0].kind, "missing");
  assert.deepEqual(names(dependencyStartupOrder(g).blocked), ["broken", "consumer"]);
  assert.equal(graph(resources, { resourcesIncomplete: true }).declarations.get("/resources/broken")[0].kind, "unresolved");
});

test("provides resolves aliases; constraints are neither missing resources nor graph edges", () => {
  const g = graph([resource("qbx", ["/server:7290", "/onesync", "/gameBuild:h4", "/native:ABC", "/policy:subdir_file_mapping"], { provides: ["qb-core", "QBX"] }), resource("job", ["QB-core", "qb-core"])]);
  assert.equal(g.dependencies.get("/resources/job").size, 1);
  assert.equal(g.dependencies.get("/resources/qbx").size, 0);
  assert.ok(g.declarations.get("/resources/qbx").every((entry) => entry.kind === "constraint"));
  assert.deepEqual(names(dependencyStartupOrder(g).order), ["qbx", "job"]);
});

test("multiple providers and data/artifact shadows remain possible impact, never guessed", () => {
  const g = graph([resource("original"), resource("replacement", [], { provides: ["original"] }), resource("job", ["original"]), resource("ui", ["job"]), resource("chat"), resource("chat", [], { path: "/artifact/chat", origin: "artifact" }), resource("consumer", ["chat"])]);
  assert.equal(g.declarations.get("/resources/job")[0].kind, "ambiguous");
  assert.equal(g.declarations.get("/resources/consumer")[0].kind, "ambiguous");
  assert.deepEqual(dependencyImpact(g, "/resources/replacement").direct, []);
  assert.deepEqual(names(dependencyImpact(g, "/resources/replacement").possible), ["job", "ui"]);
  assert.ok(dependencyStartupOrder(g).blocked.includes("/resources/job"));
});

test("dynamic metadata and hostile object keys stay explicit and safe", () => {
  const g = graph([resource("dynamic", [], { dynamic: true }), resource("__proto__"), resource("constructor", ["__proto__"])]);
  assert.deepEqual(names(dependencyStartupOrder(g).blocked), ["dynamic"]);
  assert.deepEqual(names(dependencyImpact(g, "/resources/__proto__").direct), ["constructor"]);
  assert.equal(dependencyImpact(g, "/absent").resource, undefined);
});

test("normalizes Windows paths without folding POSIX path case", () => {
  assert.equal(resourcePathKey("\\\\?\\C:\\Server\\resource\\"), "c:/server/resource");
  assert.equal(resourcePathKey("\\\\server\\Share\\resource"), "//server/share/resource");
  assert.equal(resourcePathKey("\\\\?\\UNC\\Server\\Share\\resource"), "//server/share/resource");
  assert.notEqual(resourcePathKey("/case/A"), resourcePathKey("/case/a"));
});

test("5000-node chains do not recurse and oversized provider graphs are bounded", () => {
  const resources = Array.from({ length: 5000 }, (_, index) => resource(`r${index}`, index ? [`r${index - 1}`] : []));
  const g = graph(resources);
  assert.equal(dependencyStartupOrder(g).order.length, 5000);
  assert.equal(dependencyImpact(g, "/resources/r0").transitive.length, 4998);
  const wide = graph(Array.from({ length: 400 }, (_, index) => resource(`p${index}`, ["shared"], { provides: ["shared"] })));
  assert.equal(wide.truncated, true);
  assert.ok([...wide.possibleDependencies.values()].reduce((sum, edges) => sum + edges.size, 0) <= 100_000);
});

test("dependency list pages stay bounded, preserve order and clamp stale page indices", () => {
  const items = Array.from({ length: 5000 }, (_, index) => index);
  assert.deepEqual(dependencyPage(items, 0).items, items.slice(0, 50));
  assert.equal(dependencyPage(items, 1).start, 50);
  assert.deepEqual(dependencyPage(items, 999).items, items.slice(4950));
  assert.equal(dependencyPage(items, -1).page, 0);
  assert.equal(dependencyPage(items, NaN).page, 0);
  assert.deepEqual(dependencyPage(items.slice(0, 3), 99), { page: 0, start: 0, items: [0, 1, 2] });
  assert.deepEqual(dependencyPage([], 20), { page: 0, start: 0, items: [] });
  assert.equal(dependencyPage(items.slice(0, 51), 1).items.length, 1);
});

test("cycle/provider summaries cannot expand thousands of names into one list item", () => {
  const g = graph(Array.from({ length: 5000 }, (_, index) => resource(`item-${index}`)));
  const ids = [...g.nodes.keys()];
  assert.equal(dependencyNames(g, ids.slice(0, 2)), "item-0, item-1");
  assert.match(dependencyNames(g, ids), /item-7 \(8 of 5000 shown\)$/);
  assert.ok(!dependencyNames(g, ids).includes("item-8"));
});
