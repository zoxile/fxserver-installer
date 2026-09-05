import type { MariaDBCredentials } from "$lib/modules/mariadb";

export const databaseSession = $state<{
	credentials: MariaDBCredentials | null;
	connectionString: string;
	revision: number;
	defaults: { host: string; port: number; username: string; database: string };
}>({
	credentials: null,
	connectionString: "",
	revision: 0,
	defaults: { host: "localhost", port: 3306, username: "root", database: "" },
});

export function formatMariaDBConnectionString(credentials: MariaDBCredentials) {
	const username = encodeURIComponent(credentials.username.trim());
	const password = encodeURIComponent(credentials.password);
	const host = credentials.host.trim() || "localhost";
	const port = Number(credentials.port) || 3306;
	const database = credentials.database?.trim();
	return `mysql://${username}${password ? `:${password}` : ""}@${host}:${port}${database ? `/${encodeURIComponent(database)}` : ""}`;
}

export function rememberDatabaseCredentials(credentials: MariaDBCredentials, revision = databaseSession.revision) {
	if (revision !== databaseSession.revision) return false;
	databaseSession.credentials = { ...credentials };
	databaseSession.connectionString = formatMariaDBConnectionString(credentials);
	databaseSession.defaults = { host: credentials.host, port: credentials.port, username: credentials.username, database: credentials.database ?? "" };
	window.dispatchEvent(new Event("workspace-settings-changed"));
	return true;
}
