import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import releasePolicy from "../../../release-policy.json";

const repository = "zoxile/fxserver-installer";
const releaseBaseUrl = `https://github.com/${repository}/releases`;

export interface AppReleaseInfo {
	version: string;
	tagName: string;
	htmlUrl: string;
	installerUrl: string;
	prerelease?: boolean;
}

function hasTauriRuntime() {
	return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function getCurrentAppVersion() {
	if (!hasTauriRuntime()) return "dev";
	return getVersion();
}

export async function fetchLatestAppRelease(force = false): Promise<AppReleaseInfo> {
	if (!hasTauriRuntime()) throw new Error("Published release checks require the desktop app.");
	const release = await invoke<AppReleaseInfo>("fetch_latest_app_release", { force });
	const current = await getCurrentAppVersion();
	if (!release || !parseVersion(release.version) || release.version !== normalizeVersion(release.version) || !parseVersion(current)
		|| ![release.version, `v${release.version}`].includes(release.tagName)
		|| release.htmlUrl !== `${releaseBaseUrl}/tag/${release.tagName}`
		|| release.installerUrl !== `${releaseBaseUrl}/download/${release.tagName}/FXServer.Installer_${release.version}_windows_x64-setup.exe`
		|| (release.prerelease || releasePolicy.betaVersions.includes(release.version)) && !releasePolicy.betaVersions.includes(current)) {
		throw new Error("No verified published installer is available for this release channel.");
	}
	return release;
}

export function compareVersions(left: string, right: string) {
	const leftParts = parseVersion(left);
	const rightParts = parseVersion(right);
	if (!leftParts || !rightParts) return 0;

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
	if (typeof version !== "string") return null;
	const match = normalizeVersion(version).match(/^(0|[1-9]\d{0,8})\.(0|[1-9]\d{0,8})\.(0|[1-9]\d{0,8})$/);
	if (!match) return null;
	return [Number(match[1]), Number(match[2]), Number(match[3])];
}
