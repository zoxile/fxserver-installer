import { log } from "$lib/core/logger.svelte";
import { listTxDataProfiles } from "$lib/modules/fxserver";
import { publicEnvironment } from "$lib/core/workspaceSettings";

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
		localStorage.setItem(envStorageKey, JSON.stringify(savedEnv));
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
		return publicEnvironment(value);
	} catch {
		return {};
	}
}

export function writeSavedEnvironment(values: Record<string, string>) {
	localStorage.setItem(envStorageKey, JSON.stringify(publicEnvironment(values)));
	window.dispatchEvent(new Event("workspace-settings-changed"));
}

export function setTxDataPath(path: string) {
	loadFxserverSettings();
	if (fxserverSettings.txDataPath !== path.trim()) resetTxDataProfiles();
	fxserverSettings.txDataPath = path.trim();
	const savedEnvironment = readSavedEnvironment();
	if (fxserverSettings.txDataPath) {
		savedEnvironment.TXHOST_DATA_PATH = fxserverSettings.txDataPath;
	} else {
		delete savedEnvironment.TXHOST_DATA_PATH;
	}
	writeSavedEnvironment(savedEnvironment);
}

export function resetTxDataProfiles() {
	profileRequest += 1;
	fxserverSettings.profiles = [];
	fxserverSettings.hasRootLogs = false;
	fxserverSettings.profileError = "";
	fxserverSettings.loadingProfiles = false;
}

export function setServerProfile(profile: string) {
	loadFxserverSettings();
	fxserverSettings.profile = profile.trim();
	localStorage.setItem(profileStorageKey, fxserverSettings.profile);
	localStorage.removeItem(legacyLogProfileStorageKey);
	window.dispatchEvent(new Event("workspace-settings-changed"));
}

export async function refreshTxDataProfiles(artifactPath = "", discover = false) {
	loadFxserverSettings();
	const request = ++profileRequest;
	const path = fxserverSettings.txDataPath.trim();
	const selectedProfile = fxserverSettings.profile;
	fxserverSettings.profileError = "";
	fxserverSettings.profiles = [];
	fxserverSettings.hasRootLogs = false;

	if (!path && !artifactPath.trim()) { fxserverSettings.loadingProfiles = false; return; }

	fxserverSettings.loadingProfiles = true;
	try {
		const result = await listTxDataProfiles(path, { artifactPath, discover });
		if (request !== profileRequest || path !== fxserverSettings.txDataPath.trim()) return;
		// Only empty configurations or a newly chosen folder may be normalized.
		if ((!path || discover) && result.dataPath && result.dataPath !== path) {
			fxserverSettings.txDataPath = result.dataPath;
			writeSavedEnvironment({ ...readSavedEnvironment(), TXHOST_DATA_PATH: result.dataPath });
		}
		fxserverSettings.profiles = result.profiles;
		fxserverSettings.hasRootLogs = result.hasRootLogs;
		if (discover && result.selectedProfile && fxserverSettings.profile === selectedProfile) setServerProfile(result.selectedProfile);
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
