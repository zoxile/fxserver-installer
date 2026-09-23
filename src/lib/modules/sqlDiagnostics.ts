import { invoke } from "@tauri-apps/api/core";
import { trackTask } from "$lib/core/tasks.svelte";
import type { MariaDBCredentials } from "./mariadb";

export interface SqlDiagnosis { title: string; explanation: string; checks: string[] }
export interface SqlInspection {
	environment: { version: string; sqlMode: string; charset: string; collation: string; schemaCollation: string | null };
	columns: { table: string; column: string; columnType: string; charset: string | null; collation: string | null; engine: string | null }[];
	foreignKeys: { constraint: string; column: string; parentDatabase: string; parentTable: string; parentColumn: string; parentType: string | null; parentCollation: string | null }[];
	truncated: boolean;
}

const explanations: [number[], string, string, string[]][] = [
	[[1005, 1215], "Foreign key or table definition rejected", "The server could not create this table or relationship. The referenced columns or table definition may not match.", ["Inspect each table named in the error. Compare both column types, including integer size and UNSIGNED, and text character sets and collations.", "Check that the parent table exists, uses a compatible engine, and has an index beginning with the referenced columns.", "Review the exact server error: error 1005 can also have causes unrelated to foreign keys."]],
	[[1451, 1452], "A foreign key protects related rows", "This change would leave a row without the related row it depends on.", ["For an insert, confirm the parent row exists. For a delete or update, inspect dependent rows first. Schema inspection shows definitions, not row data.", "Do not disable foreign-key checks to hide the failure. Choose a migration that preserves the relationship."]],
	[[1267, 1271], "Incompatible text collations", "The statement compares or combines text with comparison rules the server cannot use together.", ["Inspect the columns used in the failing comparison. Compare their character sets and collations, then check the failing connection's settings.", "Different collations elsewhere in a database are not automatically a problem. Inspection uses its own connection, which may differ from your resource's connection.", "Review conversions on a backup first. Changing a database default does not convert existing columns, and conversions can change unique-key comparisons."]],
	[[1273, 1115], "Unsupported collation or character set", "The script names a collation or character set this server does not recognize.", ["Use Server details to check the server version, then check that version's documentation for supported character sets and collations. Inspection does not list every supported option.", "Review the import's source database version. Choose a compatible collation deliberately instead of replacing names throughout the whole file."]],
	[[1044, 1045, 1142], "Connection or privilege rejected", "The server rejected the login or this account is not allowed to perform the requested operation.", ["Verify the username, host, password and the account's grants for this database, then validate the connection above.", "Accounts such as user@localhost and user@127.0.0.1 can have different passwords and permissions."]],
	[[1046, 1049], "Database scope is missing or unavailable", "The statement has no selected database, or names one the server cannot open.", ["Choose an existing database in the execution scope, or review the script's CREATE DATABASE and USE statements.", "Confirm this account can see the database. For schema inspection, select the database in the scope above; a USE statement in the editor does not select it here."]],
	[[1054, 1146], "Column or table was not found", "The statement refers to a column or table the server could not find in this context.", ["Check the selected database, identifier spelling, case sensitivity, and required earlier migrations.", "Inspect the named table and compare its column names with the failing statement before changing a resource's queries."]],
	[[1062], "A unique key already contains this value", "This change would duplicate a value that a primary or unique key requires to be unique.", ["Inspect the primary/unique index and the existing row separately. Schema inspection does not show row data or unique indexes. Decide whether the data should be updated or kept.", "Do not blindly replace INSERT with REPLACE: replacement can delete existing rows and affect related records."]],
	[[1064], "SQL syntax rejected", "The server could not parse part of the statement. Schema inspection alone cannot validate SQL syntax.", ["Review the statement around the reported line, including quotes, delimiters, and reserved identifiers.", "Check whether the script targets another MySQL/MariaDB version. The app does not rewrite or retry failed statements."]],
	[[1205, 1213], "Lock timeout or deadlock", "The statement could not obtain a lock, or the server stopped a transaction to resolve competing locks.", ["Inspect active transactions separately and avoid concurrent migrations. Schema inspection does not show locks.", "A timeout or deadlock can leave a script partially applied. Check transaction boundaries and committed changes before a deliberate retry."]],
	[[1366, 1406, 1264], "Value does not fit the column", "A value cannot be stored as supplied because of its encoding, length, type or numeric range.", ["Inspect the destination table and compare the column type and character set with the value being written.", "Keep strict validation enabled. Do not truncate or silently coerce data to make an import pass."]],
	[[2002, 2003, 2006, 2013], "Connection failed or was lost", "The app could not reach the server, or the connection ended before the operation completed.", ["Check service state, address, port and network access, then validate the connection above.", "After a lost connection, the result of a write may be unknown. Check the database before retrying."]],
];

