import assert from "node:assert/strict";
import { test } from "node:test";
import { githubRequest, installerName, publishVerifiedRelease, validateConfiguration, validateDraft, validateManifest, verifyInstaller } from "./validate-updater-release.mjs";

// Public test vectors from tauri-plugin-updater 2.14.0 (Apache-2.0/MIT), not a release key.
// https://github.com/tauri-apps/plugins-workspace/blob/updater-v2.14.0/plugins/updater/src/updater.rs
const encode = (value) => Buffer.from(value).toString("base64");
const publicKey = encode("untrusted comment: minisign public key: A49EF4746FA6E6D7\nRWTX5qZvdPSepDGWg2bz8n6ArQnBMwgQK1geVosorcbLzcunD+KJ+cYe");
const rawSignature = "untrusted comment: signature from tauri secret key\nRUTX5qZvdPSepPF2zsjvP/L/TQtBLRnWRvaq+4omK1UEugtQp/USrGiI1v762sDy3id3aor7bRC9ETyeot6lMHHrWoZ5hjUM1Qc=\ntrusted comment: timestamp:1791649051\tfile:payload\tversion:2.0.0\nDDq2CLOEl5cQmC2eJRHlnZPVViOZsu1CjpPKQoei4A/XBfkDqGO0B+QMrcrHgEQ5qV3mne75tdFWhJpqUXh9AA==";
const signature = encode(rawSignature);
const version = "2.0.0";
const expected = { version, tag: `v${version}`, commit: "a".repeat(40), prerelease: true, publicKey };
const name = installerName(version);
const installerUrl = `https://github.com/zoxile/fxserver-installer/releases/download/v${version}/${name}`;
const platform = () => ({ url: installerUrl, signature });
const manifest = () => ({ version, platforms: { "windows-x86_64": platform(), "windows-x86_64-nsis": platform() } });
const draft = () => ({ draft: true, tag_name: expected.tag, target_commitish: expected.commit, prerelease: true, assets: [name, `${name}.sig`, "latest.json"].map((name, i) => ({ name, id: i + 1, size: 1024, state: "uploaded" })) });
const config = () => ({ version, identifier: "com.fxserver.installer", plugins: { updater: { pubkey: publicKey, endpoints: [], requireSignedVersion: true, windows: { installMode: "passive" } } } });

test("release config requires a known channel, public key, and strict updater settings", () => {
  assert.equal(validateConfiguration(config(), { channel: "beta" }, [version]).tag, expected.tag);
  for (const channel of ["development", "", "nightly"]) assert.throws(() => validateConfiguration(config(), { channel }, [version]));
  assert.throws(() => validateConfiguration(config(), { channel: "stable" }, [version]));
  for (const patch of [{ pubkey: "" }, { requireSignedVersion: false }, { allowDowngrades: true }, { dangerousAcceptInvalidCerts: true }, { dangerousAcceptInvalidHostnames: true }, { dangerousInsecureTransportProtocol: true }, { endpoints: ["https://other.example/feed"] }, { windows: { installMode: "passive", installerArgs: ["/D=C:/other"] } }]) {
    const value = config();
    Object.assign(value.plugins.updater, patch);
    assert.throws(() => validateConfiguration(value, { channel: "beta" }, [version]));
  }
});

test("feed selects only the exact NSIS installer and matching version", () => {
  assert.equal(validateManifest(manifest(), version).url, installerUrl);
  for (const url of ["https://evil.example/update.exe", installerUrl.replace("2.0.0", "1.0.0"), `${installerUrl}?download=1`, installerUrl.replace("windows_x64-setup.exe", "linux_x64.AppImage"), installerUrl.replace(".exe", ".msi")]) {
    const value = manifest();
    value.platforms["windows-x86_64"].url = url;
    assert.throws(() => validateManifest(value, version));
  }
  const value = manifest();
  value.platforms["linux-x86_64"] = platform();
  assert.throws(() => validateManifest(value, version));
  assert.throws(() => validateManifest({ ...manifest(), version: "3.0.0" }, version));
  assert.throws(() => validateManifest({ version, platforms: {} }, version));
});

test("feed rejects missing, malformed, or inconsistent signatures", () => {
  for (const signature of ["", "malformed", " ", "?".repeat(5000)]) {
    const value = manifest();
    value.platforms["windows-x86_64"].signature = signature;
    assert.throws(() => validateManifest(value, version));
  }
});

test("cryptographic verification accepts the official version-bound test vector", () => {
  verifyInstaller(Buffer.from("test"), signature, publicKey, version);
});

test("cryptographic verification rejects changed bytes, signature, key, and version", () => {
  assert.throws(() => verifyInstaller(Buffer.from("other"), signature, publicKey, version));
  assert.throws(() => verifyInstaller(Buffer.from("test"), signature, publicKey, "3.0.0"));
  assert.throws(() => verifyInstaller(Buffer.from("test"), encode(rawSignature.replace("version:2.0.0", "version:3.0.0")), publicKey, "3.0.0"));
  assert.throws(() => verifyInstaller(Buffer.from("test"), signature, encode(Buffer.from(publicKey, "base64").toString().replace("RWTX", "RWTY")), version));
  assert.throws(() => verifyInstaller(Buffer.from("test"), encode(rawSignature.replace("DDq2", "DDq3")), publicKey, version));
  assert.throws(() => verifyInstaller(Buffer.from("test"), "bad", publicKey, version));
});

