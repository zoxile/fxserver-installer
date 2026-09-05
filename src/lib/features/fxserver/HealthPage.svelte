<script lang="ts">
	import { onMount } from "svelte";
	import ActivityIcon from "@lucide/svelte/icons/activity";
	import CpuIcon from "@lucide/svelte/icons/cpu";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import HardDriveIcon from "@lucide/svelte/icons/hard-drive";
	import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
	import MemoryStickIcon from "@lucide/svelte/icons/memory-stick";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import SaveIcon from "@lucide/svelte/icons/save";
	import ShieldCheckIcon from "@lucide/svelte/icons/shield-check";
	import Trash2Icon from "@lucide/svelte/icons/trash-2";
	import { Button } from "$lib/components/ui/button/index.js";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Checkbox } from "$lib/components/ui/checkbox/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import { chooseFolder } from "$lib/core/selectFolder";
	import { clearHealthEvents, configureHealth, defaultHealthConfig, getHealthStatus, type HealthStatus } from "$lib/modules/health";

	let status = $state<HealthStatus | null>(null);
	let config = $state({ ...defaultHealthConfig });
	let loading = $state(false);
	let saving = $state(false);
	let error = $state("");
	let message = $state("");
	let showSessionNotice = $state(true);
	let active = false;
	let requestPending = false;
	let updateRevision = 0;
	const dirty = $derived(status !== null && JSON.stringify(config) !== JSON.stringify(status.config));
	const recoveryLabel = $derived(status?.recoveryBlocked ? "Retry limit reached" : status?.recoveryArmed ? "Armed" : "Disarmed");

	function errorMessage(value: unknown) {
		return value instanceof Error ? value.message : String(value);
	}

	async function refresh(loadConfig = false, showLoading = false) {
		if (requestPending || saving) return;
		requestPending = true;
		const revision = updateRevision;
		if (showLoading) loading = true;
		try {
			const result = await getHealthStatus();
			if (!active || revision !== updateRevision) return;
			if (loadConfig || status?.workspaceId !== result.workspaceId) config = { ...result.config };
			status = result;
		} catch (cause) {
			if (active) error = errorMessage(cause);
		} finally {
			requestPending = false;
			loading = false;
		}
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		if (!status || saving) return;
		saving = true;
		updateRevision += 1;
		error = "";
		try {
			const result = await configureHealth({ ...config }, status.workspaceId);
			if (!active) return;
			status = result;
			config = { ...result.config };
			message = "Health settings applied for this session.";
		} catch (cause) {
			if (active) error = errorMessage(cause);
		} finally {
			saving = false;
		}
	}

	async function browse() {
		try {
			const path = await chooseFolder(config.diskPath);
			if (path && active) config.diskPath = path;
		} catch (cause) {
			if (active) error = errorMessage(cause);
		}
	}

	async function clear() {
		try {
			await clearHealthEvents();
			if (active && status) status.events = [];
		} catch (cause) {
			if (active) error = errorMessage(cause);
		}
	}

	function metric(value: number | null | undefined, suffix: string) {
		return value == null ? "Not sampled" : `${value.toFixed(1)}${suffix}`;
	}

	onMount(() => {
		active = true;
		void refresh(true, true);
		const timer = setInterval(() => void refresh(), 5000);
		return () => {
			active = false;
			clearInterval(timer);
		};
	});
</script>

