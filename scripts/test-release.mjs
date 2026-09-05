import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";

const bump = resolve("scripts/bump-version.mjs");
const files = ["package.json", "package-lock.json", "src-tauri/tauri.conf.json", "src-tauri/Cargo.toml", "src-tauri/Cargo.lock"];
async function fixture(run) {
  const root = await mkdtemp(join(tmpdir(), "fxsi-version-test-"));
  try {
    await mkdir(join(root, "src-tauri"));
    const data = ['{"version":"0.3.2"}', '{"version":"0.3.2","packages":{"":{"version":"0.3.2"}}}', '{"version":"0.3.2"}', '[package]\nname = "app"\nversion = "0.3.2"\n\n[dependencies]\nserde = "1"\n', '[[package]]\nname = "app"\nversion = "0.3.2"\n\n[[package]]\nname = "serde"\nversion = "1.0.0"\n'];
    for (let index = 0; index < files.length; index++) await writeFile(join(root, files[index]), data[index]);
    await run(root);
  } finally { await rm(root, { recursive: true, force: true }); }
}
const runBump = (root, version) => spawnSync(process.execPath, [bump, version], { cwd: root, encoding: "utf8", windowsHide: true });
const contents = (root) => Promise.all(files.map((file) => readFile(join(root, file), "utf8")));

test("version bump synchronizes manifests and preserves dependency versions", async () => fixture(async (root) => {
  assert.equal(runBump(root, "minor").status, 0);
  const result = await contents(root);
  assert.equal(JSON.parse(result[0]).version, "0.4.0");
  assert.equal(JSON.parse(result[1]).packages[""].version, "0.4.0");
  assert.equal(JSON.parse(result[2]).version, "0.4.0");
  assert.match(result[3], /version = "0\.4\.0"/);
  assert.match(result[4], /name = "serde"\nversion = "1\.0\.0"/);
  assert.equal(runBump(root, "0.4.1-beta.9007199254740992").status, 0);
  assert.equal(runBump(root, "prerelease").status, 0);
  assert.equal(JSON.parse(await readFile(join(root, files[0]), "utf8")).version, "0.4.1-beta.9007199254740993");
}));

test("invalid versions and missing Cargo entries do not partially rewrite manifests", async () => fixture(async (root) => {
  const original = await contents(root);
  for (const version of ["01.2.3", "1.2.3-beta..1", "1.2.3-01", "99999999999999999.0.0", "1.2.3/path"]) {
    assert.notEqual(runBump(root, version).status, 0);
    assert.deepEqual(await contents(root), original);
  }
  await writeFile(join(root, files[4]), '[[package]]\nname = "different"\nversion = "1.0.0"\n');
  const before = await contents(root);
  assert.notEqual(runBump(root, "patch").status, 0);
  assert.deepEqual(await contents(root), before);
}));
