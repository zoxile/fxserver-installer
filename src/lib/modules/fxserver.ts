import { invoke } from "@tauri-apps/api/core";
import { log } from "$lib/core/logger.svelte";

export interface FxserverEnvironmentVariable {
	key: string;
	value: string;
}

export interface FxserverLaunchRequest {
	artifactPath: string;
	environment: FxserverEnvironmentVariable[];
	serverProfile?: string | null;
}

export interface FxserverRconConfig {
	host: string;
	port: number;
	password: string;
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

export interface FxserverTerminalEntry {
	id: number;
	stream: "stdout" | "stderr" | "system" | "command" | string;
	line: string;
	timestamp: string;
}

export interface FxserverTerminalResult {
	entries: FxserverTerminalEntry[];
}

export interface TxDataLogRequest {
	dataPath: string;
	profile?: string | null;
	logName: "fxserver.log" | "admin.log" | "server.log";
	maxLines?: number;
}

export interface TxDataLogResult {
	path: string;
	logName: string;
	content: string;
	lineCount: number;
}

export interface TxDataProfilesResult {
	dataPath: string;
	profiles: string[];
	hasRootLogs: boolean;
}

export interface ServerConfigRequest {
	txDataPath: string;
	profile: string;
}

export interface ServerConfigFile {
	name: string;
	path: string;
	content: string;
	size: number;
	modified?: number | null;
	hasRconPassword: boolean;
	hasRconlog: boolean;
}

export interface ServerConfigResult {
	txDataPath: string;
	profile: string;
	profileConfigPath: string;
	dataPath: string;
	files: ServerConfigFile[];
	rconPasswordFound: boolean;
	rconPasswordFile?: string | null;
	rconPasswordLine?: number | null;
	rconlogFound: boolean;
	rconlogLine?: number | null;
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

export async function getFxserverTerminal(maxLines = 500) {
	if (!hasTauriRuntime()) {
		return {
			entries: [],
		} satisfies FxserverTerminalResult;
	}

	try {
		return await invoke<FxserverTerminalResult>("get_fxserver_terminal", { maxLines });
	} catch (error) {
		log("FXServer terminal refresh failed.", {
			level: "error",
			scope: "fxserver.terminal",
			detail: errorMessage(error),
		});
		throw error;
	}
}

export async function sendFxserverCommand(command: string, rcon: FxserverRconConfig) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<void>();

	try {
		await invoke<void>("send_fxserver_command", { request: { command, rcon } });
		log("FXServer command sent.", {
			level: "debug",
			scope: "fxserver.terminal",
			detail: `${rcon.host}:${rcon.port} ${command}`,
		});
	} catch (error) {
		log("FXServer command failed.", {
			level: "error",
			scope: "fxserver.terminal",
			detail: errorMessage(error),
		});
		throw error;
	}
}

export async function readTxDataLog(request: TxDataLogRequest) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<TxDataLogResult>();

	try {
		const result = await invoke<TxDataLogResult>("read_txdata_log", { request });
		log(`Loaded ${result.logName}.`, {
			level: "success",
			scope: "fxserver.logs",
			detail: result.path,
		});
		return result;
	} catch (error) {
		log("txData log read failed.", {
			level: "error",
			scope: "fxserver.logs",
			detail: errorMessage(error),
		});
		throw error;
	}
}

export async function listTxDataProfiles(dataPath: string) {
	if (!hasTauriRuntime()) {
		return {
			dataPath,
			profiles: [],
			hasRootLogs: false,
		};
	}

	try {
		const result = await invoke<TxDataProfilesResult>("list_txdata_profiles", { dataPath });
		log(`Detected ${result.profiles.length} txData profile${result.profiles.length === 1 ? "" : "s"}.`, {
			level: "debug",
			scope: "fxserver.profiles",
			detail: result.dataPath,
		});
		return result;
	} catch (error) {
		log("txData profile scan failed.", {
			level: "error",
			scope: "fxserver.profiles",
			detail: errorMessage(error),
		});
		throw error;
	}
}

export async function readServerConfig(request: ServerConfigRequest) {
	if (!hasTauriRuntime()) {
		return {
			txDataPath: request.txDataPath,
			profile: request.profile,
			profileConfigPath: `${request.txDataPath}\\${request.profile}\\config.json`,
			dataPath: "",
			files: [],
			rconPasswordFound: false,
			rconPasswordFile: null,
			rconPasswordLine: null,
			rconlogFound: false,
			rconlogLine: null,
		} satisfies ServerConfigResult;
	}

	try {
		const result = await invoke<ServerConfigResult>("read_server_config", { request });
		log(`Loaded ${result.files.length} server config file${result.files.length === 1 ? "" : "s"}.`, {
			level: "success",
			scope: "fxserver.config",
			detail: result.dataPath,
		});
		return result;
	} catch (error) {
		log("Server config read failed.", {
			level: "error",
			scope: "fxserver.config",
			detail: errorMessage(error),
		});
		throw error;
	}
}

export async function saveServerConfig(path: string, content: string) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<ServerConfigFile>();

	try {
		const file = await invoke<ServerConfigFile>("save_server_config", { request: { path, content } });
		log(`Saved ${file.name}.`, {
			level: "success",
			scope: "fxserver.config",
			detail: file.path,
		});
		return file;
	} catch (error) {
		log("Server config save failed.", {
			level: "error",
			scope: "fxserver.config",
			detail: errorMessage(error),
		});
		throw error;
	}
}
