import { invoke } from "@tauri-apps/api/core";
import { trackTask } from "$lib/core/tasks.svelte";
import type { MariaDBCredentials } from "./mariadb";

export const inspectionTabs = [
	{ value: "overview", label: "Overview" }, { value: "status", label: "Status" },
	{ value: "processes", label: "Processes" }, { value: "variables", label: "Variables" },
	{ value: "charsets", label: "Charsets" }, { value: "engines", label: "Engines" },
	{ value: "users", label: "Users" }, { value: "grants", label: "Grants" },
] as const;
export type InspectionView = typeof inspectionTabs[number]["value"];
export interface InspectionResult { columns: string[]; rows: (string | null)[][]; hasMore: boolean; limit: number; notice: string }
export function inspectDatabase(credentials: MariaDBCredentials, view: InspectionView, database: string) {
	return trackTask("inspect_database", "Read database inspection", () => invoke<InspectionResult>("inspect_database", { credentials, view, database }));
}