export function diagnoseSqlError(message: string): SqlDiagnosis {
	const bounded = message.slice(0, 32_768);
	const code = bounded.match(/\bERROR\s+(\d{4})\b/i) ?? bounded.match(/\berror\s*[:(]?\s*(\d{4})\b/i);
	const match = code ? explanations.find(([codes]) => codes.includes(Number(code[1]))) : undefined;
	return match ? { title: `${code![1]}: ${match[1]}`, explanation: match[2], checks: match[3] } : {
		title: "Review the SQL error",
		explanation: "No recognized MariaDB error code was found. Use the original error and the checks below to narrow down the cause.",
		checks: ["Check the reported statement, execution scope and the server version.", "Failed scripts can be partially applied, especially when they contain DDL. Review existing changes before running the script again."],
	};
}

export function sqlInspectionAvailability(database: string, credentialsReady: boolean, disabled = false) {
	if (!credentialsReady) return { state: "unvalidated", message: "Validate the connection above before inspecting a database. Error explanations are available without a validated connection." };
	if (disabled) return { state: "busy", message: "Wait for the current database operation to finish before inspecting." };
	if (database === "__global__") return { state: "global", message: "Global scope has no selected database. Choose a database in the scope above to inspect its tables. A USE statement in your SQL does not change this selection." };
	if (!database.trim()) return { state: "no-database", message: "Choose a database in the scope above to inspect its tables." };
	return { state: "ready", message: "" };
}

export function summarizeSqlInspection(report: SqlInspection, table: string) {
	const columnCount = report.columns.length;
	const tableCount = new Set(report.columns.map((column) => column.table)).size;
	const charsets = [...new Set(report.columns.flatMap((column) => column.charset ? [column.charset] : []))];
	const collations = [...new Set(report.columns.flatMap((column) => column.collation ? [column.collation] : []))];
	return {
		columnCount, tableCount, charsets, collations,
		title: report.truncated ? "Partial schema inspection" : columnCount ? "Schema inspection complete" : "No visible table columns found",
		description: `${columnCount} visible column${columnCount === 1 ? "" : "s"} across ${tableCount} table${tableCount === 1 ? "" : "s"}${report.truncated ? " in this limited result" : ""}.`,
		nextStep: columnCount
			? "Open Column details and compare the names, types, character sets and collations with the failing statement. This is a metadata snapshot, not a pass/fail test of your SQL."
			: table.trim()
				? "Check the exact table name and selected database, or clear the optional table field to inspect all visible tables. The table may be missing, a view, or hidden from this account."
				: "This database may be empty or its tables may be hidden from this account. Check the selected database and the account's permissions. Views are not included.",
	};
}

export function inspectDatabaseSql(credentials: MariaDBCredentials, database: string, table: string) {
	if (!database.trim() || database === "__global__") return Promise.reject(new Error("Choose a database before inspecting its schema."));
	return trackTask("inspect_database_sql", "Inspect SQL compatibility", () => invoke<SqlInspection>("inspect_database_sql", { credentials: { ...credentials }, database, table: table.trim() || null }));
}
