import { createHash, createPublicKey, verify } from "node:crypto";
import { readFile, appendFile } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { pathToFileURL } from "node:url";

const REPOSITORY = "zoxile/fxserver-installer";
const API = `https://api.github.com/repos/${REPOSITORY}`;
const DOWNLOADS = `https://github.com/${REPOSITORY}/releases/download`;
const MAX_METADATA = 256 * 1024;
const MAX_INSTALLER = 256 * 1024 * 1024;
const TARGET = "windows-x86_64";
const versionPattern = /^(0|[1-9]\d{0,8})\.(0|[1-9]\d{0,8})\.(0|[1-9]\d{0,8})$/;
const fail = (message) => { throw new Error(message); };

function base64(value) {
  if (typeof value !== "string" || !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(value.trim())) fail("Invalid updater base64 encoding.");
  return Buffer.from(value.trim(), "base64");
}

function publicPacket(encoded) {
  const lines = base64(encoded).toString("utf8").trim().split(/\r?\n/);
  if (lines.length !== 2 || !lines[0].startsWith("untrusted comment:")) fail("Invalid updater public key.");
  const packet = base64(lines[1]);
  if (packet.length !== 42 || packet.subarray(0, 2).toString() !== "Ed") fail("Invalid updater public key packet.");
  return packet;
}

export function validateConfiguration(config, policy, betaVersions) {
  if (!versionPattern.test(config.version)) fail("Updater releases require a canonical numeric version.");
  if (!["beta", "stable"].includes(policy.channel)) fail("Development builds must not publish an updater feed.");
  if ((policy.channel === "beta") !== betaVersions.includes(config.version)) fail("Updater channel and beta release policy disagree.");
  if (config.identifier !== "com.fxserver.installer") fail("Unexpected application identity.");
  const updater = config.plugins?.updater;
  if (!updater || updater.requireSignedVersion !== true || !Array.isArray(updater.endpoints) || updater.endpoints.length !== 0) fail("Updater configuration must require version-bound signatures and app-owned endpoints.");
  if (["allowDowngrades", "dangerousInsecureTransportProtocol", "dangerousAcceptInvalidCerts", "dangerousAcceptInvalidHostnames"].some((key) => updater[key])) fail("Unsafe updater configuration.");
  if (updater.windows?.installMode !== "passive" || updater.windows?.installerArgs?.length) fail("Updater must use the managed passive NSIS install mode without extra arguments.");
  publicPacket(updater.pubkey);
  return { version: config.version, tag: `v${config.version}`, prerelease: policy.channel === "beta", publicKey: updater.pubkey };
}

export function installerName(version) {
  if (!versionPattern.test(version)) fail("Invalid updater version.");
  return `FXServer.Installer_${version}_windows_x64-setup.exe`;
}

export function validateManifest(manifest, version) {
  const url = `${DOWNLOADS}/v${version}/${installerName(version)}`;
  const entries = Object.entries(manifest?.platforms ?? {});
  const platform = manifest?.platforms?.[TARGET];
  if (manifest?.version !== version || !platform || !entries.length || entries.length > 2) fail("Updater feed version or platform is incorrect.");
  for (const [target, entry] of entries) {
    if (![TARGET, `${TARGET}-nsis`].includes(target) || entry.url !== url || typeof entry.signature !== "string" || !entry.signature.trim() || entry.signature.length > 4096 || entry.signature !== platform.signature) fail("Updater feed contains an unexpected installer URL, platform, or signature.");
    base64(entry.signature);
  }
  return platform;
}