<section class="space-y-6">
	<header class="flex items-center justify-between gap-4">
		<div>
			<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">FXServer</p>
			<h1 class="mt-2 text-3xl font-semibold">Health &amp; Recovery</h1>
		</div>
		<Button variant="outline" size="icon" title="Refresh health status" aria-label="Refresh health status" onclick={() => refresh(false, true)} disabled={loading || saving}>
			{#if loading}<LoaderCircleIcon class="animate-spin" />{:else}<RefreshCwIcon />{/if}
		</Button>
	</header>

	{#if showSessionNotice}
		<Notice tone="info" title="Session monitoring" message="Monitoring runs while the app is open or in the tray. It is disabled after quitting or switching workspaces. Recovery only applies to servers started by this app; manual stops never trigger it." onDismiss={() => showSessionNotice = false} />
	{/if}
	{#if error}<Notice tone="error" message={error} onDismiss={() => error = ""} />{/if}
	{#if message}<Notice tone="success" {message} onDismiss={() => message = ""} />{/if}

	<div class="grid grid-cols-1 gap-4 border-y border-border py-4 sm:grid-cols-3">
		<div class="flex items-center gap-3"><CpuIcon class="size-5 shrink-0 text-cyan-400" /><div><p class="text-xs text-muted-foreground">FXServer CPU</p><p class="font-mono text-lg tabular-nums">{metric(status?.sample?.cpuPercent, "%")}</p></div></div>
		<div class="flex items-center gap-3"><MemoryStickIcon class="size-5 shrink-0 text-emerald-400" /><div><p class="text-xs text-muted-foreground">FXServer RAM</p><p class="font-mono text-lg tabular-nums">{metric(status?.sample?.memoryPercent, "%")}</p></div></div>
		<div class="flex items-center gap-3"><HardDriveIcon class="size-5 shrink-0 text-amber-400" /><div><p class="text-xs text-muted-foreground">Available disk</p><p class="font-mono text-lg tabular-nums">{metric(status?.sample?.freeDiskGb, " GiB")}</p></div></div>
		{#if status?.sample}<p class="text-xs text-muted-foreground sm:col-span-3">Last sampled {new Date(status.sample.timestamp).toLocaleTimeString()}</p>{/if}
	</div>

	<form onsubmit={save} class="space-y-5">
		<fieldset disabled={!status || saving} class="grid min-w-0 gap-6 xl:grid-cols-2">
			<section class="min-w-0 space-y-4">
				<h2 class="flex items-center gap-2 text-base font-semibold"><ActivityIcon class="size-4" />Health Alerts</h2>
				<div class="flex items-center gap-2"><Checkbox id="health-alerts" bind:checked={config.alertsEnabled} /><label for="health-alerts" class="text-sm">Enable health alerts</label></div>
				<div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
					<div class="space-y-2"><label for="health-cpu" class="text-xs text-muted-foreground">CPU threshold (%)</label><Input id="health-cpu" type="number" min={1} max={100} step={1} required bind:value={config.cpuThresholdPercent} /></div>
					<div class="space-y-2"><label for="health-memory" class="text-xs text-muted-foreground">RAM threshold (% of physical memory)</label><Input id="health-memory" type="number" min={1} max={100} step={1} required bind:value={config.memoryThresholdPercent} /></div>
					<div class="space-y-2"><label for="health-disk-minimum" class="text-xs text-muted-foreground">Minimum free disk (GiB, 0 disables)</label><Input id="health-disk-minimum" type="number" min={0} max={1000000} step={0.1} required bind:value={config.minimumFreeDiskGb} /></div>
					<div class="space-y-2"><label for="health-sustain" class="text-xs text-muted-foreground">Sustained period (seconds)</label><Input id="health-sustain" type="number" min={10} max={600} step={5} required bind:value={config.sustainedSeconds} /></div>
				</div>
				<div class="space-y-2"><label for="health-disk-path" class="text-xs text-muted-foreground">Disk folder</label><div class="flex gap-2"><Input id="health-disk-path" class="min-w-0 flex-1" bind:value={config.diskPath} required={config.alertsEnabled && config.minimumFreeDiskGb > 0} /><Button variant="outline" size="icon" title="Choose disk folder" aria-label="Choose disk folder" onclick={browse}><FolderOpenIcon /></Button></div></div>
				<div class="space-y-2"><label for="health-cooldown" class="text-xs text-muted-foreground">Alert cooldown (seconds)</label><Input id="health-cooldown" type="number" min={30} max={3600} step={5} required bind:value={config.alertCooldownSeconds} /></div>
			</section>
			<section class="min-w-0 space-y-4 xl:border-l xl:border-border xl:pl-6">
				<div class="flex items-center justify-between gap-3"><h2 class="flex items-center gap-2 text-base font-semibold"><ShieldCheckIcon class="size-4" />Crash Recovery</h2><span class={`text-xs ${status?.recoveryBlocked ? "text-amber-400" : status?.recoveryArmed ? "text-emerald-400" : "text-muted-foreground"}`}>{recoveryLabel}</span></div>
				<div class="flex items-center gap-2"><Checkbox id="health-recovery" bind:checked={config.recoveryEnabled} /><label for="health-recovery" class="text-sm">Restart after an unexpected exit</label></div>
				<div class="space-y-2"><label for="health-backoff" class="text-xs text-muted-foreground">Delay before each attempt (seconds)</label><Input id="health-backoff" type="number" min={10} max={300} step={5} required bind:value={config.recoveryBackoffSeconds} /></div>
				<dl class="divide-y divide-border text-sm">
					<div class="flex justify-between gap-3 py-3"><dt class="text-muted-foreground">Maximum attempts</dt><dd>3 in 10 minutes</dd></div>
					<div class="flex justify-between gap-3 py-3"><dt class="text-muted-foreground">Recent attempts</dt><dd class="tabular-nums">{status?.recoveryAttempts ?? 0} / 3</dd></div>
					<div class="flex justify-between gap-3 py-3"><dt class="text-muted-foreground">Next attempt</dt><dd>{status?.nextRecoverySeconds == null ? "None scheduled" : `In ${status.nextRecoverySeconds}s`}</dd></div>
					<div class="flex justify-between gap-3 py-3"><dt class="text-muted-foreground">Server process</dt><dd>{status?.sample?.running ? `Running (${status.sample.pid})` : status?.sample ? "Stopped" : "Not sampled"}</dd></div>
				</dl>
			</section>
		</fieldset>
		<div class="flex justify-end border-t border-border pt-4"><Button type="submit" disabled={!status || saving || !dirty}>{#if saving}<LoaderCircleIcon class="animate-spin" />{:else}<SaveIcon />{/if}Apply Settings</Button></div>
	</form>

	<Card.Root class="rounded-md">
		<Card.Header class="flex flex-row items-center justify-between gap-3 border-b border-border pb-4">
			<Card.Title>Health Events</Card.Title>
			<Button variant="ghost" size="icon-sm" title="Clear health events" aria-label="Clear health events" onclick={clear} disabled={!status?.events.length}><Trash2Icon /></Button>
		</Card.Header>
		<Card.Content>
			{#if status?.events.length}
				<ol class="max-h-96 divide-y divide-border overflow-y-auto">
					{#each status.events as entry (entry.id)}
						<li class="grid gap-1 py-3 text-sm sm:grid-cols-[10rem_minmax(0,1fr)] sm:gap-4"><time datetime={new Date(entry.timestamp).toISOString()} class="text-xs text-muted-foreground tabular-nums">{new Date(entry.timestamp).toLocaleString()}</time><p class={`wrap-break-word ${entry.level === "error" ? "text-red-400" : entry.level === "warn" ? "text-amber-300" : "text-foreground"}`}>{entry.message}</p></li>
					{/each}
				</ol>
			{:else}<p class="py-6 text-center text-sm text-muted-foreground">No health events recorded.</p>{/if}
		</Card.Content>
	</Card.Root>
</section>
