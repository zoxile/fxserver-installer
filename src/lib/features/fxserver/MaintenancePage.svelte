<script lang="ts">
	import { onDestroy, onMount } from "svelte";
	import PlayIcon from "@lucide/svelte/icons/play";
	import SquareIcon from "@lucide/svelte/icons/square";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import ClipboardCheckIcon from "@lucide/svelte/icons/clipboard-check";
	import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
	import CheckIcon from "@lucide/svelte/icons/check";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Checkbox } from "$lib/components/ui/checkbox/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import { getBackupManager, type ScheduleStatus } from "$lib/modules/backupManager";
	import { maintenanceSession, prepareMaintenance, runMaintenance, invalidateMaintenanceReview } from "$lib/modules/maintenance.svelte";
	import { getResourcePlan } from "$lib/modules/resourcePlan.svelte";
	import { workspaceSession } from "$lib/core/workspaces.svelte";
	import { taskSession } from "$lib/core/tasks.svelte";
	import type { PageId } from "$lib/navigation";

	let { onNavigate }: { onNavigate: (page: PageId) => void } = $props();
	const workspaceId = workspaceSession.activeId;
	const resourcePlan = getResourcePlan(workspaceId);
	let schedules = $state<ScheduleStatus[]>([]);
	let selected = $state<string[]>([]);
	let announce = $state(false);
	let waitForPlayers = $state(false);
	let confirmed = $state(false);
	let loading = $state(false);
	let error = $state("");
	const running = $derived(maintenanceSession.run?.status === "running");
	const review = $derived(maintenanceSession.review?.workspaceId === workspaceId ? maintenanceSession.review : null);
	const pending = $derived(resourcePlan.entries.filter((entry) => entry.status === "ready"));
	$effect(() => { review?.id; confirmed = false; });
	async function refresh() {
		loading = true; error = "";
		try { schedules = (await getBackupManager(workspaceId)).schedules; selected = selected.filter((id) => schedules.some((item) => item.config.id === id)); }
		catch (caught) { error = String(caught); }
		finally { loading = false; }
	}
	async function prepare() {
		error = ""; loading = true; confirmed = false;
		try { await prepareMaintenance({ scheduleIds: selected, announce, waitForPlayers }); }
		catch (caught) { error = String(caught); }
		finally { loading = false; }
	}
	async function run() {
		if (!review || !confirmed) return;
		error = "";
		try { await runMaintenance(review); }
		catch (caught) { error = String(caught); }
	}
	function invalidateReview() { invalidateMaintenanceReview(); confirmed = false; }
	onMount(() => { void refresh(); });
	onDestroy(() => { if (!running) invalidateMaintenanceReview(); });
</script>

