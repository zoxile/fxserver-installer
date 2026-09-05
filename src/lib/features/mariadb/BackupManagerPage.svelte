<script lang="ts">
	import { onMount } from "svelte";
	import { listen } from "@tauri-apps/api/event";
	import ArchiveIcon from "@lucide/svelte/icons/archive";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
	import PauseIcon from "@lucide/svelte/icons/pause";
	import PencilIcon from "@lucide/svelte/icons/pencil";
	import PlayIcon from "@lucide/svelte/icons/play";
	import PlusIcon from "@lucide/svelte/icons/plus";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import RotateCcwIcon from "@lucide/svelte/icons/rotate-ccw";
	import SaveIcon from "@lucide/svelte/icons/save";
	import Trash2Icon from "@lucide/svelte/icons/trash-2";
	import XIcon from "@lucide/svelte/icons/x";
	import * as Card from "$lib/components/ui/card/index.js";
	import * as Select from "$lib/components/ui/select/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Checkbox } from "$lib/components/ui/checkbox/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import ConnectionCard from "./ConnectionCard.svelte";
	import { databaseSession, rememberDatabaseCredentials } from "$lib/core/databaseSession.svelte";
	import { chooseFolder } from "$lib/core/selectFolder";
	import { getWorkspaceId } from "$lib/core/workspaces.svelte";
	import { listMariaDBDatabases, validateMariaDBCredentials, type MariaDBCredentials } from "$lib/modules/mariadb";
	import {
		getBackupManager, previewBackupRestore, removeBackupSchedule,
		restoreBackupSnapshot, runBackupNow, saveBackupSchedule,
		type BackupEvent, type BackupOverview, type BackupSchedule, type BackupSnapshot,
		type RestorePreview, type ScheduleStatus,
	} from "$lib/modules/backupManager";

	const workspaceId = getWorkspaceId();
	const systemDatabases = new Set(["mysql", "sys", "information_schema", "performance_schema"]);
	let credentials = $state<MariaDBCredentials>({ ...databaseSession.defaults, password: "", ...databaseSession.credentials });
	let validated = $state("");
	let databases = $state<string[]>([]);
	let overview = $state<BackupOverview>({ schedules: [], snapshots: [], busy: false });
	let config = $state<BackupSchedule>(newSchedule());
	let enabled = $state(false);
	let busy = $state(false);
	let refreshing = $state(false);
	let error = $state("");
	let connectionError = $state("");
	let message = $state("");
	let stage = $state("");
	let noticeDismissed = $state(false);
	let preview = $state<RestorePreview | null>(null);
	let confirmation = $state("");
	let snapshotsShown = $state(25);
	let active = true;
	let refreshPending: Promise<void> | undefined;
	const credentialsReady = $derived(Boolean(validated) && JSON.stringify(credentials) === validated);
	const databaseOptions = $derived(databases.map((database) => ({ value: database, label: database })));
	const working = $derived(busy || overview.busy);

	function newSchedule(): BackupSchedule {
		return { id: crypto.randomUUID(), workspaceId, name: "", database: "", outputDir: "", intervalMinutes: 60, retainCount: 7 };
	}

	onMount(() => {
		active = true;
		let unlisten: (() => void) | undefined;
		let refreshTimer: number | undefined;
		void refresh();
		if ("__TAURI_INTERNALS__" in window) {
			void listen<BackupEvent>("backup-manager-progress", ({ payload }) => {
				if (!active || payload.workspaceId !== workspaceId) return;
				stage = payload.message;
				if (payload.stage === "error") error = payload.message;
				window.clearTimeout(refreshTimer);
				refreshTimer = window.setTimeout(() => void refresh(), 150);
			}).then((stop) => { if (active) unlisten = stop; else stop(); }).catch((caught) => { if (active) error = String(caught); });
		}
		const interval = window.setInterval(() => { if (!document.hidden) void refresh(); }, 15_000);
		if (databaseSession.credentials) void connect();
		return () => { active = false; unlisten?.(); window.clearInterval(interval); window.clearTimeout(refreshTimer); };
	});

	async function refresh() {
		if (refreshPending) return refreshPending;
		refreshing = true;
		refreshPending = getBackupManager(workspaceId).then((value) => { if (active) overview = value; })
			.catch((caught) => { if (active) error = String(caught); })
			.finally(() => { refreshing = false; refreshPending = undefined; });
		return refreshPending;
	}

	async function action(work: () => Promise<void>) {
		if (busy) return;
		busy = true;
		error = "";
		message = "";
		try { await work(); }
		catch (caught) { if (active) error = caught instanceof Error ? caught.message : String(caught); }
		finally { if (active) { busy = false; await refresh(); } }
	}

	async function connect() {
		await action(async () => {
			connectionError = "";
			const original = { ...credentials };
			const signature = JSON.stringify(original);
			const requested = { ...original, database: null };
			try {
				await validateMariaDBCredentials(requested);
				const available = await listMariaDBDatabases(requested);
				if (!active || JSON.stringify(credentials) !== signature) return;
				databases = available.filter((name) => !systemDatabases.has(name.toLowerCase()));
				validated = signature;
				rememberDatabaseCredentials(original);
				if (!config.database && databases[0]) config.database = databases[0];
			} catch (caught) { validated = ""; connectionError = String(caught); throw caught; }
		});
	}

	async function browse() {
		try { const folder = await chooseFolder(config.outputDir); if (folder) config.outputDir = folder; }
		catch (caught) { error = String(caught); }
	}

	async function save() {
		await action(async () => {
			await saveBackupSchedule({ ...config }, enabled, enabled ? { ...credentials } : undefined);
			message = enabled ? "Schedule enabled for this app session." : "Schedule saved and paused.";
		});
	}

	function edit(schedule: ScheduleStatus) {
		config = { ...schedule.config };
		enabled = schedule.enabled;
	}

	async function toggle(schedule: ScheduleStatus) {
		await action(async () => {
			await saveBackupSchedule({ ...schedule.config }, !schedule.enabled, schedule.enabled ? undefined : { ...credentials });
			if (config.id === schedule.config.id) enabled = !schedule.enabled;
		});
	}

	async function remove(schedule: ScheduleStatus) {
		if (!window.confirm(`Remove schedule "${schedule.config.name}"? Its backup files will be kept.`)) return;
		await action(async () => {
			await removeBackupSchedule(workspaceId, schedule.config.id);
			if (config.id === schedule.config.id) { config = newSchedule(); enabled = false; }
		});
	}

	async function runNow(schedule: ScheduleStatus) {
		await action(async () => {
			await runBackupNow(workspaceId, schedule.config.id, { ...credentials });
			message = "Backup created and verified.";
		});
	}

	async function review(snapshot: BackupSnapshot) {
		await action(async () => {
			preview = null;
			confirmation = "";
			preview = await previewBackupRestore(workspaceId, snapshot.id, { ...credentials });
		});
	}

	async function restore() {
		const selected = preview;
		if (!selected || confirmation !== selected.targetDatabase) return;
		await action(async () => {
			const result = await restoreBackupSnapshot(workspaceId, selected.token, confirmation);
			message = `${result.message} Recovery backup: ${result.recoverySnapshot.directory}/${result.recoverySnapshot.id}.sql`;
			preview = null;
			confirmation = "";
		});
	}

	function date(value: number | null) { return value ? new Date(value).toLocaleString() : "Not yet"; }
	function size(value: number) { return `${(value / 1048576).toLocaleString(undefined, { maximumFractionDigits: 1 })} MiB`; }