// Minisign's prehashed format signs the payload hash and separately signs its trusted comment.
export function verifyInstaller(bytes, encodedSignature, encodedPublicKey, version) {
  const keyPacket = publicPacket(encodedPublicKey);
  const lines = base64(encodedSignature).toString("utf8").trim().split(/\r?\n/);
  if (lines.length !== 4 || !lines[0].startsWith("untrusted comment:") || !lines[2].startsWith("trusted comment: ")) fail("Invalid updater signature format.");
  const packet = base64(lines[1]);
  const globalSignature = base64(lines[3]);
  if (packet.length !== 74 || globalSignature.length !== 64 || packet.subarray(0, 2).toString() !== "ED" || !packet.subarray(2, 10).equals(keyPacket.subarray(2, 10))) fail("Updater signature algorithm or key ID does not match.");
  const publicKey = createPublicKey({ key: Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), keyPacket.subarray(10)]), format: "der", type: "spki" });
  const trusted = lines[2].slice("trusted comment: ".length);
  const signature = packet.subarray(10);
  if (!verify(null, createHash("blake2b512").update(bytes).digest(), publicKey, signature) || !verify(null, Buffer.concat([signature, Buffer.from(trusted)]), publicKey, globalSignature)) fail("Updater installer signature verification failed.");
  const versions = trusted.split("\t").filter((field) => field.startsWith("version:")).map((field) => field.slice(8));
  if (versions.length !== 1 || versions[0] !== version) fail("Updater signature is missing the expected signed version. Use a Tauri CLI supporting --app-version.");
}

export function validateDraft(release, expected) {
  if (release?.draft !== true || release.tag_name !== expected.tag || release.target_commitish !== expected.commit || release.prerelease !== expected.prerelease) fail("Release must remain a draft at the exact expected commit, tag, and channel.");
  const required = [installerName(expected.version), `${installerName(expected.version)}.sig`, "latest.json"];
  const assets = {};
  for (const name of required) {
    const matches = (release.assets ?? []).filter((asset) => asset.name === name);
    const limit = name === "latest.json" ? MAX_METADATA : name.endsWith(".sig") ? 4096 : MAX_INSTALLER;
    if (matches.length !== 1 || matches[0].state !== "uploaded" || !Number.isSafeInteger(matches[0].id) || matches[0].id <= 0 || !Number.isSafeInteger(matches[0].size) || matches[0].size <= 0 || matches[0].size > limit) fail(`Missing, duplicate, incomplete, or excessive release asset: ${name}`);
    assets[name] = matches[0];
  }
  return assets;
}

async function readLimited(response, limit) {
  const declared = response.headers.get("content-length");
  if (declared !== null && (!/^\d+$/.test(declared) || Number(declared) > limit)) fail("GitHub response exceeds its size limit.");
  const reader = response.body?.getReader();
  if (!reader) fail("Empty GitHub response.");
  const chunks = [];
  let size = 0;
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      size += value.length;
      if (size > limit) fail("GitHub response exceeds its size limit.");
      chunks.push(value);
    }
  } finally {
    await reader.cancel();
  }
  return Buffer.concat(chunks, size);
}

export async function githubRequest(path, { token, binary = false, limit = MAX_METADATA, method = "GET", body, allowMissing = false, fetcher = fetch } = {}) {
  if (!path.startsWith("/releases") || path.includes("..") || !token) fail("Invalid authenticated release request.");
  let url = `${API}${path}`;
  const timeout = AbortSignal.timeout(binary ? 15 * 60 * 1000 : 30_000);
  for (let redirects = 0; redirects < 4; redirects++) {
    const api = new URL(url).hostname === "api.github.com";
    const response = await fetcher(url, {
      method, redirect: "manual", signal: timeout,
      headers: { "User-Agent": "FXServer-Installer release-validation", "Accept": binary ? "application/octet-stream" : "application/vnd.github+json", ...(api ? { Authorization: `Bearer ${token}`, "X-GitHub-Api-Version": "2022-11-28" } : {}), ...(body ? { "Content-Type": "application/json" } : {}) },
      ...(body ? { body: JSON.stringify(body) } : {}),
    });
    if ([301, 302, 303, 307, 308].includes(response.status)) {
      await response.body?.cancel();
      const target = new URL(response.headers.get("location") ?? "", url);
      if (!binary || method !== "GET" || target.protocol !== "https:" || target.hostname !== "release-assets.githubusercontent.com" || target.port || target.username || target.password || target.hash) fail("GitHub returned an untrusted asset redirect.");
      url = target.href;
      continue;
    }
    if (response.status === 404 && allowMissing) { await response.body?.cancel(); return null; }
    if (!response.ok) { await response.body?.cancel(); fail(`GitHub release request failed with HTTP ${response.status}.`); }
    const bytes = await readLimited(response, limit);
    return binary ? bytes : JSON.parse(bytes.toString("utf8"));
  }
  fail("Too many GitHub release redirects.");
}

