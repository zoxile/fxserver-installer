import { invoke } from "@tauri-apps/api/core";

export interface PlatformInfo {
	os: "windows" | "linux";
	arch: string;
	artifactPlatform: "windows" | "linux";
	clientLogsSupported: boolean;
	secureStorageSupported: boolean;
}

function browserPlatform(): PlatformInfo {
	const os = typeof navigator !== "undefined" && /linux/i.test(navigator.platform || navigator.userAgent) ? "linux" : "windows";
	return { os, arch: "unknown", artifactPlatform: os, clientLogsSupported: os === "windows", secureStorageSupported: os === "windows" };
}

export const platform = $state<PlatformInfo>(browserPlatform());
let initialization: Promise<PlatformInfo> | undefined;

export function initializePlatform(): Promise<PlatformInfo> {
	return initialization ??= (async () => {
		if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
			try {
				const info = await invoke<PlatformInfo>("get_platform_info");
				if (info && ["windows", "linux"].includes(info.os) && ["windows", "linux"].includes(info.artifactPlatform)
					&& typeof info.arch === "string" && typeof info.clientLogsSupported === "boolean" && typeof info.secureStorageSupported === "boolean") {
					Object.assign(platform, info);
				}
			} catch {
				// Older runtimes and browser previews retain the inexpensive navigator fallback.
			}
		}
		return platform;
	})();
}

export function platformLabel() {
	return platform.artifactPlatform === "linux" ? "Linux" : "Windows";
}

export function artifactListingUrl(edition: "legacy" | "enhanced" = "legacy") {
	return `https://docs.fivem.net/docs/server-download/?platform=${edition}&os=${platform.artifactPlatform}`;
}

export function serverPathPlaceholder() {
	return platform.os === "linux" ? "/home/fivem/server" : "C:\\FXServer\\server";
}

export function txDataPathPlaceholder() {
	return platform.os === "linux" ? "/home/fivem/txData" : "C:\\FiveM\\txData";
}
