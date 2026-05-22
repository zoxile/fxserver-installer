import { readFile, writeFile } from "node:fs/promises";

const VERSION_RE = /^v?(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$/;
const bumpTypes = new Set(["major", "minor", "patch", "prerelease"]);

const target = process.argv[2]?.trim();

if (!target) {
	console.error("Usage: npm run version:bump -- <major|minor|patch|prerelease|x.y.z>");
	process.exit(1);
}

const packagePath = "package.json";
const packageLockPath = "package-lock.json";
const tauriConfigPath = "src-tauri/tauri.conf.json";
const cargoTomlPath = "src-tauri/Cargo.toml";
const cargoLockPath = "src-tauri/Cargo.lock";

const packageJson = JSON.parse(await readFile(packagePath, "utf8"));
const packageLock = JSON.parse(await readFile(packageLockPath, "utf8"));
const tauriConfig = JSON.parse(await readFile(tauriConfigPath, "utf8"));
const cargoToml = await readFile(cargoTomlPath, "utf8");
const cargoLock = await readFile(cargoLockPath, "utf8");

const currentVersion = tauriConfig.version || packageJson.version;
const nextVersion = bumpTypes.has(target) ? bumpVersion(currentVersion, target) : normalizeVersion(target);

packageJson.version = nextVersion;
packageLock.version = nextVersion;
if (packageLock.packages?.[""]) {
	packageLock.packages[""].version = nextVersion;
}
tauriConfig.version = nextVersion;

await writeJson(packagePath, packageJson);
await writeJson(packageLockPath, packageLock);
await writeJson(tauriConfigPath, tauriConfig);
await writeFile(cargoTomlPath, replaceCargoPackageVersion(cargoToml, nextVersion), "utf8");
await writeFile(cargoLockPath, replaceCargoLockPackageVersion(cargoLock, nextVersion), "utf8");

console.log(`Bumped app version to ${nextVersion}`);

function bumpVersion(version, type) {
	const parsed = parseVersion(version);

	if (type === "major") {
		return `${parsed.major + 1}.0.0`;
	}

	if (type === "minor") {
		return `${parsed.major}.${parsed.minor + 1}.0`;
	}

	if (type === "patch") {
		return `${parsed.major}.${parsed.minor}.${parsed.patch + 1}`;
	}

	const prerelease = parsed.prerelease ? bumpPrerelease(parsed.prerelease) : "beta.0";
	return `${parsed.major}.${parsed.minor}.${parsed.patch + (parsed.prerelease ? 0 : 1)}-${prerelease}`;
}

function bumpPrerelease(value) {
	const parts = value.split(".");
	const last = parts.at(-1);

	if (last && /^\d+$/.test(last)) {
		parts[parts.length - 1] = String(Number(last) + 1);
		return parts.join(".");
	}

	return `${value}.1`;
}

function normalizeVersion(version) {
	const parsed = parseVersion(version);
	return `${parsed.major}.${parsed.minor}.${parsed.patch}${parsed.prerelease ? `-${parsed.prerelease}` : ""}`;
}

function parseVersion(version) {
	const match = VERSION_RE.exec(version);
	if (!match) {
		console.error(`Invalid version "${version}". Use semver like 1.2.3, or a bump type.`);
		process.exit(1);
	}

	return {
		major: Number(match[1]),
		minor: Number(match[2]),
		patch: Number(match[3]),
		prerelease: match[4] ?? "",
	};
}

function replaceCargoPackageVersion(content, version) {
	const lines = content.split(/\r?\n/);
	let inPackage = false;

	for (let index = 0; index < lines.length; index += 1) {
		const line = lines[index];
		if (line.trim() === "[package]") {
			inPackage = true;
			continue;
		}

		if (inPackage && line.startsWith("[") && line.trim() !== "[package]") {
			break;
		}

		if (inPackage && line.trimStart().startsWith("version = ")) {
			lines[index] = `version = "${version}"`;
			return lines.join("\n");
		}
	}

	console.error("Could not find the Cargo [package] version field.");
	process.exit(1);
}

function replaceCargoLockPackageVersion(content, version) {
	const lines = content.split(/\r?\n/);
	let inPackage = false;
	let isAppPackage = false;

	for (let index = 0; index < lines.length; index += 1) {
		const line = lines[index];
		const trimmed = line.trim();

		if (trimmed === "[[package]]") {
			inPackage = true;
			isAppPackage = false;
			continue;
		}

		if (inPackage && trimmed.startsWith("name = ")) {
			isAppPackage = trimmed === 'name = "app"';
			continue;
		}

		if (inPackage && isAppPackage && trimmed.startsWith("version = ")) {
			lines[index] = `version = "${version}"`;
			return lines.join("\n");
		}
	}

	return content;
}

function writeJson(path, value) {
	return writeFile(path, `${JSON.stringify(value, null, 2)}\n`, "utf8");
}
