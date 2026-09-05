import { invoke } from "@tauri-apps/api/core";
import { trackTask } from "$lib/core/tasks.svelte";
import type { MariaDBCredentials } from "./mariadb";

export interface BrowserColumn { name: string; columnType: string; nullable: boolean; defaultValue: string | null; extra: string; binary: boolean }
export interface BrowserIndex { name: string; column: string | null; sequence: number; unique: boolean; indexType: string; prefixLength: number | null }
export interface BrowserMetadata { columns: BrowserColumn[]; indexes: BrowserIndex[]; editable?: boolean; editReason?: string | null }
export type FilterOperator = "eq" | "ne" | "lt" | "lte" | "gt" | "gte" | "contains" | "isNull" | "isNotNull";
export interface BrowserFilter { column: string; operator: FilterOperator; value: string | null }
export interface BrowserRequest { database: string; table: string; filters: BrowserFilter[]; sortColumn: string | null; descending: boolean; offset: number; pageSize: number }
export interface BrowserPage { rows: (string | null)[][]; hasMore: boolean; truncatedCells: boolean; pageSize?: number }
export interface BrowserExport { path: string; rows: number; hasMore: boolean }
export type CellInput = { kind: "null" } | { kind: "text" | "number"; value: string };
export interface ColumnInput { column: string; value: CellInput }
export type ChangeKind = "insert" | "update" | "delete";
export interface BrowserChange { workspaceId: string; database: string; table: string; kind: ChangeKind; values: ColumnInput[]; original: (string | null)[] | null }
export interface ChangePreview { token: string; sql: string; parameters: ColumnInput[]; confirmation: string; expiresAt: number; kind: ChangeKind; host: string; port: number }

export function getBrowserMetadata(credentials: MariaDBCredentials, database: string, table: string) {
	return trackTask("get_database_browser_metadata", "Read table metadata", () => invoke<BrowserMetadata>("get_database_browser_metadata", { credentials, database, table }));
}
export function getBrowserRows(credentials: MariaDBCredentials, request: BrowserRequest) {
	return trackTask("get_database_browser_rows", "Browse database rows", () => invoke<BrowserPage>("get_database_browser_rows", { credentials, request }));
}
export function exportBrowserCsv(credentials: MariaDBCredentials, request: BrowserRequest, outputPath: string) {
	return trackTask("export_database_browser_csv", "Export database CSV", () => invoke<BrowserExport>("export_database_browser_csv", { credentials, request, outputPath }));
}

export function previewBrowserChange(credentials: MariaDBCredentials, change: BrowserChange) {
	return trackTask("preview_database_browser_change", "Preview single-row change", () => invoke<ChangePreview>("preview_database_browser_change", { credentials, change }));
}
export function applyBrowserChange(workspaceId: string, token: string, confirmation: string) {
	return trackTask("apply_database_browser_change", "Apply confirmed single-row change", () => invoke<number>("apply_database_browser_change", { workspaceId, token, confirmation }));
}