test("old valid signatures without a signed version cannot be published", () => {
  const key = encode("untrusted comment: minisign public key E7620F1842B4E81F\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3");
  const sig = encode("untrusted comment: signature from minisign secret key\nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\ntrusted comment: timestamp:1633700835\tfile:test\tprehashed\nwLMDjy9FLAuxZ3q4NlEvkgtyhrr0gtTu6KC4KBJdITbbOeAi1zBIYo0v4iTgt8jJpIidRJnp94ABQkJAgAooBQ==");
  assert.throws(() => verifyInstaller(Buffer.from("test"), sig, key, version), /signed version/);
});

test("only complete drafts for the expected commit and channel are eligible", () => {
  assert.equal(Object.keys(validateDraft(draft(), expected)).length, 3);
  for (const patch of [{ draft: false }, { tag_name: "v1.0.0" }, { target_commitish: "b".repeat(40) }, { prerelease: false }, { assets: [] }]) assert.throws(() => validateDraft({ ...draft(), ...patch }, expected));
  for (const patch of [{ size: 0 }, { size: undefined }, { size: "1024" }, { size: 257 * 1024 * 1024 }, { state: "new" }, { id: "1/../../other" }]) {
    const value = draft();
    Object.assign(value.assets[0], patch);
    assert.throws(() => validateDraft(value, expected));
  }
  const duplicate = draft();
  duplicate.assets.push(duplicate.assets[0]);
  assert.throws(() => validateDraft(duplicate, expected));
});

test("asset redirects strip authentication and accept only the GitHub asset CDN", async () => {
  const calls = [];
  const fetcher = async (url, options) => {
    calls.push({ url, options });
    return calls.length === 1 ? new Response(null, { status: 302, headers: { location: "https://release-assets.githubusercontent.com/asset?token=value" } }) : new Response("test");
  };
  const result = await githubRequest("/releases/assets/1", { token: "example-token", binary: true, fetcher });
  assert.equal(result.toString(), "test");
  assert.equal(calls[0].options.headers.Authorization, "Bearer example-token");
  assert.equal(calls[1].options.headers.Authorization, undefined);
  for (const location of ["http://release-assets.githubusercontent.com/a", "https://evil.example/a", "https://release-assets.githubusercontent.com.evil.test/a", "https://user@release-assets.githubusercontent.com/a", "https://release-assets.githubusercontent.com:8443/a", "https://release-assets.githubusercontent.com/a#x"]) {
    await assert.rejects(githubRequest("/releases/assets/1", { token: "example", binary: true, fetcher: async () => new Response(null, { status: 302, headers: { location } }) }), /untrusted/);
  }
});

test("release downloads have hard byte limits, including absent content-length", async () => {
  for (const headers of [{}, { "content-length": "1000" }]) {
    await assert.rejects(githubRequest("/releases/assets/1", { token: "example", binary: true, limit: 3, fetcher: async () => new Response("test", { headers }) }), /size limit/);
  }
  await assert.rejects(githubRequest("/releases/assets/1", { token: "example", binary: true, fetcher: async () => new Response(null, { status: 403 }) }), /HTTP 403/);
});

function publicationFixture({ missingAsset = false, tampered = false, wrongFeed = false } = {}) {
  const release = draft();
  release.assets[0].size = 4;
  if (missingAsset) release.assets.pop();
  const feed = manifest();
  if (wrongFeed) feed.version = "3.0.0";
  const requests = [];
  return {
    requests,
    options: {
      token: "example-token",
      fetcher: async (url, options) => {
        requests.push({ url, method: options.method, body: options.body });
        if (options.method === "PATCH") return Response.json({ ...release, draft: false });
        if (url.endsWith("/releases/1")) return Response.json(release);
        if (url.endsWith("/assets/3")) return Response.json(feed);
        if (url.endsWith("/assets/2")) return new Response(signature);
        if (url.endsWith("/assets/1")) return new Response(tampered ? "fail" : "test");
        throw new Error(`Unexpected test request: ${url}`);
      },
    },
  };
}

test("publication is the final operation after uploaded assets verify", async () => {
  const { requests, options } = publicationFixture();
  await publishVerifiedRelease(expected, 1, options);
  assert.deepEqual(requests.map((request) => request.method), ["GET", "GET", "GET", "GET", "PATCH"]);
  assert.deepEqual(JSON.parse(requests.at(-1).body), { draft: false, make_latest: "false" });
});

test("a failed asset or signature check never publishes the draft", async () => {
  for (const invalid of [{ missingAsset: true }, { tampered: true }, { wrongFeed: true }]) {
    const { requests, options } = publicationFixture(invalid);
    await assert.rejects(publishVerifiedRelease(expected, 1, options));
    assert.equal(requests.filter((request) => request.method === "PATCH").length, 0);
  }
});
