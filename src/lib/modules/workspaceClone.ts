import { invoke } from "@tauri-apps/api/core";
import { hasRunningTasks, taskSession, trackTask } from "$lib/core/tasks.svelte";
import type { MariaDBCredentials } from "./mariadb";

export type CloneMode = "clone" | "export" | "import";
export interface CloneRequest {
	sourcePath: string;
	destinationPath: string;
	mode: CloneMode;
	resources: string[];
	configs: string[];
	serverPort: number;
	txAdminPort: number;
	sourceServerPort: number;
	sourceTxAdminPort: number;
	database?: { dumpPath: string; sourceDatabase: string; host: string; port: number; username: string } | null;
}
export interface CloneChoices { sourcePath: string; resources: string[]; configs: string[] }
export interface ClonePreview {
	id: string;
	sourcePath: string;
	destinationPath: string;
	mode: CloneMode;
	serverPort: number;
	txAdminPort: number;
	files: { path: string; size: number; sha256: string }[];
	excluded: { path: string; reason: string }[];
	totalBytes: number;
	expiresAt: number;
	database: { sourcePath: string; sourceDatabase: string; sizeBytes: number; sha256: string; tableCount: number; target: { database: string; host: string; port: number } | null } | null;
}
export interface CloneResult { destinationPath: string; serverDataPath: string; txDataPath: string; artifactPath: string; fileCount: number; database: { host: string; port: number; username: string; database: string } | null }

function exclusiveTask<T>(command: string, label: string, args: Record<string, unknown>) {
	if (hasRunningTasks() || taskSession.switching) return Promise.reject(new Error("Wait for background work and workspace switching to finish."));
	return trackTask(command, label, () => invoke<T>(command, args));
}
export function listCloneChoices(sourcePath: string) { return exclusiveTask<CloneChoices>("list_workspace_clone_choices", "Read clone source", { sourcePath }); }
export function previewClone(request: CloneRequest) { return exclusiveTask<ClonePreview>("preview_workspace_clone", "Preview private clone", { request }); }
export function executeClone(preview: ClonePreview, confirmedDestination: string, privateCopyConfirmed: boolean, databaseCredentials?: MariaDBCredentials, confirmedDatabase?: string) {
	return exclusiveTask<CloneResult>("execute_workspace_clone", "Create private clone", { previewId: preview.id, confirmedDestination, privateCopyConfirmed, databaseCredentials: databaseCredentials ?? null, confirmedDatabase: confirmedDatabase ?? null });
}
export function discardClonePreview(id: string) { return invoke<void>("discard_workspace_clone_preview", { previewId: id }); }
