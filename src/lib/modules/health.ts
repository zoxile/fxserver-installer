import { invoke } from "@tauri-apps/api/core";
import { taskInvoke } from "$lib/core/tasks.svelte";

export interface HealthConfig {
	alertsEnabled: boolean;
	recoveryEnabled: boolean;
	cpuThresholdPercent: number;
	memoryThresholdPercent: number;
	minimumFreeDiskGb: number;
	diskPath: string;
	sustainedSeconds: number;
	alertCooldownSeconds: number;
	recoveryBackoffSeconds: number;
}

export interface HealthEvent {
	id: number;
	timestamp: number;
	level: "info" | "warn" | "error";
	kind: string;
	message: string;
	workspaceId: string;
}

export interface HealthStatus {
	workspaceId: string;
	config: HealthConfig;
	sample: {
		timestamp: number;
		running: boolean;
		pid: number | null;
		cpuPercent: number | null;
		memoryPercent: number | null;
		freeDiskGb: number | null;
	} | null;
	events: HealthEvent[];
	recoveryArmed: boolean;
	recoveryBlocked: boolean;
	recoveryAttempts: number;
	nextRecoverySeconds: number | null;
}

export const defaultHealthConfig: HealthConfig = {
	alertsEnabled: false,
	recoveryEnabled: false,
	cpuThresholdPercent: 90,
	memoryThresholdPercent: 80,
	minimumFreeDiskGb: 5,
	diskPath: "",
	sustainedSeconds: 15,
	alertCooldownSeconds: 300,
	recoveryBackoffSeconds: 30,
};

export function getHealthStatus() {
	return invoke<HealthStatus>("get_health_status");
}

export function configureHealth(config: HealthConfig, workspaceId: string) {
	return taskInvoke<HealthStatus>("configure_health", { config, workspaceId });
}

export function clearHealthEvents() {
	return taskInvoke<void>("clear_health_events");
}
