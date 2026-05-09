import { invoke } from "@tauri-apps/api/core";

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

export interface MariaDBUserConfig {
	username: string;
	password: string;
	host: string;
	database?: string | null;
	privileges: string[];
}

export interface MariaDBQueryResult {
	success: boolean;
	stdout: string;
	stderr: string;
	columns: string[];
	rows: string[][];
}

function hasTauriRuntime() {
	return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function unavailableOutsideTauri<T>(): Promise<T> {
	return Promise.reject(new Error("MariaDB actions are available in the Tauri desktop app."));
}

export function getMariaDBStatus() {
	if (!hasTauriRuntime()) {
		return Promise.resolve({
			installed: false,
			running: false,
			version: null,
			serviceName: null,
			serviceDisplayName: null,
			installPath: null,
		});
	}

	return invoke<MariaDBStatus>("get_mariadb_status");
}

export function installMariaDB(options: MariaDBInstallOptions) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<string>();
	return invoke<string>("install_mariadb", { options });
}

export function startMariaDBService(serviceName?: string | null) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<MariaDBStatus>();
	return invoke<MariaDBStatus>("start_mariadb_service", { serviceName });
}

export function stopMariaDBService(serviceName?: string | null) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<MariaDBStatus>();
	return invoke<MariaDBStatus>("stop_mariadb_service", { serviceName });
}

export function restartMariaDBService(serviceName?: string | null) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<MariaDBStatus>();
	return invoke<MariaDBStatus>("restart_mariadb_service", { serviceName });
}

export function executeMariaDBQuery(credentials: MariaDBCredentials, query: string) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<MariaDBQueryResult>();
	return invoke<MariaDBQueryResult>("execute_mariadb_query", { credentials, query });
}

export function saveMariaDBUser(credentials: MariaDBCredentials, config: MariaDBUserConfig) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<void>();
	return invoke<void>("save_mariadb_user", { credentials, config });
}

export function deleteMariaDBUser(credentials: MariaDBCredentials, username: string, host: string) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<void>();
	return invoke<void>("delete_mariadb_user", { credentials, username, host });
}
