import { taskInvoke } from "$lib/core/tasks.svelte";

export interface ResourceTarget {
	workspaceId: string;
	txDataPath: string;
	profile: string;
	resourcePath: string;
}

export interface ResourceFileChange {
	path: string;
	kind: "added" | "modified" | "removed";
	oldSize: number | null;
	newSize: number | null;
	preserve: boolean;
	canPreserve: boolean;
}

export interface ResourceUpdatePreview {
	id: string;
	resourceName: string;
	repository: string;
	branch: string;
	archiveSha256: string;
	archiveBytes: number;
	changes: ResourceFileChange[];
	createdAt: number;
}

export interface ResourceSnapshot {
	id: string;
	resourceName: string;
	createdAt: number;
	fileCount: number;
	sizeBytes: number;
	reason: string;
}

export function previewResourceUpdate(target: ResourceTarget, branch: string) {
	return taskInvoke<ResourceUpdatePreview>("preview_resource_update", { request: { target, branch } });
}

export function applyResourceUpdate(target: ResourceTarget, previewId: string, protectedPaths: string[]) {
	return taskInvoke<ResourceSnapshot>("apply_resource_update", { request: { target, previewId, protectedPaths } });
}

export function discardResourcePreview(previewId: string) {
	return taskInvoke<void>("discard_resource_preview", { previewId });
}

export function listResourceSnapshots(target: ResourceTarget) {
	return taskInvoke<ResourceSnapshot[]>("list_resource_snapshots", { target });
}

export function rollbackResourceUpdate(target: ResourceTarget, snapshotId: string) {
	return taskInvoke<ResourceSnapshot>("rollback_resource_update", { target, snapshotId });
}

export function deleteResourceSnapshot(target: ResourceTarget, snapshotId: string) {
	return taskInvoke<void>("delete_resource_snapshot", { target, snapshotId });
}
