<script lang="ts">
	import { onMount } from "svelte";
	import { listen, type UnlistenFn } from "@tauri-apps/api/event";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import { acceptBackgroundLog } from "$lib/core/logger.svelte";
	import { acceptBackupProgress } from "$lib/core/tasks.svelte";

	type BackgroundLog = { id: string; level: string; scope: string; message: string };
	let alerts = $state<BackgroundLog[]>([]);
	onMount(() => {
		if (!("__TAURI_INTERNALS__" in window)) return;
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
		return () => { disposed = true; listeners.forEach((unlisten) => unlisten()); };
	});
</script>

{#if alerts.length}
	<div class="fixed top-12 right-4 z-50 grid w-96 max-w-[calc(100vw-6rem)] gap-2" aria-label="Background notifications">
		{#each alerts as alert (alert.id)}
			<Notice tone={alert.level === "error" ? "error" : "warn"} title={alert.scope === "fxserver.health" ? "Server health" : "Background task"} message={alert.message} class="bg-card shadow-lg" onDismiss={() => alerts = alerts.filter((item) => item.id !== alert.id)} />
		{/each}
	</div>
{/if}
