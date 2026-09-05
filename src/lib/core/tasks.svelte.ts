import { invoke } from "@tauri-apps/api/core";
import { log } from "./logger.svelte";
import { appendTaskIncident } from "./incidents.svelte";

export type TaskStatus = "running" | "completed" | "failed" | "cancelled";
export interface BackgroundTask {
	id: string;
	command: string;
	label: string;
	workspaceId: string;
	status: TaskStatus;
	startedAt: number;
	finishedAt?: number;
}

const labels: Record<string, string> = {
	start_fxserver: "Start FXServer",
	stop_fxserver: "Stop FXServer",
	restart_fxserver: "Restart FXServer",
	send_fxserver_command: "Send console command",
	send_fxserver_rcon_command: "Send resource command",
	install_windows_artifact: "Install artifact",
	install_mariadb: "Install MariaDB",
	update_mariadb: "Update MariaDB",
	uninstall_mariadb: "Uninstall MariaDB",
	start_mariadb: "Start MariaDB",
	stop_mariadb: "Stop MariaDB",
	restart_mariadb: "Restart MariaDB",
	start_mariadb_service: "Start MariaDB",
	stop_mariadb_service: "Stop MariaDB",
	restart_mariadb_service: "Restart MariaDB",
	validate_mariadb_credentials: "Validate database credentials",
	execute_mariadb_query: "Run database query",
	backup_mariadb: "Back up database",
	create_mariadb_user: "Create database user",
	save_mariadb_user: "Create database user",
	grant_mariadb_permissions: "Change database permissions",
	delete_mariadb_user: "Delete database user",
	update_mariadb_user: "Update database user",
	save_server_config: "Save server configuration",
	preview_resource_update: "Prepare resource update",
	apply_resource_update: "Apply resource update",
	rollback_resource_update: "Roll back resource",
	delete_resource_snapshot: "Delete resource snapshot",
	run_fxserver_preflight: "Check server readiness",
	preview_diagnostic_export: "Prepare diagnostics",
	export_diagnostic_zip: "Export diagnostics",
	save_backup_schedule: "Save backup schedule",
	remove_backup_schedule: "Remove backup schedule",
	run_scheduled_backup_now: "Back up database",
	preview_backup_restore: "Check database restore",
	restore_backup_snapshot: "Restore database",
	configure_health: "Configure health monitoring",
	apply_live_bridge_change: "Change live bridge installation",
	send_live_bridge_action: "Send live resource command",
};

export const taskSession = $state({
	items: [] as BackgroundTask[],
	workspaceId: "default",
	switching: false,
});

export function hasRunningTasks() {
	return taskSession.items.some((task) => task.status === "running");
}

export function clearFinishedTasks() {
	taskSession.items = taskSession.items.filter((task) => task.status === "running");
}

export async function trackTask<T>(command: string, label: string, action: () => Promise<T>): Promise<T> {
	if (taskSession.switching) throw new Error("Wait for the workspace switch to finish.");
	const id = crypto.randomUUID();
	taskSession.items = [
		{ id, command, label, workspaceId: taskSession.workspaceId, status: "running", startedAt: Date.now() },
		...taskSession.items.filter((task, index) => task.status === "running" || index < 99),
	];
	try {
		const result = await action();
		finish(id, "completed");
		log(`${label} completed.`, { level: "success", scope: "tasks" });
		return result;
	} catch (error) {
		finish(id, error instanceof DOMException && error.name === "AbortError" ? "cancelled" : "failed");
		log(`${label} failed.`, { level: "error", scope: "tasks" });
		throw error;
	}
}

function finish(id: string, status: TaskStatus) {
	const task = taskSession.items.find((item) => item.id === id);
	if (task) {
		task.status = status;
		task.finishedAt = Date.now();
		appendTaskIncident(task);
	}
}

export function taskInvoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
	const label = labels[command];
	return label ? trackTask(command, label, () => invoke<T>(command, args)) : invoke<T>(command, args);
}

export function acceptBackupProgress(event: { workspaceId: string; scheduleId: string; stage: string; timestamp: number }) {
	if (taskSession.items.some((item) => item.workspaceId === event.workspaceId && item.status === "running" && ["run_scheduled_backup_now", "restore_backup_snapshot", "test_backup_restore"].includes(item.command))) return;
	const command = `scheduled-backup:${event.scheduleId}`;
	const existing = taskSession.items.find((item) => item.command === command && item.workspaceId === event.workspaceId && item.status === "running");
	if (event.stage === "running" && !existing) {
		taskSession.items = [{ id: crypto.randomUUID(), command, label: "Scheduled database backup", workspaceId: event.workspaceId, status: "running", startedAt: event.timestamp }, ...taskSession.items.filter((task, index) => task.status === "running" || index < 99)];
	} else if (existing && ["completed", "error"].includes(event.stage)) {
		finish(existing.id, event.stage === "completed" ? "completed" : "failed");
	}
}
