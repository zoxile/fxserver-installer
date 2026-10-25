import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import appPackage from "../../../package.json";
import { trackExclusiveTask, trackTask } from "./tasks.svelte";

export type UpdatePhase = "disabled" | "idle" | "checking" | "available" | "downloading" | "ready" | "stopping" | "installing" | "error";
export interface AppUpdateStatus {
	phase: UpdatePhase;
	currentVersion: string;
	availableVersion: string | null;
	releaseUrl: string | null;
	downloadedBytes: number;
	totalBytes: number | null;
	message: string | null;
	supported: boolean;
}

export const updatePhaseBusy = (phase: UpdatePhase) => ["checking", "downloading", "stopping", "installing"].includes(phase);

export const appUpdate = $state({
	status: { phase: "disabled", currentVersion: appPackage.version, availableVersion: null, releaseUrl: null,
		downloadedBytes: 0, totalBytes: null, message: null, supported: false } as AppUpdateStatus,
	loading: true,
	working: false,
	error: "",
});
let initialization: Promise<void> | undefined;

export function initializeAppUpdate(): Promise<void> {
	return initialization ??= (async () => {
		if (!("__TAURI_INTERNALS__" in window)) {
			appUpdate.status.message = "In-app updates are available in installed release builds.";
			appUpdate.loading = false;
			return;
		}
		try {
			await listen<AppUpdateStatus>("app-update-progress", ({ payload }) => { appUpdate.status = payload; });
			appUpdate.status = await invoke<AppUpdateStatus>("get_app_update_status");
			if (appUpdate.status.supported && appUpdate.status.phase === "idle") await checkForAppUpdate(false);
		} catch (error) { appUpdate.error = String(error); }
		finally { appUpdate.loading = false; }
	})();
}

export async function checkForAppUpdate(force = true) {
	if (appUpdate.working || updatePhaseBusy(appUpdate.status.phase) || !appUpdate.status.supported) return;
	appUpdate.working = true;
	appUpdate.error = "";
	try { appUpdate.status = await invoke<AppUpdateStatus>("check_app_update", { force }); }
	catch (error) { appUpdate.error = String(error); }
	finally { appUpdate.working = false; }
}

export async function updateAndRestart() {
	if (appUpdate.working || updatePhaseBusy(appUpdate.status.phase) || !appUpdate.status.supported || !appUpdate.status.availableVersion) return;
	appUpdate.working = true;
	appUpdate.error = "";
	try {
		if (appUpdate.status.phase !== "ready") {
			appUpdate.status = await trackTask("download_app_update", "Download signed app update", () => invoke<AppUpdateStatus>("download_app_update"));
		}
		appUpdate.status = await trackExclusiveTask("install_app_update", "Install app update", () => invoke<AppUpdateStatus>("install_app_update"));
	} catch (error) { appUpdate.error = String(error); }
	finally { appUpdate.working = false; }
}
