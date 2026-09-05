import { taskInvoke as invoke } from "$lib/core/tasks.svelte";
import { downloadDir } from "@tauri-apps/api/path";
import { listen } from "@tauri-apps/api/event";
import { log } from "$lib/core/logger.svelte";
import { mariadbActivity } from "$lib/core/mariadbActivity.svelte";

export interface MariaDBStatus {
	installed: boolean;
	running: boolean;
	version: string | null;
	serviceName: string | null;
	serviceDisplayName: string | null;
	installPath: string | null;
}

export interface MariaDBCredentials {
	host: string;
	port: number;
	username: string;
	password: string;
	database?: string | null;
}

export interface MariaDBInstallOptions {
	rootPassword: string;
	serviceName: string;
	port: number;
	installDir?: string | null;
	dataDir?: string | null;
	allowRemoteRootAccess: boolean;
	createAnonymousUser: boolean;
	skipNetworking: boolean;
	optimizeForTransactions: boolean;
	useUtf8: boolean;
	pageSize?: string | null;
	bufferPoolSize?: string | null;
	installHeidiSql: boolean;
	installDevelopmentFiles: boolean;
}

export interface MariaDBPackageInfo {
	latestVersion: string | null;
	installedPackageVersion: string | null;
	updateAvailable: boolean;
}

export interface MariaDBUserConfig {
	username: string;
	password: string;
	host: string;
	database?: string | null;
	privileges: string[];
}

export interface MariaDBUserUpdateConfig {
	username: string;
	host: string;
	password?: string | null;
	database?: string | null;
	privileges: string[];
}

export interface MariaDBUser {
	username: string;
	host: string;
	plugin?: string | null;
	passwordExpired?: string | null;
	locked?: string | null;
}

export interface MariaDBUserPrivilege {
	database: string;
	table?: string | null;
	privilege: string;
	grantable: string;
}

export interface MariaDBUserAccess {
	username: string;
	host: string;
	grants: string[];
	schemaPrivileges: MariaDBUserPrivilege[];
	tablePrivileges: MariaDBUserPrivilege[];
}

export interface MariaDBQueryResult {
	success: boolean;
	stdout: string;
	stderr: string;
	columns: string[];
	rows: string[][];
}

export interface MariaDBBackupOptions {
	outputDir: string;
	fileName?: string | null;
	database?: string | null;
	tables: string[];
	allDatabases: boolean;
	schemaOnly: boolean;
	dataOnly: boolean;
	includeRoutines: boolean;
	includeTriggers: boolean;
	includeEvents: boolean;
	singleTransaction: boolean;
	addDropStatements: boolean;
	whereClause?: string | null;
}

export interface MariaDBBackupResult {
	path: string;
	sizeBytes: number;
	stderr: string;
}

let cachedStatus: MariaDBStatus | null = null;

function hasTauriRuntime() {
	return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function getDefaultMariaDBBackupOutputDir() {
	if (!hasTauriRuntime()) return "";

	try {
		return await downloadDir();
	} catch (error) {
		log("Could not resolve the default MariaDB backup folder.", { level: "warn", scope: "mariadb.backup", detail: errorMessage(error) });
		return "";
	}
}

function unavailableOutsideTauri<T>(): Promise<T> {
	log("MariaDB action blocked outside the desktop runtime.", { level: "warn", scope: "mariadb.runtime" });
	return Promise.reject(new Error("MariaDB actions are available in the Tauri desktop app."));
}

function errorMessage(error: unknown) {
	return error instanceof Error ? error.message : String(error);
}

async function invokeMariaDB<T>(command: string, args: Record<string, unknown>, action: string, success: (value: T) => string) {
	log(`${action} started.`, { scope: "mariadb" });

	try {
		const value = await invoke<T>(command, args);
		log(success(value), { level: "success", scope: "mariadb" });
		return value;
	} catch (error) {
		log(`${action} failed.`, { level: "error", scope: "mariadb", detail: errorMessage(error) });
		throw error;
	}
}

function cacheStatus(status: MariaDBStatus) {
	cachedStatus = status;
	return status;
}

function browserPreviewStatus(): MariaDBStatus {
	return {
		installed: false,
		running: false,
		version: null,
		serviceName: null,
		serviceDisplayName: null,
		installPath: null,
	};
}

function browserPreviewPackageInfo(): MariaDBPackageInfo {
	return {
		latestVersion: null,
		installedPackageVersion: null,
		updateAvailable: false,
	};
}

export function getMariaDBStatus(force = false) {
	if (!force && cachedStatus) {
		log("MariaDB status restored from the current app session cache.", { level: "debug", scope: "mariadb.status" });
		return Promise.resolve(cachedStatus);
	}

	if (!hasTauriRuntime()) {
		log("MariaDB status requested in browser preview.", { level: "debug", scope: "mariadb.status" });
		return Promise.resolve(cacheStatus(browserPreviewStatus()));
	}

	return invokeMariaDB<MariaDBStatus>("get_mariadb_status", {}, force ? "MariaDB status refresh" : "MariaDB initial status load", (status) =>
		status.installed ? `MariaDB detected${status.version ? `: ${status.version}` : "."}` : "MariaDB is not installed.",
	).then(cacheStatus);
}

export function installMariaDB(options: MariaDBInstallOptions) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<string>();
	return runInstaller("install_mariadb", { options }, "MariaDB install");
}

