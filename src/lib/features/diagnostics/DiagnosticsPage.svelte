<script lang="ts">
	import ClipboardCheckIcon from "@lucide/svelte/icons/clipboard-check";
	import DownloadIcon from "@lucide/svelte/icons/download";
	import EyeIcon from "@lucide/svelte/icons/eye";
	import FileArchiveIcon from "@lucide/svelte/icons/file-archive";
	import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
	import FileCheck2Icon from "@lucide/svelte/icons/file-check-2";
	import XIcon from "@lucide/svelte/icons/x";
	import { save } from "@tauri-apps/plugin-dialog";
	import { onDestroy, onMount } from "svelte";
	import * as Card from "$lib/components/ui/card/index.js";
	import * as Select from "$lib/components/ui/select/index.js";
	import { Checkbox } from "$lib/components/ui/checkbox/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import { databaseSession } from "$lib/core/databaseSession.svelte";
	import { getInstallPath, loadInstallPath } from "$lib/core/paths.svelte";
	import { getFxserverStatus } from "$lib/modules/fxserver";
	import { fxserverSettings, loadFxserverSettings } from "$lib/features/fxserver/fxserverSettings.svelte";
	import { exportDiagnosticZip, previewDiagnosticExport, runPreflight, type DiagnosticPreview, type PreflightReport, type PreflightRequest } from "$lib/modules/diagnostics";
	import PreflightResults from "./PreflightResults.svelte";
	import ConfigDiff from "$lib/features/config-history/ConfigDiff.svelte";
	import { applyDiagnosticConfigPatch, previewDiagnosticConfigPatch, type DiagnosticConfigPatch } from "$lib/modules/diagnostics";
	import type { PageId } from "$lib/navigation";

	let { onNavigate }: { onNavigate?: (page: PageId) => void } = $props();

	let checking = $state(false);
	let preparing = $state(false);
	let exporting = $state(false);
	let useDatabase = $state(true);
	let includeApplicationLog = $state(false);
	let includeServerLog = $state(false);
	let report = $state<PreflightReport | null>(null);
	let preview = $state<DiagnosticPreview | null>(null);
	let selectedEntry = $state("manifest.json");
	let error = $state("");
	let message = $state("");
	let showPrivacyNotice = $state(true);
	let reportRequest = $state<PreflightRequest | null>(null);
	let patch = $state<DiagnosticConfigPatch | null>(null);
	let patchBusy = $state(false);
	let patchRevealed = $state(false);
	let patchReviewed = $state(false);
	let patchContext = "";
	let active = true;
	onDestroy(() => { active = false; });
	const context = $derived(JSON.stringify([getInstallPath(), fxserverSettings.txDataPath, fxserverSettings.profile]));
	const activeEntry = $derived(preview?.entries.find((entry) => entry.name === selectedEntry));
	$effect(() => { includeApplicationLog; includeServerLog; useDatabase; preview = null; });
	$effect(() => { context; report = null; reportRequest = null; preview = null; patch = null; });
	$effect(() => { patch; patchRevealed; patchReviewed = false; });

	onMount(() => { loadInstallPath(); loadFxserverSettings(); });

	function request(checkPorts = false): PreflightRequest {
		return {
			artifactPath: getInstallPath(),
			txDataPath: fxserverSettings.txDataPath,
			profile: fxserverSettings.profile,
			credentials: useDatabase && databaseSession.credentials ? { ...databaseSession.credentials } : null,
			checkPorts,
		};
	}

	async function runChecks() {
		if (!active || checking || patchBusy) return;
		checking = true;
		error = "";
		patch = null;
		const targetContext = context;
		const target = request();
		try {
			const status = await getFxserverStatus();
			if (!active || targetContext !== context) return;
			target.checkPorts = !status.running;
			const result = await runPreflight(target);
			if (active && targetContext === context) { report = result; reportRequest = { ...target, credentials: null, checkPorts: false }; }
		} catch (caught) { error = String(caught); }
		finally { checking = false; }
	}

	async function reviewPatch() {
		if (!active || !reportRequest || patchBusy || checking) return;
		patchBusy = true;
		error = "";
		message = "";
		patch = null;
		patchRevealed = false;
		const targetContext = context;
		try {
			const result = await previewDiagnosticConfigPatch({ ...reportRequest });
			if (active && targetContext === context) { patchContext = targetContext; patch = result; }
		} catch (caught) { error = String(caught); }
		finally { patchBusy = false; }
	}

	async function applyPatch() {
		if (!active || !patch || !patchReviewed || !patchRevealed || patchBusy || patchContext !== context) return;
		const reviewed = patch;
		patchBusy = true;
		error = "";
		let saved = false;
		try {
			const result = await applyDiagnosticConfigPatch(reviewed.id);
			message = `${result.name} patched. Previous content is preserved in encrypted history. No resource or service was started.`;
			saved = true;
		} catch (caught) { error = String(caught); }
		finally { patchBusy = false; patch = null; preview = null; }
		if (saved) await runChecks();
	}

	async function prepareExport() {
		if (!active || preparing) return;
		preparing = true;
		error = "";
		message = "";
		preview = null;
		const target = JSON.stringify([context, includeApplicationLog, includeServerLog, useDatabase]);
		try {
			const result = await previewDiagnosticExport({ preflight: request(), includeApplicationLog, includeServerLog });
			if (!active || target !== JSON.stringify([context, includeApplicationLog, includeServerLog, useDatabase])) return;
			preview = result;
			selectedEntry = preview.entries[0]?.name ?? "";
		} catch (caught) { error = String(caught); }
		finally { preparing = false; }
	}

	async function exportZip() {
		if (!active || !preview || exporting) return;
		const reviewed = preview;
		exporting = true;
		error = "";
		try {
			const path = await save({ title: "Export reviewed diagnostics", defaultPath: `fxserver-diagnostics-${new Date(reviewed.createdAt * 1000).toISOString().replace(/[:.]/g, "-")}.zip`, filters: [{ name: "Diagnostic archive", extensions: ["zip"] }] });
			if (!path || !active || preview?.id !== reviewed.id) return;
			const result = await exportDiagnosticZip(reviewed.id, path);
			message = `Diagnostic ZIP created: ${result.path}`;
			preview = null;
		} catch (caught) { error = String(caught); }
		finally { exporting = false; }
	}
