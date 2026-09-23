import { invoke } from "@tauri-apps/api/core";
import { validateMariaDBCredentials, type MariaDBCredentials } from "$lib/modules/mariadb";

let storageQueue: Promise<unknown> = Promise.resolve();
let storageRevision = 0;
let pendingRemember = false;
let validationGeneration = 0;
let validation: { credentials: MariaDBCredentials; revision: number; pending: Promise<boolean> } | undefined;

export const databaseSession = $state<{
	credentials: MariaDBCredentials | null;
	validated: { credentials: MariaDBCredentials; revision: number } | null;
	validating: boolean;
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
	validated: null,
	validating: false,
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
	const changed = !databaseSession.credentials || !sameLogin(databaseSession.credentials, credentials);
	databaseSession.credentials = { ...credentials };
	databaseSession.connectionString = formatMariaDBConnectionString(credentials);
	databaseSession.defaults = { host: credentials.host, port: credentials.port, username: credentials.username, database: credentials.database ?? "" };
	window.dispatchEvent(new Event("workspace-settings-changed"));
	if ((changed || pendingRemember) && databaseSession.rememberLogin && isDatabaseSessionValidated(credentials)) {
		pendingRemember = false;
		void persistDatabaseLogin(credentials);
	}
	return true;
}

export function isDatabaseSessionValidated(credentials: MariaDBCredentials) {
	const validated = databaseSession.validated;
	return Boolean(validated && validated.revision === databaseSession.revision && sameAuthentication(validated.credentials, credentials));
}

export function invalidateDatabaseSession(credentials?: MariaDBCredentials) {
	if (credentials && !sameAuthentication(databaseSession.validated?.credentials ?? validation?.credentials, credentials)) return;
	validationGeneration++;
	validation = undefined;
	databaseSession.validated = null;
	databaseSession.validating = false;
}

export function handleDatabaseConnectionError(credentials: MariaDBCredentials, error: unknown) {
	const message = String(error).slice(0, 32768);
	if (/\b(?:error|code)\s*[:=(]?\s*(?:1045|1698|2002|2003|2005|2006|2013)\b/i.test(message)) {
		invalidateDatabaseSession(credentials);
	}
}

export function ensureDatabaseSession(credentials: MariaDBCredentials, force = false): Promise<boolean> {
	const revision = databaseSession.revision;
	if (!force && isDatabaseSessionValidated(credentials)) return Promise.resolve(true);
	if (validation?.revision === revision && sameAuthentication(validation.credentials, credentials)) return validation.pending;
	const snapshot = { ...credentials };
	const generation = ++validationGeneration;
	databaseSession.validated = null;
	databaseSession.validating = true;
	const current = () => revision === databaseSession.revision && generation === validationGeneration;
	// Authenticate the server login once; each operation still checks its database permissions.
	const pending = validateMariaDBCredentials({ ...snapshot, database: null }).then(() => {
		if (!current()) return false;
		databaseSession.validated = { credentials: snapshot, revision };
		rememberDatabaseCredentials(snapshot, revision);
		return true;
	}).finally(() => {
		if (current()) {
			databaseSession.validating = false;
			validation = undefined;
		}
	});
	validation = { credentials: snapshot, revision, pending };
	return pending;
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
			if (snapshot && databaseSession.rememberLogin) pendingRemember = true;
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
	pendingRemember = enabled && (!current || !isDatabaseSessionValidated(current));
	databaseSession.loginError = "";
	const validated = databaseSession.credentials;
	if (!enabled) void persistDatabaseLogin();
	else if (validated && current && sameLogin(validated, current) && isDatabaseSessionValidated(current)) void persistDatabaseLogin(validated);
}

function sameAuthentication(left: MariaDBCredentials | null | undefined, right: MariaDBCredentials) {
	return Boolean(left && left.host === right.host && Number(left.port) === Number(right.port)
		&& left.username === right.username && left.password === right.password);
}

function sameLogin(left: MariaDBCredentials, right: MariaDBCredentials) {
	return sameAuthentication(left, right) && (left.database ?? "") === (right.database ?? "");
}

export async function restoreDatabaseLogin(workspaceId: string) {
	invalidateDatabaseSession();
	pendingRemember = false;
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
