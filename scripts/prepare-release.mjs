import { appendFile, readFile } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { randomBytes } from "node:crypto";

const readJson = async (path) => JSON.parse(await readFile(path, "utf8"));
const git = (...args) => execFileSync("git", args, { encoding: "utf8", windowsHide: true }).trim();
const pkg = await readJson("package.json");
const lock = await readJson("package-lock.json");
const config = await readJson("src-tauri/tauri.conf.json");
const policy = await readJson("release-policy.json");
const version = config.version;
if (typeof version !== "string" || !/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?$/.test(version)) throw new Error("Invalid release version.");
if ([pkg.version, lock.version, lock.packages?.[""]?.version].some((value) => value !== version)) throw new Error("Frontend and Tauri versions differ. Run version:bump.");
const metadata = JSON.parse(execFileSync("cargo", ["metadata", "--manifest-path", "src-tauri/Cargo.toml", "--no-deps", "--format-version", "1", "--locked", "--offline"], { encoding: "utf8", windowsHide: true }));
if (metadata.packages.find((item) => item.name === "app")?.version !== version) throw new Error("Rust and Tauri versions differ. Run version:bump.");
if (!Array.isArray(policy.betaVersions) || policy.betaVersions.some((item) => typeof item !== "string")) throw new Error("Invalid beta release policy.");
const beta = policy.betaVersions.includes(version) || version.includes("-");
const tag = `v${version}`;
let body;
try { body = await readFile(`docs/releases/${tag}.md`, "utf8"); }
catch (error) { if (error.code !== "ENOENT") throw error; }
if (!body) {
  const previous = git("tag", "--merged", "HEAD", "--sort=-version:refname").split(/\r?\n/).find((value) => /^v\d+\.\d+\.\d+/.test(value) && value !== tag);
  const subjects = git("log", ...(previous ? [`${previous}..HEAD`] : []), "--format=%s");
  const entries = subjects.split(/\r?\n/).filter((line) => /^(feat|fix|perf|refactor|docs|test|chore)(\([^)]+\))?!?:/.test(line));
  body = entries.length ? `## Changes\n\n${entries.map((line) => `- ${line}`).join("\n")}` : `Built from ${git("rev-parse", "HEAD")}.`;
}
if (beta) body = `> [!WARNING]\n> Version ${version} is a beta and is not fully tested on live servers or databases. Issues and breaking changes may occur. Keep independent backups and validate in a disposable environment first.\n\n${body}`;
const values = { version, tag, commit: git("rev-parse", "HEAD"), prerelease: String(beta), release_name: `FXServer Installer ${tag}${beta ? " (Beta)" : ""}`, body: body.trim() };
if (process.env.GITHUB_OUTPUT) {
  const delimiter = `fxsi_${randomBytes(16).toString("hex")}`;
  await appendFile(process.env.GITHUB_OUTPUT, Object.entries(values).map(([key, value]) => `${key}<<${delimiter}\n${value}\n${delimiter}\n`).join(""));
}
console.log(JSON.stringify(values, null, 2));