export async function publishVerifiedRelease(expected, releaseId, options) {
  if (!/^\d+$/.test(String(releaseId))) fail("Missing draft release ID.");
  const path = `/releases/${releaseId}`;
  const release = await githubRequest(path, options);
  const assets = validateDraft(release, expected);
  const download = (name, limit) => githubRequest(`/releases/assets/${assets[name].id}`, { ...options, binary: true, limit });
  const manifest = JSON.parse((await download("latest.json", MAX_METADATA)).toString("utf8"));
  const platform = validateManifest(manifest, expected.version);
  const signature = (await download(`${installerName(expected.version)}.sig`, 4096)).toString("utf8").trim();
  if (signature !== platform.signature.trim()) fail("Published signature and updater feed disagree.");
  const installer = await download(installerName(expected.version), MAX_INSTALLER);
  if (installer.length !== assets[installerName(expected.version)].size) fail("Published installer size does not match release metadata.");
  verifyInstaller(installer, signature, expected.publicKey, expected.version);
  const published = await githubRequest(path, { ...options, method: "PATCH", body: { draft: false, make_latest: expected.prerelease ? "false" : "true" } });
  if (published.draft || published.tag_name !== expected.tag) fail("GitHub did not confirm publication.");
}

async function main() {
  const mode = process.argv[2];
  if (!["config", "state", "publish"].includes(mode)) fail("Usage: node scripts/validate-updater-release.mjs config|state|publish");
  const config = JSON.parse(await readFile("src-tauri/tauri.conf.json", "utf8"));
  const channel = JSON.parse(await readFile("update-channel.json", "utf8"));
  const policy = JSON.parse(await readFile("release-policy.json", "utf8"));
  const expected = validateConfiguration(config, channel, policy.betaVersions);
  if (mode === "config") {
    const help = execFileSync(process.execPath, ["node_modules/@tauri-apps/cli/tauri.js", "signer", "sign", "--help"], { encoding: "utf8", windowsHide: true });
    if (!help.includes("--app-version")) fail("Upgrade the Tauri CLI: this release requires version-bound updater signatures (--app-version).");
    console.log("Updater release configuration is valid.");
    return;
  }
  if (process.env.GITHUB_REPOSITORY !== REPOSITORY) fail("Updater publication is restricted to the official repository.");
  const git = (...args) => execFileSync("git", args, { encoding: "utf8", windowsHide: true }).trim();
  expected.commit = git("rev-parse", "HEAD");
  git("merge-base", "--is-ancestor", expected.commit, "origin/main");
  const options = { token: process.env.GITHUB_TOKEN };
  if (mode === "state") {
    const release = await githubRequest(`/releases/tags/${expected.tag}`, { ...options, allowMissing: true });
    if (release?.draft && (release.target_commitish !== expected.commit || release.prerelease !== expected.prerelease)) fail("An existing draft belongs to another commit/channel. Inspect it before retrying.");
    const tag = git("tag", "--list", expected.tag);
    if (tag && !release) fail("This version is already tagged without a matching release. Resolve it manually before publishing.");
    if (tag && release?.draft && git("rev-list", "-n", "1", expected.tag) !== expected.commit) fail("The existing draft tag points to another commit.");
    const exists = release && !release.draft;
    if (process.env.GITHUB_OUTPUT) await appendFile(process.env.GITHUB_OUTPUT, `exists=${Boolean(exists)}\nrelease_id=${release?.draft ? release.id : ""}\n`);
    console.log(exists ? "Release already published; no assets will be changed." : "Ready to build a draft release.");
    return;
  }
  await publishVerifiedRelease(expected, process.env.RELEASE_ID, options);
  console.log(`Published ${expected.tag} after verifying its uploaded installer, signature, and updater feed.`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => { console.error(error.message); process.exitCode = 1; });
}
