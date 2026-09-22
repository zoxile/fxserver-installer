import { invoke } from "@tauri-apps/api/core";
import type { MariaDBCredentials } from "$lib/modules/mariadb";

let storageQueue: Promise<unknown> = Promise.resolve();
let storageRevision = 0;

export const databaseSession = $state<{
	credentials: MariaDBCredentials | null;
	connectionString: string;
	revision: number;
	workspaceId: string;
	rememberLogin: boolean;
	restoringLogin: boolean;
	savingLogin: boolean;
	loginError: string;
	defaults: { host: string; port: number; username: string; database: string };
}>({
	credentials: null,
	connectionString: "",
	revision: 0,
	workspaceId: "default",
	rememberLogin: false,
	restoringLogin: false,
	savingLogin: false,
	loginError: "",
	defaults: { host: "localhost", port: 3306, username: "root", database: "" },
});

export function formatMariaDBConnectionString(credentials: MariaDBCredentials) {
	const username = encodeURIComponent(credentials.username.trim());
	const password = encodeURIComponent(credentials.password);
	const host = credentials.host.trim() || "localhost";
	const address = host.includes(":") && !host.startsWith("[") ? `[${host}]` : host;
	const port = Number(credentials.port) || 3306;
	const database = credentials.database?.trim();
	return `mysql://${username}${password ? `:${password}` : ""}@${address}:${port}${database ? `/${encodeURIComponent(database)}` : ""}`;
}

export function rememberDatabaseCredentials(credentials: MariaDBCredentials, revision = databaseSession.revision) {
	if (revision !== databaseSession.revision) return false;
	databaseSession.credentials = { ...credentials };
	databaseSession.connectionString = formatMariaDBConnectionString(credentials);
	databaseSession.defaults = { host: credentials.host, port: credentials.port, username: credentials.username, database: credentials.database ?? "" };
	window.dispatchEvent(new Event("workspace-settings-changed"));
	if (databaseSession.rememberLogin) void persistDatabaseLogin(credentials);
	return true;
}

function serializeStorage<T>(action: () => Promise<T>): Promise<T> {
	const pending = storageQueue.then(action, action);
	storageQueue = pending.catch(() => undefined);
	return pending;
}

export async function forgetDatabaseLogin(workspaceId: string) {
	if (!("__TAURI_INTERNALS__" in window)) return;
	await serializeStorage(() => invoke("clear_database_login", { workspaceId }));
}

async function persistDatabaseLogin(credentials?: MariaDBCredentials) {
	const workspaceId = databaseSession.workspaceId;
	const revision = databaseSession.revision;
	const request = ++storageRevision;
	const snapshot = credentials ? { ...credentials } : undefined;
	databaseSession.savingLogin = true;
	databaseSession.loginError = "";
	try {
		if ("__TAURI_INTERNALS__" in window) {
			await serializeStorage(() => invoke(snapshot ? "save_database_login" : "clear_database_login", { workspaceId, ...(snapshot ? { credentials: snapshot } : {}) }));
		}
	} catch {
		if (revision === databaseSession.revision && request === storageRevision) {
			databaseSession.loginError = snapshot
				? "Could not save this login. Validate the connection and try again."
				: "Could not forget the saved login. Try again before closing the app.";
		}
	} finally {
		if (revision === databaseSession.revision && request === storageRevision) databaseSession.savingLogin = false;
	}
}

export function setRememberDatabaseLogin(enabled: boolean, current?: MariaDBCredentials) {
	databaseSession.rememberLogin = enabled;
	databaseSession.loginError = "";
	const validated = databaseSession.credentials;
	if (!enabled) void persistDatabaseLogin();
	else if (validated && current && sameLogin(validated, current)) void persistDatabaseLogin(validated);
}

function sameLogin(left: MariaDBCredentials, right: MariaDBCredentials) {
	return left.host === right.host && Number(left.port) === Number(right.port) && left.username === right.username
		&& left.password === right.password && (left.database ?? "") === (right.database ?? "");
}

export async function restoreDatabaseLogin(workspaceId: string) {
	const revision = databaseSession.revision;
	databaseSession.workspaceId = workspaceId;
	databaseSession.rememberLogin = false;
	databaseSession.loginError = "";
	databaseSession.savingLogin = false;
	if (!("__TAURI_INTERNALS__" in window)) return;
	databaseSession.restoringLogin = true;
	try {
		const credentials = await serializeStorage(() => invoke<MariaDBCredentials | null>("load_database_login", { workspaceId }));
		if (revision !== databaseSession.revision) return;
		if (credentials) {
			databaseSession.rememberLogin = true;
			if (!databaseSession.credentials) {
				databaseSession.credentials = credentials;
				databaseSession.connectionString = formatMariaDBConnectionString(credentials);
			}
		}
	} catch {
		if (revision === databaseSession.revision) {
			databaseSession.loginError = "Saved login could not be unlocked. Enter your credentials again, or forget the saved login.";
		}
	} finally {
		if (revision === databaseSession.revision) databaseSession.restoringLogin = false;
	}
}
