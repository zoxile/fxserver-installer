import { invoke } from "@tauri-apps/api/core";
import { trackTask } from "$lib/core/tasks.svelte";
import type { MariaDBCredentials } from "$lib/modules/mariadb";

export interface BackupSchedule {
	id: string;
	workspaceId: string;
	name: string;
	database: string;
	outputDir: string;
	intervalMinutes: number;
	retainCount: number;
}

export interface ScheduleStatus {
	config: BackupSchedule;
	enabled: boolean;
	running: boolean;
	nextRun: number | null;
	lastRun: number | null;
	lastError: string | null;
}

export interface BackupSnapshot {
	id: string;
	workspaceId: string;
	scheduleId: string;
	database: string;
	directory: string;
	createdAt: number;
	sizeBytes: number;
	sha256: string;
	kind: "scheduled" | "manual" | "recovery";
	sourceHost: string;
	sourcePort: number;
}

export interface BackupOverview {
	schedules: ScheduleStatus[];
	snapshots: BackupSnapshot[];
	busy: boolean;
}

export interface BackupEvent {
	workspaceId: string;
	scheduleId: string;
	stage: "running" | "completed" | "error";
	message: string;
	timestamp: number;
}

export interface RestorePreview {
	token: string;
	snapshot: BackupSnapshot;
	targetHost: string;
	targetPort: number;
	targetDatabase: string;
	existingTables: number;
	expiresAt: number;
	warnings: string[];
}

export interface RestoreResult {
	recoverySnapshot: BackupSnapshot;
	message: string;
}

export function getBackupManager(workspaceId: string) {
	if (!("__TAURI_INTERNALS__" in window)) return Promise.resolve<BackupOverview>({ schedules: [], snapshots: [], busy: false });
	return invoke<BackupOverview>("get_backup_manager", { workspaceId });
}

function run<T>(command: string, label: string, args: Record<string, unknown>) {
	return trackTask(command, label, async () => {
		return invoke<T>(command, args);
	});
}

export function saveBackupSchedule(config: BackupSchedule, enabled: boolean, credentials?: MariaDBCredentials) {
	return run<void>("save_backup_schedule", "Save backup schedule", { config, enabled, credentials: credentials ?? null });
}

export function removeBackupSchedule(workspaceId: string, scheduleId: string) {
	return run<void>("remove_backup_schedule", "Remove backup schedule", { workspaceId, scheduleId });
}

export function runBackupNow(workspaceId: string, scheduleId: string, credentials: MariaDBCredentials) {
	return run<BackupSnapshot>("run_scheduled_backup_now", "Back up database", { workspaceId, scheduleId, credentials });
}

export function previewBackupRestore(workspaceId: string, snapshotId: string, credentials: MariaDBCredentials) {
	return run<RestorePreview>("preview_backup_restore", "Verify restore snapshot", { workspaceId, snapshotId, credentials });
}

export function restoreBackupSnapshot(workspaceId: string, token: string, confirmationDatabase: string) {
	return run<RestoreResult>("restore_backup_snapshot", "Restore database snapshot", { workspaceId, token, confirmationDatabase });
}
