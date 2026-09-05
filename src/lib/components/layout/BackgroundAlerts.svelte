<script lang="ts">
	import { onMount } from "svelte";
	import { listen, type UnlistenFn } from "@tauri-apps/api/event";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import { acceptBackgroundLog, type AppLogEntry } from "$lib/core/logger.svelte";
	import { acceptBackupProgress, taskSession } from "$lib/core/tasks.svelte";
	import { acceptBridgeStatus } from "$lib/core/liveBridge.svelte";
	import { appendIncident, appendIncidents, appendHealthIncident } from "$lib/core/incidents.svelte";
	import type { BridgeStatus } from "$lib/modules/liveBridge";

	type BackgroundLog = { id: string; level: string; scope: string; message: string };
	let alerts = $state<BackgroundLog[]>([]);
	function recordLog(event: Event) {
		const entry = (event as CustomEvent<AppLogEntry>).detail;
		if (["debug"].includes(entry.level) || ["tasks", "navigation", "fxserver.health", "core.logger"].includes(entry.scope)) return;
		if (entry.level === "info" && !/fxserver|resource|config|mariadb|backup/.test(entry.scope)) return;
		appendIncident({ id: entry.id, workspaceId: taskSession.workspaceId, timestamp: entry.timestamp, level: entry.level,
			title: entry.message, detail: entry.detail, type: /resource/.test(entry.scope) ? "resource" : /config/.test(entry.scope) ? "config" : "log" });
	}
	onMount(() => {
		window.addEventListener("app-log-entry", recordLog);
		if (!("__TAURI_INTERNALS__" in window)) return () => window.removeEventListener("app-log-entry", recordLog);
		let disposed = false;
		const listeners: UnlistenFn[] = [];
		const register = async (pending: Promise<UnlistenFn>) => {
			try {
				const unlisten = await pending;
				if (disposed) unlisten(); else listeners.push(unlisten);
			} catch (error) { console.error("Could not listen for background events.", error); }
		};
		void register(listen<BackgroundLog>("background-app-log", ({ payload }) => {
			acceptBackgroundLog(payload);
			if (["warn", "error"].includes(payload.level)) alerts = [...alerts.slice(-2), payload];
		}));
		void register(listen<Parameters<typeof acceptBackupProgress>[0]>("backup-manager-progress", ({ payload }) => acceptBackupProgress(payload)));
		void register(listen<BridgeStatus>("live-bridge-update", ({ payload }) => acceptBridgeStatus(payload)));
		void register(listen<Parameters<typeof appendHealthIncident>[0]>("fxserver-health-event", ({ payload }) => appendHealthIncident(payload)));
		void register(listen<{ id: number; workspaceId: string; timestamp: number; level: "warn" | "error"; message: string }[]>("fxserver-console-incidents", ({ payload }) => {
			appendIncidents(payload.map((entry) => ({ id: `console:${entry.timestamp}:${entry.id}`, timestamp: entry.timestamp, workspaceId: entry.workspaceId,
				level: entry.level, title: entry.message, type: "log", panel: "server-manage" })));
		}));
		return () => { disposed = true; listeners.forEach((unlisten) => unlisten()); window.removeEventListener("app-log-entry", recordLog); };
	});
</script>

{#if alerts.length}
	<div class="fixed top-12 right-4 z-50 grid w-96 max-w-[calc(100vw-6rem)] gap-2" aria-label="Background notifications">
		{#each alerts as alert (alert.id)}
			<Notice tone={alert.level === "error" ? "error" : "warn"} title={alert.scope === "fxserver.health" ? "Server health" : "Background task"} message={alert.message} class="bg-card shadow-lg" onDismiss={() => alerts = alerts.filter((item) => item.id !== alert.id)} />
		{/each}
	</div>
{/if}
