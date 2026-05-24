import { getVersion } from "@tauri-apps/api/app";

const repository = "zoxile/fxserver-installer";
const latestVersionManifestUrl = `https://raw.githubusercontent.com/${repository}/main/src-tauri/tauri.conf.json`;
const releaseBaseUrl = `https://github.com/${repository}/releases`;

export interface AppReleaseInfo {
	version: string;
	tagName: string;
	htmlUrl: string;
	installerUrl: string;
}

function hasTauriRuntime() {
	return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function getCurrentAppVersion() {
	if (!hasTauriRuntime()) return "dev";
	return getVersion();
}

export async function fetchLatestAppRelease(): Promise<AppReleaseInfo> {
	const response = await fetch(latestVersionManifestUrl, {
		cache: "no-store",
		headers: {
			Accept: "application/json,text/plain,*/*",
		},
	});

	if (!response.ok) {
		throw new Error(`Version manifest returned ${response.status} while checking the latest app release.`);
	}

	const manifest = (await response.json()) as { version?: string };
	const version = manifest.version?.trim();
	if (!version) throw new Error("Version manifest did not include an app version.");

	const tagName = `v${normalizeVersion(version)}`;
	const fileName = `FXServer.Installer_${normalizeVersion(version)}_windows_x64-setup.exe`;
	const htmlUrl = `${releaseBaseUrl}/tag/${encodeURIComponent(tagName)}`;

	return {
		version: normalizeVersion(tagName),
		tagName,
		htmlUrl,
		installerUrl: `${releaseBaseUrl}/download/${encodeURIComponent(tagName)}/${encodeURIComponent(fileName)}`,
	};
}

export function compareVersions(left: string, right: string) {
	const leftParts = parseVersion(left);
	const rightParts = parseVersion(right);

	for (let index = 0; index < 3; index += 1) {
		const difference = leftParts[index] - rightParts[index];
		if (difference !== 0) return difference;
	}

	return 0;
}

export function normalizeVersion(version: string) {
	return version.trim().replace(/^v/i, "");
}

function parseVersion(version: string) {
	const match = normalizeVersion(version).match(/^(\d+)\.(\d+)\.(\d+)/);
	if (!match) return [0, 0, 0];
	return [Number(match[1]), Number(match[2]), Number(match[3])];
}
