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
	plainLine?: string;
	segments?: FxserverTerminalSegment[];
	timestamp: string;
}

export interface FxserverTerminalSegment {
	text: string;
	color?: string | null;
	emphasis?: boolean | null;
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

export interface ResourceScanRequest {
	txDataPath: string;
	profile: string;
}

export interface FxserverResourceInfo {
	name: string;
	path: string;
	manifestPath: string;
	manifestName: string;
	version?: string | null;
	repository?: string | null;
}

export interface ResourceScanResult {
	txDataPath: string;
	profile: string;
	dataPath: string;
	resourceRoot: string;
	resources: FxserverResourceInfo[];
}

export interface ResourceUpdateRequest {
	resourcePath: string;
	repository: string;
	branch: string;
}

export interface ResourceUpdateResult {
	resourcePath: string;
	repository: string;
	branch: string;
	updatedAt: string;
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

export async function getFxserverStatus(options: { logResult?: boolean } = {}) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<FxserverStatus>();

	try {
		const status = await invoke<FxserverStatus>("get_fxserver_status");
		if (options.logResult) {
			log(status.running ? "FXServer status refreshed." : "FXServer is not running from this app.", {
				level: status.running ? "success" : "debug",
				scope: "fxserver.manage",
				detail: status.pid ? `PID ${status.pid}` : undefined,
			});
		}
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

export async function getFxserverTerminal(maxLines = 500, afterId: number | null = null) {
	if (!hasTauriRuntime()) {
		return {
			entries: [],
		} satisfies FxserverTerminalResult;
	}

	try {
		return await invoke<FxserverTerminalResult>("get_fxserver_terminal", { maxLines, afterId });
	} catch (error) {
		log("FXServer terminal refresh failed.", {
			level: "error",
			scope: "fxserver.terminal",
			detail: errorMessage(error),
		});
		throw error;
	}
}

export async function getSavedFxserverRconPassword() {
	if (!hasTauriRuntime()) return "";
	return (await invoke<string | null>("get_fxserver_rcon_password")) ?? "";
}

export async function saveFxserverRconPassword(password: string) {
	if (!hasTauriRuntime()) return;
	await invoke<void>("save_fxserver_rcon_password", { password });
}

export async function clearFxserverRconPassword() {
	if (!hasTauriRuntime()) return;
	await invoke<void>("clear_fxserver_rcon_password");
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

export async function sendFxserverRconCommand(command: string, rcon: FxserverRconConfig) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<string>();

	try {
		const output = await invoke<string>("send_fxserver_rcon_command", { request: { command, rcon } });
		log("FXServer RCON command sent.", {
			level: "debug",
			scope: "fxserver.resources",
			detail: `${rcon.host}:${rcon.port} ${command}`,
		});
		return output;
	} catch (error) {
		log("FXServer RCON command failed.", {
			level: "error",
			scope: "fxserver.resources",
			detail: errorMessage(error),
		});
		throw error;
	}
}

export async function scanFxserverResources(request: ResourceScanRequest) {
	if (!hasTauriRuntime()) {
		return {
			txDataPath: request.txDataPath,
			profile: request.profile,
			dataPath: "",
			resourceRoot: "",
			resources: [],
		} satisfies ResourceScanResult;
	}

	try {
		const result = await invoke<ResourceScanResult>("scan_fxserver_resources", { request });
		log(`Scanned ${result.resources.length} FXServer resources.`, {
			level: "success",
			scope: "fxserver.resources",
			detail: result.resourceRoot,
		});
		return result;
	} catch (error) {
		log("FXServer resource scan failed.", {
			level: "error",
			scope: "fxserver.resources",
			detail: errorMessage(error),
		});
		throw error;
	}
}

export async function updateGithubResource(request: ResourceUpdateRequest) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<ResourceUpdateResult>();

	try {
		const result = await invoke<ResourceUpdateResult>("update_github_resource", { request });
		log(`Updated resource from ${result.repository}.`, {
			level: "success",
			scope: "fxserver.resources",
			detail: result.resourcePath,
		});
		return result;
	} catch (error) {
		log("FXServer resource update failed.", {
			level: "error",
			scope: "fxserver.resources",
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
