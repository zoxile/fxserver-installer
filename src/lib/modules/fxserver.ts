import { invoke } from "@tauri-apps/api/core";
import { log } from "$lib/core/logger";

export interface FxserverEnvironmentVariable {
	key: string;
	value: string;
}

export interface FxserverLaunchRequest {
	artifactPath: string;
	environment: FxserverEnvironmentVariable[];
	serverProfile?: string | null;
}

export interface FxserverLaunchResult {
	pid: number;
	artifactPath: string;
	startedAt: string;
}

export interface FxserverResources {
	cpuPercent: number;
	memoryBytes: number;
	totalMemoryBytes: number;
	memoryPercent: number;
	threadCount: number;
	handleCount: number;
}

export interface FxserverStatus {
	running: boolean;
	pid?: number | null;
	artifactPath?: string | null;
	startedAt?: string | null;
	uptimeSeconds?: number | null;
	resources?: FxserverResources | null;
}

function hasTauriRuntime() {
	return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function unavailableOutsideTauri<T>(): Promise<T> {
	return Promise.reject(new Error("FXServer management is available in the Tauri desktop app."));
}

function errorMessage(error: unknown) {
	return error instanceof Error ? error.message : String(error);
}

export async function getFxserverStatus() {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<FxserverStatus>();

	try {
		const status = await invoke<FxserverStatus>("get_fxserver_status");
		log(status.running ? "FXServer status refreshed." : "FXServer is not running from this app.", {
			level: status.running ? "success" : "debug",
			scope: "fxserver.manage",
			detail: status.pid ? `PID ${status.pid}` : undefined,
		});
		return status;
	} catch (error) {
		log("FXServer status refresh failed.", {
			level: "error",
			scope: "fxserver.manage",
			detail: errorMessage(error),
		});
		throw error;
	}
}

export async function startFxserver(request: FxserverLaunchRequest) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<FxserverLaunchResult>();

	try {
		const result = await invoke<FxserverLaunchResult>("start_fxserver", { request });
		log("FXServer started.", {
			level: "success",
			scope: "fxserver.manage",
			detail: `${result.artifactPath} (PID ${result.pid})`,
		});
		return result;
	} catch (error) {
		log("FXServer start failed.", {
			level: "error",
			scope: "fxserver.manage",
			detail: errorMessage(error),
		});
		throw error;
	}
}

export async function stopFxserver() {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<void>();

	try {
		await invoke<void>("stop_fxserver");
		log("FXServer stopped.", {
			level: "success",
			scope: "fxserver.manage",
		});
	} catch (error) {
		log("FXServer stop failed.", {
			level: "error",
			scope: "fxserver.manage",
			detail: errorMessage(error),
		});
		throw error;
	}
}
