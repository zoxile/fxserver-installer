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
	restoreTests?: RestoreTestEvidence[];
}

export interface RestoreTestEvidence {
	id: string;
	workspaceId: string;
	snapshotId: string;
	snapshotSha256: string;
	targetHost: string;
	targetPort: number;
	temporaryDatabase: string;
	status: "running" | "interrupted" | "preflight_refused" | "passed" | "failed";
	startedAt: number;
	finishedAt: number | null;
	tablesVerified: string[];
	error: string | null;
	cleanupError: string | null;
	cleanedUp: boolean;
	created: boolean;
}

export interface RestoreTestPreview {
	token: string;
	snapshotId: string;
	targetHost: string;
	targetPort: number;
	temporaryDatabase: string;
	tables: string[];
	statements: number;
	expiresAt: number;
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

export function previewBackupRestoreTest(workspaceId: string, snapshotId: string, credentials: MariaDBCredentials) {
	return run<RestoreTestPreview>("preview_backup_restore_test", "Preflight isolated restore test", { workspaceId, snapshotId, credentials });
}

export function testBackupRestore(workspaceId: string, token: string, confirmationDatabase: string, confirmCleanup: boolean) {
	return run<RestoreTestEvidence>("test_backup_restore", "Test isolated database restore", { workspaceId, token, confirmationDatabase, confirmCleanup });
}

export function cleanupBackupRestoreTest(workspaceId: string, testId: string, confirmationDatabase: string, credentials: MariaDBCredentials) {
	return run<RestoreTestEvidence>("cleanup_backup_restore_test", "Clean up owned restore-test database", { workspaceId, testId, confirmationDatabase, credentials });
}
