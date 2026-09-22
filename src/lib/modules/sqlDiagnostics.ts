import { invoke } from "@tauri-apps/api/core";
import { trackTask } from "$lib/core/tasks.svelte";
import type { MariaDBCredentials } from "./mariadb";

export interface SqlDiagnosis { title: string; checks: string[] }
export interface SqlInspection {
	environment: { version: string; sqlMode: string; charset: string; collation: string; schemaCollation: string | null };
	columns: { table: string; column: string; columnType: string; charset: string | null; collation: string | null; engine: string | null }[];
	foreignKeys: { constraint: string; column: string; parentDatabase: string; parentTable: string; parentColumn: string; parentType: string | null; parentCollation: string | null }[];
	truncated: boolean;
}

const explanations: [number[], string, string[]][] = [
	[[1005, 1215], "Foreign key or table definition rejected", ["Compare both column types, including integer size and UNSIGNED. Check character sets and collations for text keys.", "Check that the parent table exists, uses a compatible engine, and has an index beginning with the referenced columns.", "Review the exact server error: error 1005 can also have causes unrelated to foreign keys."]],
	[[1451, 1452], "A foreign key protects related rows", ["For an insert, confirm the parent row exists. For a delete or update, inspect dependent rows first.", "Do not disable foreign-key checks to hide the failure. Choose a migration that preserves the relationship."]],
	[[1267, 1271], "Incompatible text collations", ["Inspect the compared columns and the connection collation. Different collations elsewhere in a database are not automatically a problem.", "Review conversions on a backup first. Changing a database default does not convert existing columns, and conversions can change unique-key comparisons."]],
	[[1273, 1115], "Unsupported collation or character set", ["Check the server version and available character sets/collations in database inspection.", "Review the import's source database version. Choose a compatible collation deliberately instead of replacing names throughout the whole file."]],
	[[1044, 1045, 1142], "Connection or privilege rejected", ["Verify the username, host, password and the account's grants for this database.", "Accounts such as user@localhost and user@127.0.0.1 can have different passwords and permissions."]],
	[[1046, 1049], "Database scope is missing or unavailable", ["Choose an existing database in the execution scope, or review the script's CREATE DATABASE and USE statements.", "Confirm this account can see the database."]],
	[[1054, 1146], "Column or table was not found", ["Check the selected database, identifier spelling, case sensitivity, and required earlier migrations.", "Inspect the live schema before changing a resource's queries."]],
	[[1062], "A unique key already contains this value", ["Inspect the primary/unique index and the existing row. Decide whether the data should be updated or kept.", "Do not blindly replace INSERT with REPLACE: replacement can delete existing rows and affect related records."]],
	[[1064], "SQL syntax rejected", ["Review the statement around the reported line, including quotes, delimiters, and reserved identifiers.", "Check whether the script targets another MySQL/MariaDB version. The app does not rewrite or retry failed statements."]],
	[[1205, 1213], "Lock timeout or deadlock", ["Inspect active transactions and avoid concurrent migrations.", "A timeout or deadlock can leave a script partially applied. Check transaction boundaries and committed changes before a deliberate retry."]],
	[[1366, 1406, 1264], "Value does not fit the column", ["Compare the value's encoding, length, numeric range and the destination column definition.", "Keep strict validation enabled. Do not truncate or silently coerce data to make an import pass."]],
	[[2002, 2003, 2006, 2013], "Connection failed or was lost", ["Check service state, address, port and network access.", "After a lost connection, the result of a write may be unknown. Check the database before retrying."]],
];

export function diagnoseSqlError(message: string): SqlDiagnosis {
	const bounded = message.slice(0, 32_768);
	const code = bounded.match(/\bERROR\s+(\d{4})\b/i) ?? bounded.match(/\berror\s*[:(]?\s*(\d{4})\b/i);
	const match = code ? explanations.find(([codes]) => codes.includes(Number(code[1]))) : undefined;
	return match ? { title: `${code![1]}: ${match[1]}`, checks: match[2] } : {
		title: "Review the SQL error",
		checks: ["Check the reported statement, execution scope and the server version.", "Failed scripts can be partially applied, especially when they contain DDL. Review existing changes before running the script again."],
	};
}

export function inspectDatabaseSql(credentials: MariaDBCredentials, database: string, table: string) {
	return trackTask("inspect_database_sql", "Inspect SQL compatibility", () => invoke<SqlInspection>("inspect_database_sql", { credentials: { ...credentials }, database, table: table.trim() || null }));
}
