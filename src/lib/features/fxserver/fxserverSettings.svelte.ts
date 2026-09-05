import { log } from "$lib/core/logger.svelte";
import { listTxDataProfiles } from "$lib/modules/fxserver";

const envStorageKey = "fxserver.manage.env";
const profileStorageKey = "fxserver.manage.serverProfile";
const legacyLogProfileStorageKey = "fxserver.manage.logProfile";

export const fxserverSettings = $state({
	txDataPath: "",
	profile: "",
	profiles: [] as string[],
	hasRootLogs: false,
	loadingProfiles: false,
	profileError: "",
});

let loaded = false;
let profileRequest = 0;

export function loadFxserverSettings() {
	if (loaded) return;
	loaded = true;

	try {
		const savedEnv = readSavedEnvironment();
		fxserverSettings.txDataPath = typeof savedEnv.TXHOST_DATA_PATH === "string" ? savedEnv.TXHOST_DATA_PATH : "";
		fxserverSettings.profile = localStorage.getItem(profileStorageKey) ?? localStorage.getItem(legacyLogProfileStorageKey) ?? "";
	} catch {
		fxserverSettings.txDataPath = "";
		fxserverSettings.profile = "";
	}
}

export function readSavedEnvironment() {
	try {
		const value = JSON.parse(localStorage.getItem(envStorageKey) || "{}");
		if (!value || typeof value !== "object" || Array.isArray(value)) return {};
		return Object.fromEntries(Object.entries(value).filter((entry): entry is [string, string] => typeof entry[1] === "string"));
	} catch {
		return {};
	}
}

export function writeSavedEnvironment(values: Record<string, string>) {
	localStorage.setItem(envStorageKey, JSON.stringify(values));
	window.dispatchEvent(new Event("workspace-settings-changed"));
}

export function setTxDataPath(path: string) {
	loadFxserverSettings();
	fxserverSettings.txDataPath = path.trim();
	const savedEnvironment = readSavedEnvironment();
	if (fxserverSettings.txDataPath) {
		savedEnvironment.TXHOST_DATA_PATH = fxserverSettings.txDataPath;
	} else {
		delete savedEnvironment.TXHOST_DATA_PATH;
	}
	writeSavedEnvironment(savedEnvironment);
}

export function setServerProfile(profile: string) {
	loadFxserverSettings();
	fxserverSettings.profile = profile.trim();
	localStorage.setItem(profileStorageKey, fxserverSettings.profile);
	localStorage.removeItem(legacyLogProfileStorageKey);
	window.dispatchEvent(new Event("workspace-settings-changed"));
}

export async function refreshTxDataProfiles() {
	loadFxserverSettings();
	const request = ++profileRequest;
	const path = fxserverSettings.txDataPath.trim();
	fxserverSettings.profileError = "";
	fxserverSettings.profiles = [];
	fxserverSettings.hasRootLogs = false;

	if (!path) { fxserverSettings.loadingProfiles = false; return; }

	fxserverSettings.loadingProfiles = true;
	try {
		const result = await listTxDataProfiles(path);
		if (request !== profileRequest || path !== fxserverSettings.txDataPath.trim()) return;
		fxserverSettings.profiles = result.profiles;
		fxserverSettings.hasRootLogs = result.hasRootLogs;
		if (fxserverSettings.profile && !result.profiles.includes(fxserverSettings.profile)) {
			setServerProfile("");
		}
	} catch (error) {
		if (request !== profileRequest || path !== fxserverSettings.txDataPath.trim()) return;
		fxserverSettings.profileError = error instanceof Error ? error.message : String(error);
		log("Could not refresh txData profiles.", {
			level: "error",
			scope: "fxserver.settings",
			detail: fxserverSettings.profileError,
		});
	} finally {
		if (request === profileRequest) fxserverSettings.loadingProfiles = false;
	}
}