</script>

<section class="space-y-6">
	<header class="flex items-center justify-between gap-3">
		<div><p class="text-xs font-semibold text-muted-foreground uppercase">MariaDB</p><h1 class="mt-2 text-3xl font-semibold">Backups &amp; Restore</h1></div>
		<Button variant="outline" size="icon" onclick={refresh} disabled={refreshing} title="Refresh schedules and snapshots" aria-label="Refresh backups">
			{#if refreshing}<LoaderCircleIcon class="animate-spin" />{:else}<RefreshCwIcon />{/if}
		</Button>
	</header>
	{#if !noticeDismissed}<Notice tone="warn" title="Session-only scheduling" message="Schedules run while this app is open or in the tray. After restarting the app, schedules are paused until you validate credentials and enable them. Passwords are never saved with schedules." onDismiss={() => (noticeDismissed = true)} />{/if}
	{#if error}<Notice tone="error" message={error} onDismiss={() => (error = "")} />{/if}
	{#if message}<Notice tone="success" {message} onDismiss={() => (message = "")} />{/if}
	{#if working && stage}<div role="status" class="flex items-center gap-2 text-sm text-muted-foreground"><LoaderCircleIcon class="size-4 shrink-0 animate-spin" /><span>{stage}</span></div>{/if}

	<div class="grid items-start gap-4 xl:grid-cols-2">
		<ConnectionCard bind:credentials busy={working} {credentialsReady} {connectionError} stretch={false} onApply={connect} />
		<Card.Root class="min-w-0 rounded-md">
			<Card.Header class="border-b border-border pb-4">
				<div class="flex items-center justify-between gap-3"><Card.Title>Schedule</Card.Title><Button size="icon-sm" variant="ghost" title="New schedule" aria-label="New schedule" disabled={working} onclick={() => { config = newSchedule(); enabled = false; }}><PlusIcon /></Button></div>
			</Card.Header>
			<Card.Content class="space-y-4">
				<label class="grid gap-2 text-xs font-medium">Name<Input bind:value={config.name} maxlength={100} placeholder="Hourly production backup" disabled={working} /></label>
				<label class="grid gap-2 text-xs font-medium">Database
					<Select.Root type="single" bind:value={config.database} items={databaseOptions} disabled={working || !credentialsReady}>
						<Select.Trigger class="w-full font-mono text-xs">{config.database || "Choose database"}</Select.Trigger>
						<Select.Content>{#each databaseOptions as option}<Select.Item value={option.value} label={option.label}>{option.label}</Select.Item>{/each}</Select.Content>
					</Select.Root>
				</label>
				<div class="space-y-2"><label for="schedule-folder" class="text-xs font-medium">Backup folder</label><div class="flex gap-2"><Input id="schedule-folder" bind:value={config.outputDir} class="min-w-0 font-mono text-xs" disabled={working} /><Button size="icon" variant="outline" title="Browse backup folder" aria-label="Browse backup folder" onclick={browse} disabled={working}><FolderOpenIcon /></Button></div></div>
				<div class="grid grid-cols-2 gap-4">
					<label class="grid gap-2 text-xs font-medium">Interval (minutes)<Input type="number" min={5} max={10080} step={1} bind:value={config.intervalMinutes} disabled={working} /></label>
					<label class="grid gap-2 text-xs font-medium">Backups to retain<Input type="number" min={1} max={100} step={1} bind:value={config.retainCount} disabled={working} /></label>
				</div>
				<label class="flex items-center gap-2 text-sm"><Checkbox bind:checked={enabled} disabled={working || !credentialsReady} />Enabled for this app session</label>
				<Button onclick={save} disabled={working || !config.name.trim() || !config.database || !config.outputDir || (enabled && !credentialsReady)}><SaveIcon />Save Schedule</Button>
			</Card.Content>
		</Card.Root>
	</div>

	<section class="space-y-3" aria-label="Saved backup schedules">
		<h2 class="text-base font-semibold">Saved Schedules <span class="ml-2 text-sm font-normal text-muted-foreground">{overview.schedules.length}</span></h2>
		{#if overview.schedules.length === 0}<p class="border-y border-border py-5 text-sm text-muted-foreground">No saved schedules in this workspace.</p>{/if}
		<div class="divide-y divide-border border-y border-border">
			{#each overview.schedules as schedule (schedule.config.id)}
				<div class="grid min-w-0 gap-3 py-4 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto]">
					<div class="min-w-0"><p class="truncate font-medium" title={schedule.config.name}>{schedule.config.name}</p><p class="mt-1 text-xs text-muted-foreground">{schedule.config.database} / Every {schedule.config.intervalMinutes} min / Keep {schedule.config.retainCount}</p></div>
					<div class="min-w-0 text-xs"><p class={schedule.enabled ? "text-emerald-400" : "text-muted-foreground"}>{schedule.running ? "Backing up" : schedule.enabled ? `Next: ${date(schedule.nextRun)}` : "Paused"}</p><p class="mt-1 text-muted-foreground">Last attempt: {date(schedule.lastRun)}</p>{#if schedule.lastError}<p class="mt-1 wrap-anywhere text-destructive">{schedule.lastError}</p>{/if}</div>
					<div class="flex flex-wrap items-center gap-1">
						<Button variant="outline" size="sm" onclick={() => runNow(schedule)} disabled={working || !credentialsReady} title="Create backup now"><ArchiveIcon />Back Up</Button>
						<Button variant="ghost" size="icon-sm" onclick={() => toggle(schedule)} disabled={working || (!schedule.enabled && !credentialsReady)} title={schedule.enabled ? "Pause schedule" : "Enable schedule for this session"} aria-label={schedule.enabled ? "Pause schedule" : "Enable schedule"}>{#if schedule.enabled}<PauseIcon />{:else}<PlayIcon />{/if}</Button>
						<Button variant="ghost" size="icon-sm" onclick={() => edit(schedule)} disabled={working} title="Edit schedule" aria-label="Edit schedule"><PencilIcon /></Button>
						<Button variant="destructive" size="icon-sm" onclick={() => remove(schedule)} disabled={working} title="Remove schedule and keep backups" aria-label="Remove schedule"><Trash2Icon /></Button>
					</div>
				</div>
			{/each}
		</div>
	</section>

	{#if preview}
		<section class="space-y-4 border-y border-amber-400/40 py-5" aria-label="Restore preview">
			<div class="flex items-center justify-between gap-3"><h2 class="text-base font-semibold">Restore Preview</h2><Button size="icon-sm" variant="ghost" disabled={working} onclick={() => { preview = null; confirmation = ""; }} title="Close restore preview" aria-label="Close restore preview"><XIcon /></Button></div>
			<dl class="grid gap-4 text-sm sm:grid-cols-2"><div><dt class="text-muted-foreground">Target</dt><dd class="mt-1 wrap-anywhere font-mono">{preview.targetHost}:{preview.targetPort} / {preview.targetDatabase}</dd></div><div><dt class="text-muted-foreground">Snapshot</dt><dd class="mt-1">{date(preview.snapshot.createdAt)} / {size(preview.snapshot.sizeBytes)}</dd></div><div><dt class="text-muted-foreground">Existing tables</dt><dd class="mt-1">{preview.existingTables}</dd></div><div><dt class="text-muted-foreground">Verification expires</dt><dd class="mt-1">{date(preview.expiresAt)}</dd></div></dl>
			<ul class="list-disc space-y-1 pl-5 text-sm text-amber-200">{#each preview.warnings as warning}<li>{warning}</li>{/each}</ul>
			<div class="flex flex-wrap items-end gap-3"><label class="grid min-w-0 flex-1 gap-2 text-xs font-medium">Confirm database name: {preview.targetDatabase}<Input bind:value={confirmation} autocomplete="off" spellcheck="false" disabled={working} /></label><Button variant="destructive" onclick={restore} disabled={working || confirmation !== preview.targetDatabase}><RotateCcwIcon />Back Up & Restore</Button></div>
		</section>
	{/if}

	<section class="space-y-3" aria-label="Database backup snapshots">
		<h2 class="text-base font-semibold">Snapshots <span class="ml-2 text-sm font-normal text-muted-foreground">{overview.snapshots.length}</span></h2>
		{#if overview.snapshots.length === 0}<p class="border-y border-border py-5 text-sm text-muted-foreground">No managed snapshots in this workspace.</p>{/if}
		<div class="divide-y divide-border border-y border-border">
			{#each overview.snapshots.slice(0, snapshotsShown) as snapshot (snapshot.id)}
				<div class="grid min-w-0 gap-3 py-4 sm:grid-cols-[minmax(0,1fr)_auto]">
					<div class="min-w-0"><div class="flex flex-wrap items-center gap-2"><span class="font-medium">{snapshot.database}</span><span class="text-xs text-muted-foreground">{date(snapshot.createdAt)} / {size(snapshot.sizeBytes)}</span>{#if snapshot.kind === "recovery"}<span class="text-xs text-amber-300">Recovery / retained</span>{/if}</div><p class="mt-1 truncate font-mono text-xs text-muted-foreground" title={`${snapshot.directory}/${snapshot.id}.sql`}>{snapshot.directory}/{snapshot.id}.sql</p><p class="mt-1 text-xs text-muted-foreground">Source: {snapshot.sourceHost}:{snapshot.sourcePort}</p></div>
					<Button variant="outline" size="sm" onclick={() => review(snapshot)} disabled={working || !credentialsReady} title="Verify this snapshot and preview the restore"><RotateCcwIcon />Review Restore</Button>
				</div>
			{/each}
		</div>
		{#if overview.snapshots.length > snapshotsShown}<Button variant="outline" onclick={() => (snapshotsShown += 25)}><PlusIcon />More Snapshots</Button>{/if}
	</section>
</section>