async function runInstaller(command: string, args: Record<string, unknown>, action: string) {
	if (mariadbActivity.busy) throw new Error("A MariaDB installation action is already running.");
	mariadbActivity.busy = true;
	mariadbActivity.stage = `${action} is preparing...`;
	let unlisten: (() => void) | undefined;
	try {
		unlisten = await listen<string>("mariadb-progress", ({ payload }) => {
			mariadbActivity.stage = payload;
			log(payload, { scope: "mariadb.install" });
		});
		const result = await invokeMariaDB<string>(command, args, action, (output) => output.trim() || `${action} completed.`);
		cachedStatus = null;
		mariadbActivity.stage = result;
		return result;
	} catch (error) {
		mariadbActivity.stage = errorMessage(error);
		throw error;
	} finally {
		unlisten?.();
		mariadbActivity.busy = false;
	}
}

export function getMariaDBPackageInfo() {
	if (!hasTauriRuntime()) {
		log("MariaDB package info requested in browser preview.", { level: "debug", scope: "mariadb.package" });
		return Promise.resolve(browserPreviewPackageInfo());
	}

	return invokeMariaDB<MariaDBPackageInfo>("get_mariadb_package_info", {}, "MariaDB package info refresh", (info) =>
		info.latestVersion ? `Recommended MariaDB version is ${info.latestVersion}.` : "MariaDB package info refreshed.",
	);
}

export function uninstallMariaDB() {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<string>();
	return runInstaller("uninstall_mariadb", {}, "MariaDB uninstall");
}

export function updateMariaDB() {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<string>();
	return runInstaller("update_mariadb", {}, "MariaDB update");
}

export function startMariaDBService(serviceName?: string | null) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<MariaDBStatus>();
	return invokeMariaDB<MariaDBStatus>("start_mariadb_service", { serviceName }, "MariaDB service start", (status) => `MariaDB service is ${status.running ? "running" : "not running"}.`).then(
		cacheStatus,
	);
}

export function stopMariaDBService(serviceName?: string | null) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<MariaDBStatus>();
	return invokeMariaDB<MariaDBStatus>("stop_mariadb_service", { serviceName }, "MariaDB service stop", (status) => `MariaDB service is ${status.running ? "running" : "stopped"}.`).then(cacheStatus);
}

export function restartMariaDBService(serviceName?: string | null) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<MariaDBStatus>();
	return invokeMariaDB<MariaDBStatus>("restart_mariadb_service", { serviceName }, "MariaDB service restart", (status) => `MariaDB service restart completed; running=${status.running}.`).then(
		cacheStatus,
	);
}

export function executeMariaDBQuery(credentials: MariaDBCredentials, query: string) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<MariaDBQueryResult>();
	return invokeMariaDB<MariaDBQueryResult>(
		"execute_mariadb_query",
		{ credentials, query },
		"MariaDB query execution",
		(result) => `MariaDB query ${result.success ? "succeeded" : "returned an error"} with ${result.rows.length} rows.`,
	);
}

export function validateMariaDBCredentials(credentials: MariaDBCredentials) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<void>();
	return invokeMariaDB<void>("validate_mariadb_credentials", { credentials }, "MariaDB credential validation", () => "MariaDB credentials are valid.");
}

export function listMariaDBDatabases(credentials: MariaDBCredentials) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<string[]>();
	return invokeMariaDB<string[]>("list_mariadb_databases", { credentials }, "MariaDB database list refresh", (databases) => `MariaDB returned ${databases.length} databases.`);
}

export function listMariaDBTables(credentials: MariaDBCredentials, database: string) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<string[]>();
	return invokeMariaDB<string[]>("list_mariadb_tables", { credentials, database }, `MariaDB table list refresh for ${database}`, (tables) => `MariaDB returned ${tables.length} tables.`);
}

export function backupMariaDB(credentials: MariaDBCredentials, options: MariaDBBackupOptions) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<MariaDBBackupResult>();
	return invokeMariaDB<MariaDBBackupResult>("backup_mariadb", { credentials, options }, "MariaDB backup", (result) => `Backup created at ${result.path}.`);
}

export function saveMariaDBUser(credentials: MariaDBCredentials, config: MariaDBUserConfig) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<void>();
	return invokeMariaDB<void>("save_mariadb_user", { credentials, config }, `MariaDB user save for ${config.username}@${config.host}`, () => "MariaDB user saved.");
}

export function listMariaDBUsers(credentials: MariaDBCredentials) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<MariaDBUser[]>();
	return invokeMariaDB<MariaDBUser[]>("list_mariadb_users", { credentials }, "MariaDB user list refresh", (users) => `MariaDB returned ${users.length} users.`);
}

export function updateMariaDBUser(credentials: MariaDBCredentials, config: MariaDBUserUpdateConfig) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<void>();
	return invokeMariaDB<void>("update_mariadb_user", { credentials, config }, `MariaDB user update for ${config.username}@${config.host}`, () => "MariaDB user updated.");
}

export function getMariaDBUserAccess(credentials: MariaDBCredentials, username: string, host: string) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<MariaDBUserAccess>();
	return invokeMariaDB<MariaDBUserAccess>(
		"get_mariadb_user_access",
		{ credentials, username, host },
		`MariaDB access refresh for ${username}@${host}`,
		(access) => `Loaded ${access.grants.length} grants for ${username}@${host}.`,
	);
}

export function deleteMariaDBUser(credentials: MariaDBCredentials, username: string, host: string) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<void>();
	return invokeMariaDB<void>("delete_mariadb_user", { credentials, username, host }, `MariaDB user delete for ${username}@${host}`, () => "MariaDB user deleted.");
}
