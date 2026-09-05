<script lang="ts">
	import { onMount } from "svelte";
	import CheckIcon from "@lucide/svelte/icons/check";
	import CircleAlertIcon from "@lucide/svelte/icons/circle-alert";
	import ClockIcon from "@lucide/svelte/icons/clock";
	import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
	import Trash2Icon from "@lucide/svelte/icons/trash-2";
	import { Button } from "$lib/components/ui/button/index.js";
	import * as Select from "$lib/components/ui/select/index.js";
	import { clearFinishedTasks, taskSession } from "$lib/core/tasks.svelte";
	import { mariadbActivity } from "$lib/core/mariadbActivity.svelte";
	import { workspaceSession } from "$lib/core/workspaces.svelte";
	import type { PageId } from "$lib/navigation";

	let { onNavigate }: { onNavigate: (page: PageId) => void } = $props();
	let filter = $state("all");
	let now = $state(Date.now());
	const visible = $derived(taskSession.items.filter((task) => filter === "all" || task.status === filter));
	const running = $derived(taskSession.items.filter((task) => task.status === "running").length);
	onMount(() => {
		const timer = window.setInterval(() => { now = Date.now(); }, 1000);
		return () => clearInterval(timer);
	});
	function duration(start: number, end: number) {
		const seconds = Math.max(0, Math.floor((end - start) / 1000));
		return seconds < 60 ? `${seconds}s` : `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
	}
</script>

<div class="space-y-6">
	<header class="flex flex-wrap items-center justify-between gap-3 border-b border-border pb-4">
		<div><h1 class="text-2xl font-semibold">Task Center</h1><p class="mt-1 text-sm text-muted-foreground">{running} running</p></div>
		<Button variant="outline" onclick={clearFinishedTasks} disabled={!taskSession.items.some((task) => task.status !== "running")}><Trash2Icon class="size-4" />Clear finished</Button>
	</header>
	<div class="flex items-center justify-between gap-3">
		<Select.Root type="single" bind:value={filter}>
			<Select.Trigger class="w-44" aria-label="Filter tasks">{filter === "all" ? "All tasks" : filter.charAt(0).toUpperCase() + filter.slice(1)}</Select.Trigger>
			<Select.Content>{#each ["all", "running", "completed", "failed", "cancelled"] as value}<Select.Item {value}>{value === "all" ? "All tasks" : value.charAt(0).toUpperCase() + value.slice(1)}</Select.Item>{/each}</Select.Content>
		</Select.Root>
		<Button variant="ghost" onclick={() => onNavigate("logs")}>Application Logs</Button>
	</div>
	<div class="divide-y divide-border border-y border-border">
		{#each visible as task (task.id)}
			<div class="flex items-start gap-3 py-4">
				<div class="pt-0.5">
					{#if task.status === "running"}<LoaderCircleIcon class="size-5 animate-spin text-sky-400" />
					{:else if task.status === "completed"}<CheckIcon class="size-5 text-emerald-400" />
					{:else if task.status === "failed"}<CircleAlertIcon class="size-5 text-destructive" />
					{:else}<ClockIcon class="size-5 text-muted-foreground" />{/if}
				</div>
				<div class="min-w-0 flex-1 space-y-1">
					<p class="text-sm font-medium">{task.label}</p>
					<p class="text-xs text-muted-foreground">{workspaceSession.items.find((item) => item.id === task.workspaceId)?.name ?? "Workspace"} · {new Date(task.startedAt).toLocaleTimeString()}</p>
					{#if task.status === "running" && task.command.includes("mariadb") && mariadbActivity.busy}<p class="break-words text-xs text-muted-foreground">{mariadbActivity.stage}</p>{/if}
					{#if task.status === "failed"}<p class="text-xs text-destructive">Failed. See Application Logs for details.</p>{/if}
				</div>
				<div class="shrink-0 text-right text-xs text-muted-foreground"><p class="capitalize">{task.status}</p><p class="mt-1 tabular-nums">{duration(task.startedAt, task.finishedAt ?? now)}</p></div>
			</div>
		{:else}<p class="py-12 text-center text-sm text-muted-foreground">No tasks in this view.</p>{/each}
	</div>
</div>