</script>

<section class="space-y-6">
	<header class="flex flex-wrap items-end justify-between gap-3">
		<div><p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">FXServer</p><h1 class="mt-2 text-3xl font-semibold">Diagnostics</h1></div>
		<span class="max-w-full truncate text-sm text-muted-foreground">{fxserverSettings.profile || "No profile selected"}</span>
	</header>
	{#if error}<Notice tone="error" title="Diagnostics failed" message={error} onDismiss={() => error = ""} />{/if}
	{#if message}<Notice tone="success" message={message} onDismiss={() => message = ""} />{/if}

	<Card.Root>
		<Card.Header class="border-b border-border">
			<div class="flex flex-wrap items-center gap-3">
				<ClipboardCheckIcon class="size-5 text-muted-foreground" />
				<Card.Title class="flex-1">Preflight & Dependencies</Card.Title>
				<Button size="sm" onclick={runChecks} disabled={checking || patchBusy}>
					{#if checking}<LoaderCircleIcon class="animate-spin" />{:else}<ClipboardCheckIcon />{/if}
					{checking ? "Checking" : "Run checks"}
				</Button>
			</div>
		</Card.Header>
		<Card.Content class="space-y-5">
			<div class="flex flex-wrap items-center justify-between gap-3">
				<label class="flex items-center gap-2 text-sm"><Checkbox bind:checked={useDatabase} disabled={!databaseSession.credentials || preparing || exporting} />Check session database connection</label>
				{#if report}<span class="text-xs text-muted-foreground">Checked {new Date(report.checkedAt * 1000).toLocaleTimeString()}</span>{/if}
			</div>
			{#if report}<PreflightResults {report} {onNavigate} onReviewPatch={reviewPatch} disabled={checking || patchBusy} />{:else}<p class="py-6 text-center text-sm text-muted-foreground">No diagnostic results yet.</p>{/if}
		</Card.Content>
	</Card.Root>

	{#if patchBusy && !patch}<p class="flex items-center gap-2 text-sm" role="status"><LoaderCircleIcon class="size-4 animate-spin" />Preparing reviewed patch...</p>{/if}
	{#if patch}
		<section class="min-w-0 space-y-4 border-y border-border py-5" aria-label="Review configuration repair">
			<div class="flex items-center justify-between gap-3"><h2 class="text-base font-semibold">Review rconlog startup patch</h2><Button variant="ghost" size="icon-sm" aria-label="Discard patch" title="Discard patch" disabled={patchBusy} onclick={() => patch = null}><XIcon /></Button></div>
			<p class="font-mono text-xs wrap-anywhere text-muted-foreground">{patch.path}</p>
			<p class="text-xs leading-5 text-muted-foreground">Adds only <code>ensure rconlog</code>. FXServer must be stopped. Credentials and services remain unchanged. Previous content is saved to encrypted configuration history.</p>
			<label class="flex items-center gap-2 text-xs"><Checkbox bind:checked={patchRevealed} disabled={patchBusy} />Reveal config contents, including secrets</label>
			{#if patchRevealed}<ConfigDiff before={patch.before} after={patch.after} beforeLabel="Current file" afterLabel="Reviewed repair" />{/if}
			<label class="flex items-center gap-2 text-xs"><Checkbox bind:checked={patchReviewed} disabled={!patchRevealed || patchBusy} />I reviewed this exact file change.</label>
			<div class="flex flex-wrap items-center justify-between gap-3">
				<span class="text-xs text-muted-foreground">Review expires at {new Date(patch.expiresAt * 1000).toLocaleTimeString()}</span>
				<Button onclick={applyPatch} disabled={!patchReviewed || !patchRevealed || patchBusy}>{#if patchBusy}<LoaderCircleIcon class="animate-spin" />{:else}<FileCheck2Icon />{/if}Apply reviewed patch</Button>
			</div>
		</section>
	{/if}

	<Card.Root>
		<Card.Header class="border-b border-border">
			<div class="flex items-center gap-3"><FileArchiveIcon class="size-5 text-muted-foreground" /><Card.Title>Diagnostic Export</Card.Title></div>
		</Card.Header>
		<Card.Content class="space-y-4">
			{#if showPrivacyNotice}<Notice tone="warn" title="Review before sharing" message="Summaries exclude config values and credential files. Optional logs are redacted automatically, but may contain other private information. Review the preview before exporting or sharing." onDismiss={() => showPrivacyNotice = false} />{/if}
			<div class="flex flex-wrap items-center gap-x-6 gap-y-3">
				<label class="flex items-center gap-2 text-sm"><Checkbox bind:checked={includeApplicationLog} disabled={preparing || exporting} />Application log</label>
				<label class="flex items-center gap-2 text-sm"><Checkbox bind:checked={includeServerLog} disabled={preparing || exporting} />Server log</label>
				<Button class="ml-auto" variant="outline" size="sm" onclick={prepareExport} disabled={preparing || exporting}>
					{#if preparing}<LoaderCircleIcon class="animate-spin" />{:else}<EyeIcon />{/if}Prepare preview
				</Button>
			</div>
			{#if preview}
				<div class="flex flex-wrap items-center justify-between gap-3 border-t border-border pt-4">
					<Select.Root type="single" bind:value={selectedEntry}>
						<Select.Trigger class="w-full sm:w-64" aria-label="Preview file">{selectedEntry}</Select.Trigger>
						<Select.Content>{#each preview.entries as entry}<Select.Item value={entry.name}>{entry.name}</Select.Item>{/each}</Select.Content>
					</Select.Root>
				<span class="text-xs text-muted-foreground">{preview.entries.length} files / {Math.ceil(preview.totalBytes / 1024)} KiB</span>
				</div>
				<textarea class="min-h-40 w-full resize-y rounded-sm border border-border bg-background p-3 font-mono text-xs leading-5 wrap-anywhere outline-none focus-visible:ring-2 focus-visible:ring-ring" rows="14" readonly aria-label="Reviewed diagnostic contents" value={activeEntry?.content ?? ""}></textarea>
				<div class="flex flex-wrap items-center justify-between gap-3">
					<span class="text-xs text-muted-foreground">Snapshot expires at {new Date(preview.expiresAt * 1000).toLocaleTimeString()}</span>
					<Button onclick={exportZip} disabled={exporting || preparing}>{#if exporting}<LoaderCircleIcon class="animate-spin" />{:else}<DownloadIcon />{/if}Export reviewed ZIP</Button>
				</div>
			{/if}
		</Card.Content>
	</Card.Root>
</section>
