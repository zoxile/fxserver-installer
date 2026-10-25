<script lang="ts">
	import { onMount } from "svelte";
	import DownloadIcon from "@lucide/svelte/icons/download";
	import ExternalLinkIcon from "@lucide/svelte/icons/external-link";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
	import PackageIcon from "@lucide/svelte/icons/package";
	import GaugeIcon from "@lucide/svelte/icons/gauge";
	import GitBranchIcon from "@lucide/svelte/icons/git-branch";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Progress } from "$lib/components/ui/progress/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import { appUpdate, checkForAppUpdate, initializeAppUpdate, updateAndRestart, updatePhaseBusy } from "$lib/core/appUpdate.svelte";
	import { openExternalUrl } from "$lib/core/openExternal";

	let { betaBuild }: { betaBuild: boolean } = $props();
	let confirming = $state(false);
	let dismissedError = $state("");
	const status = $derived(appUpdate.status);
	const active = $derived(appUpdate.working || appUpdate.loading || updatePhaseBusy(status.phase));
	const available = $derived(!!status.availableVersion && status.supported);
	const updateError = $derived(appUpdate.error || (status.phase === "error" ? status.message : "") || "");
	const percentage = $derived(status.totalBytes ? Math.min(100, 100 * status.downloadedBytes / status.totalBytes) : null);
	const stage = $derived(appUpdate.loading ? "Checking update support" : {
		disabled: "Manual updates", idle: "Up to date", checking: "Checking for updates", available: `Version ${status.availableVersion} available`,
		downloading: "Downloading and verifying", ready: "Ready to install", stopping: "Stopping background services", installing: "Installing update", error: "Update needs attention",
	}[status.phase]);
	const projectDetails = $derived([
		{ label: "App", value: "FXServer Installer", icon: PackageIcon },
		{ label: "Version", value: status.currentVersion, icon: GaugeIcon },
		{ label: "Git", value: "zoxile/fxserver-installer", icon: GitBranchIcon },
	]);
	onMount(() => { void initializeAppUpdate(); });
	function confirmUpdate() { confirming = false; void updateAndRestart(); }
</script>

<Card.Root class="flex min-w-0 flex-col rounded-md border-border bg-card shadow-sm" aria-label="Project and app updates">
	<Card.Header class="border-b border-border pb-4">
		<div class="flex items-center justify-between gap-2"><Card.Title>Project</Card.Title>{#if betaBuild}<span class="rounded-sm border border-amber-400/30 bg-amber-400/10 px-2 py-0.5 text-xs font-medium text-amber-300">Beta</span>{/if}</div>
		<Card.Description>Application version and updates.</Card.Description>
	</Card.Header>
	<Card.Content class="flex flex-1 flex-col gap-3">
		<dl class="divide-y divide-border">
			{#each projectDetails as detail}
				{@const Icon = detail.icon}
				<div class="flex min-w-0 items-center justify-between gap-3 py-2">
					<dt class="flex shrink-0 items-center gap-2 text-xs text-muted-foreground"><Icon class="size-3.5" />{detail.label}</dt>
					<dd class="min-w-0 truncate text-xs font-medium" title={detail.value}>{detail.value}</dd>
				</div>
			{/each}
		</dl>
		<div class="flex items-center justify-between gap-2">
			<p class="flex min-w-0 items-center gap-2 text-xs font-medium" role="status">{#if active}<LoaderCircleIcon class="size-3.5 shrink-0 animate-spin" />{/if}{stage}</p>
			<Button variant="ghost" size="icon" class="size-7 shrink-0 rounded-sm" onclick={() => { dismissedError = ""; void checkForAppUpdate(); }} disabled={active || !status.supported} title="Refresh latest version status" aria-label="Refresh latest version status"><RefreshCwIcon class="size-3.5" /></Button>
		</div>
		{#if status.phase === "downloading"}
			<div class="space-y-2">
				{#if percentage !== null}<Progress value={percentage} aria-label="App update download" />{/if}
				<p class="text-xs tabular-nums text-muted-foreground">{(status.downloadedBytes / 1048576).toFixed(1)} MB{status.totalBytes ? ` / ${(status.totalBytes / 1048576).toFixed(1)} MB` : " downloaded"}</p>
			</div>
		{/if}
		{#if updateError && updateError !== dismissedError}
			<Notice tone="error" title="App update failed" message={updateError} onDismiss={() => dismissedError = updateError} />
		{:else if status.message && status.phase !== "error"}
			<p class="text-xs leading-5 text-muted-foreground">{status.message}</p>
		{/if}
		{#if confirming && !active}
			<div class="space-y-3 border-y border-border py-3" role="group" aria-label="Confirm app update">
				<p class="text-xs leading-5">Save any open edits first. Updating will stop your managed FXServer and restart this app. The server will not restart automatically.</p>
				<div class="flex flex-wrap gap-2"><Button size="sm" onclick={confirmUpdate}>Update and restart</Button><Button size="sm" variant="ghost" onclick={() => confirming = false}>Cancel</Button></div>
			</div>
		{/if}
		<div class="mt-auto flex flex-wrap gap-2">
			{#if available}<Button size="sm" class="rounded-sm" disabled={active || confirming} onclick={() => confirming = true}><DownloadIcon class="size-3.5" />{status.phase === "ready" ? "Install update" : "Update"}</Button>{/if}
			<Button variant="outline" size="sm" class="rounded-sm" onclick={() => openExternalUrl("https://github.com/zoxile/fxserver-installer/releases")} title="Open installers and release notes">Releases<ExternalLinkIcon class="size-3.5" /></Button>
			<Button variant="outline" size="sm" class="rounded-sm" onclick={() => openExternalUrl("https://github.com/zoxile/fxserver-installer")} title="Open GitHub repository">GitHub<ExternalLinkIcon class="size-3.5" /></Button>
		</div>
	</Card.Content>
</Card.Root>
