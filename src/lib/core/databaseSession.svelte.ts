import type { MariaDBCredentials } from "$lib/modules/mariadb";

export const databaseSession = $state<{
	credentials: MariaDBCredentials | null;
	connectionString: string;
}>({
	credentials: null,
	connectionString: "",
});

export function formatMariaDBConnectionString(credentials: MariaDBCredentials) {
	const username = encodeURIComponent(credentials.username.trim());
	const password = encodeURIComponent(credentials.password);
	const host = credentials.host.trim() || "127.0.0.1";
	const port = Number(credentials.port) || 3306;
	const database = credentials.database?.trim();
	return `mysql://${username}${password ? `:${password}` : ""}@${host}:${port}${database ? `/${encodeURIComponent(database)}` : ""}`;
}

export function rememberDatabaseCredentials(credentials: MariaDBCredentials) {
	databaseSession.credentials = { ...credentials };
	databaseSession.connectionString = formatMariaDBConnectionString(credentials);
}