<div class="space-y-6">
	<header class="flex flex-wrap items-center justify-between gap-3 border-b border-border pb-4">
		<h1 class="text-2xl font-semibold">Maintenance Plans</h1>
		<Button variant="outline" disabled={loading || running} onclick={refresh}><RefreshCwIcon class={loading ? "animate-spin" : ""} />Refresh</Button>
	</header>
	{#if error && error !== maintenanceSession.run?.error}<Notice tone="error" title="Maintenance paused" message={error} />{/if}
	<Notice tone="warn" title="Review downtime and recovery" message="Maintenance stops the managed server before backing up selected databases and applying individually reviewed resource updates. Disable competing txAdmin restart schedules first. No SQL migrations or automatic rollbacks are performed. Resource snapshots do not restore database changes." />
	<section class="space-y-4" aria-label="Maintenance selection">
		<div class="flex flex-wrap items-center justify-between gap-3"><h2 class="text-lg font-semibold">Database backups</h2><Button variant="ghost" onclick={() => onNavigate("backup-manager")}>Backups & Restore</Button></div>
		<div class="divide-y divide-border border-y border-border">
			{#each schedules as schedule (schedule.config.id)}
				<label class="flex items-center gap-3 py-3">
					<Checkbox disabled={running || loading} checked={selected.includes(schedule.config.id)} onCheckedChange={(checked) => { selected = checked ? [...selected, schedule.config.id] : selected.filter((id) => id !== schedule.config.id); invalidateReview(); }} />
					<span class="min-w-0"><span class="block break-words text-sm font-medium">{schedule.config.name} / {schedule.config.database}</span><span class="block break-all text-xs text-muted-foreground">{schedule.config.outputDir}</span></span>
				</label>
			{:else}<p class="py-5 text-sm text-muted-foreground">No backup destinations configured for this workspace.</p>{/each}
		</div>
		<div class="flex flex-wrap items-center justify-between gap-3"><h2 class="text-lg font-semibold">Reviewed updates ({pending.length})</h2><Button variant="ghost" onclick={() => onNavigate("resource-manager")}>Resource Manager</Button></div>
		{#if pending.length}<ul class="space-y-2 text-sm">{#each pending as entry (entry.id)}<li class="flex flex-wrap justify-between gap-2 border-b border-border pb-2"><span class="break-all">{entry.name}</span><span class="text-muted-foreground">{entry.preview.changes.length} files / {entry.protectedPaths.length} protected</span></li>{/each}</ul>{:else}<p class="text-sm text-muted-foreground">No resource changes. This plan will back up and restart the server.</p>{/if}
		<div class="flex flex-wrap gap-x-6 gap-y-3 border-t border-border pt-4">
			<label class="flex items-center gap-2 text-sm"><Checkbox disabled={running || loading} bind:checked={announce} onCheckedChange={() => invalidateReview()} />Send maintenance announcement through RCON</label>
			<label class="flex items-center gap-2 text-sm"><Checkbox disabled={running || loading} bind:checked={waitForPlayers} onCheckedChange={() => invalidateReview()} />Wait for no connected players (Live Bridge, up to 10 minutes)</label>
		</div>
		<Button disabled={running || loading || !selected.length || Boolean(taskSession.exclusiveOwner)} onclick={prepare}><ClipboardCheckIcon />Review plan</Button>
	</section>
	{#if review}
		<section class="space-y-4 border-t border-border pt-5" aria-label="Reviewed maintenance plan">
			<h2 class="text-lg font-semibold">Ready for review</h2>
			<dl class="grid gap-3 text-sm sm:grid-cols-2"><div><dt class="text-muted-foreground">Profile / process</dt><dd class="break-all">{review.profile} / {review.pid}</dd></div><div><dt class="text-muted-foreground">Scope</dt><dd>{review.schedules.length} database backup(s), {review.updates.length} resource update(s)</dd></div><div><dt class="text-muted-foreground">Database connection</dt><dd class="break-all">{review.databaseTarget.username}@{review.databaseTarget.host}:{review.databaseTarget.port}</dd></div></dl>
			<label class="flex items-start gap-3 text-sm"><Checkbox bind:checked={confirmed} /><span>I reviewed the database selection and resource changes, accept downtime, and have disabled competing restart schedules.</span></label>
			<Button disabled={!confirmed || running} onclick={run}><PlayIcon />Run reviewed maintenance</Button>
		</section>
	{/if}
	{#if maintenanceSession.run && maintenanceSession.workspaceId === workspaceId}
		<section class="space-y-4 border-t border-border pt-5" aria-label="Maintenance progress">
			{#if maintenanceSession.run.error}<Notice tone="error" title="Maintenance paused" message={maintenanceSession.run.error} />{/if}
			<div class="flex flex-wrap items-center justify-between gap-3"><h2 class="text-lg font-semibold">Progress / {maintenanceSession.run.status}</h2><Button variant="outline" disabled={!running || maintenanceSession.run.cancelRequested} onclick={() => { if (maintenanceSession.run) maintenanceSession.run.cancelRequested = true; }}><SquareIcon />{maintenanceSession.run.cancelRequested ? "Stopping after current operation" : "Stop after current operation"}</Button></div>
			<ol class="divide-y divide-border">{#each maintenanceSession.run.steps as step (step.id)}<li class="flex gap-3 py-3"><span class="flex size-5 shrink-0 items-center justify-center">{#if step.status === "running"}<LoaderCircleIcon class="size-4 animate-spin text-sky-400" />{:else if step.status === "passed"}<CheckIcon class="size-4 text-emerald-400" />{/if}</span><div class="min-w-0"><p class="text-sm font-medium">{step.label} <span class="font-normal text-muted-foreground">/ {step.status}</span></p>{#if step.detail}<p class="mt-1 break-words text-xs text-muted-foreground">{step.detail}</p>{/if}</div></li>{/each}</ol>
			{#if maintenanceSession.run.status === "paused"}<Notice tone="warn" title="Manual review required" message="Completed backups and resource snapshots were kept. The server may be stopped or running depending on the completed steps. Check Manage Server, then review resource snapshots and database restores separately. A failed plan is never automatically replayed." /><div class="flex flex-wrap gap-2"><Button variant="outline" onclick={() => onNavigate("server-manage")}>Manage Server</Button><Button variant="outline" onclick={() => onNavigate("resource-manager")}>Resource snapshots</Button><Button variant="outline" onclick={() => onNavigate("backup-manager")}>Database restores</Button><Button variant="outline" onclick={() => onNavigate("diagnostics")}>Diagnostics</Button></div>{/if}
		</section>
	{/if}
</div>
