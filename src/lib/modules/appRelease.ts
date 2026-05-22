import { getVersion } from "@tauri-apps/api/app";

const repository = "zoxile/fxserver-installer";
const latestReleaseUrl = `https://api.github.com/repos/${repository}/releases/latest`;

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
	const response = await fetch(latestReleaseUrl, {
		headers: {
			Accept: "application/vnd.github+json",
		},
	});

	if (!response.ok) {
		throw new Error(`GitHub returned ${response.status} while checking the latest app release.`);
	}

	const release = (await response.json()) as {
		tag_name?: string;
		html_url?: string;
		assets?: Array<{ name?: string; browser_download_url?: string }>;
	};
	const tagName = release.tag_name?.trim();
	if (!tagName) throw new Error("Latest app release did not include a tag.");

	const installer = release.assets?.find((asset) => {
		const name = asset.name?.toLowerCase() ?? "";
		return name.endsWith(".exe") && name.includes("setup");
	});

	return {
		version: normalizeVersion(tagName),
		tagName,
		htmlUrl: release.html_url ?? `https://github.com/${repository}/releases/tag/${tagName}`,
		installerUrl: installer?.browser_download_url ?? release.html_url ?? `https://github.com/${repository}/releases/tag/${tagName}`,
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
