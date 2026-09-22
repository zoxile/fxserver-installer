import { invoke } from "@tauri-apps/api/core";
import { trackTask } from "$lib/core/tasks.svelte";
import type { MariaDBCredentials } from "./mariadb";

export type AdminAction = "createDatabase" | "createTable" | "empty" | "drop" | "optimize" | "analyze" | "check" | "repair";
export interface ColumnSpec {
	name: string; dataType: string; length: string | null; unsigned: boolean; nullable: boolean;
	defaultKind: string; defaultValue: string | null; autoIncrement: boolean; primary: boolean; unique: boolean;
}
export interface AdminRequest { workspaceId: string; database: string; table: string | null; action: AdminAction; columns: ColumnSpec[] }
export interface TableInfo { name: string; kind: string; engine: string | null; rows: number | null; collation: string | null; dataBytes: number; indexBytes: number; freeBytes: number }
export interface TableInfoPage { tables: TableInfo[]; hasMore: boolean }
export interface AdminPreview { token: string; sql: string; confirmation: string; expiresAt: number; host: string; port: number; database: string; table: string | null; warning: string }
export interface AdminResult { message: string; messages: string[][]; hasIssues: boolean }

export const columnTypes = ["INT", "BIGINT", "VARCHAR", "TEXT", "DECIMAL", "BOOLEAN", "DATE", "DATETIME", "TIMESTAMP", "JSON", "TINYINT", "SMALLINT", "CHAR", "TIME"];
export function newColumn(name = ""): ColumnSpec { return { name, dataType: "VARCHAR", length: "255", unsigned: false, nullable: false, defaultKind: "none", defaultValue: null, autoIncrement: false, primary: false, unique: false }; }
export function primaryColumn(): ColumnSpec { return { ...newColumn("id"), dataType: "INT", length: null, primary: true, autoIncrement: true, unsigned: true }; }
export function isSystemDatabase(name: string) { return ["mysql", "sys", "information_schema", "performance_schema"].includes(name.toLowerCase()); }
export function listAdminTables(credentials: MariaDBCredentials, database: string) {
	return trackTask("list_database_admin_tables", "Read table statistics", () => invoke<TableInfoPage>("list_database_admin_tables", { credentials, database }));
}
export function previewAdminAction(credentials: MariaDBCredentials, request: AdminRequest) {
	return trackTask("preview_database_admin_action", "Review database administration", () => invoke<AdminPreview>("preview_database_admin_action", { credentials, request }));
}
export function applyAdminAction(workspaceId: string, token: string, confirmation: string) {
	return trackTask("apply_database_admin_action", "Apply reviewed database administration", () => invoke<AdminResult>("apply_database_admin_action", { workspaceId, token, confirmation }));
}
