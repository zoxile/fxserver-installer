<script lang="ts">
	import PlayIcon from "@lucide/svelte/icons/play";
	import PauseIcon from "@lucide/svelte/icons/pause";
	import SquareIcon from "@lucide/svelte/icons/square";
	import TrashIcon from "@lucide/svelte/icons/trash-2";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import { getResourcePlan, pauseResourcePlan, stopResourcePlan, runResourcePlan, removeResourcePlanEntry } from "$lib/modules/resourcePlan.svelte";
	let { workspaceId }: { workspaceId: string } = $props();
	const plan = $derived(getResourcePlan(workspaceId));
	const ready = $derived(plan.entries.filter((entry) => entry.status === "ready").length);
	let error = $state("");
	async function run() {
		error = "";
		try { await runResourcePlan(workspaceId); }
		catch (caught) { error = String(caught); }
	}
	async function remove(id: string) {
		try { await removeResourcePlanEntry(workspaceId, id); }
		catch (caught) { error = String(caught); }
	}
</script>

<section class="space-y-4 border-y border-border py-5" aria-label="Reviewed resource update queue">
	<div class="flex flex-wrap items-center justify-between gap-3">
		<div><h2 class="text-lg font-semibold">Reviewed Update Queue</h2><p class="mt-1 text-xs text-muted-foreground">{ready} ready / {plan.status}{plan.stopRequested && plan.status === "running" ? " / stopping after current resource" : plan.pauseRequested && plan.status === "running" ? " / pausing after current resource" : ""}</p></div>
		<div class="flex flex-wrap gap-2">
			<Button size="sm" disabled={!ready || plan.status === "running"} onclick={run}><PlayIcon />{plan.status === "paused" ? "Continue Remaining" : "Apply Reviewed"}</Button>
			<Button variant="outline" size="icon" title="Pause after the current resource" aria-label="Pause update queue" disabled={plan.status !== "running" || plan.pauseRequested || plan.stopRequested} onclick={() => pauseResourcePlan(workspaceId)}><PauseIcon /></Button>
			<Button variant="outline" size="icon" title="Stop after the current resource and cancel remaining updates" aria-label="Stop update queue" disabled={(!ready && plan.status !== "running") || plan.stopRequested} onclick={() => stopResourcePlan(workspaceId)}><SquareIcon /></Button>
		</div>
	</div>
	{#if error || plan.error}<Notice tone="error" title={plan.status === "paused" ? "Queue paused on failure" : "Update queue error"} message={error || plan.error} />{/if}
	{#if plan.entries.length}
		<div class="divide-y divide-border">
			{#each plan.entries as entry (entry.id)}
				<div class="flex items-start justify-between gap-3 py-3">
					<div class="min-w-0 space-y-1"><p class="break-all text-sm font-medium">{entry.name} <span class="font-normal text-muted-foreground">/ {entry.preview.branch}</span></p><p class="break-all text-xs text-muted-foreground">{entry.preview.changes.length} changes / {entry.protectedPaths.length} protected / reviewed {new Date(entry.reviewedAt).toLocaleTimeString()}</p><p class={`break-words text-xs ${entry.status === "failed" ? "text-red-300" : entry.status === "completed" ? "text-emerald-300" : "text-muted-foreground"}`}>{entry.status}: {entry.outcome}</p></div>
					<Button variant="ghost" size="icon" disabled={plan.status === "running"} title={`Remove ${entry.name} from queue`} aria-label={`Remove ${entry.name} from queue`} onclick={() => remove(entry.id)}><TrashIcon /></Button>
				</div>
			{/each}
		</div>
	{:else}<p class="text-sm text-muted-foreground">No individually reviewed updates queued.</p>{/if}
</section>
