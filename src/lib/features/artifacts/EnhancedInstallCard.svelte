<script lang="ts">
	// Enhanced install flow adapted from Huntercorlett's fork (53f1834).
	import DownloadIcon from "@lucide/svelte/icons/download";
	import ExternalLinkIcon from "@lucide/svelte/icons/external-link";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
	import { onMount } from "svelte";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import * as Select from "$lib/components/ui/select/index.js";
	import { openExternalUrl } from "$lib/core/openExternal";
	import { taskSession } from "$lib/core/tasks.svelte";
	import { enhancedDownloadUrl, fetchEnhancedCatalog, installEnhancedArtifact, type EnhancedArtifactBuild, type ArtifactInstallResult } from "$lib/modules/artifact";
	let { destination, blocked, oninstalled }: { destination: string; blocked: boolean; oninstalled: (result: ArtifactInstallResult) => void } = $props();
	let builds = $state<EnhancedArtifactBuild[]>([]);
	let selected = $state("");
	let customUrl = $state("");
	let error = $state("");
	let warning = $state("");
	let loading = $state(false);
	let retrying = $state(false);
	let active = true;
	const installing = $derived(taskSession.items.some((task) => ["install_windows_artifact", "install_enhanced_artifact"].includes(task.command) && task.status === "running"));
	const selectedBuild = $derived(builds.find((build) => build.downloadUrl === selected) ?? builds[0]);
	const url = $derived(customUrl.trim() || selectedBuild?.downloadUrl || "");
	const options = $derived(builds.map((build) => ({ value: build.downloadUrl, label: build.version })));
	const canInstall = $derived(!installing && !blocked && Boolean(url && destination.trim()) && (!loading || Boolean(customUrl.trim())));
	onMount(() => { void refresh(true); return () => { active = false; }; });
	async function refresh(retryInitialFailure = false) {
		if (loading) return;
		loading = true; retrying = false; error = ""; warning = "";
		try {
			const catalog = await fetchEnhancedCatalog().catch(async (caught) => {
				if (!retryInitialFailure || !active) throw caught;
				retrying = true;
				await new Promise((resolve) => setTimeout(resolve, 750));
				return active ? fetchEnhancedCatalog() : null;
			});
			if (!active || !catalog) return;
			if (!catalog.builds.length) throw new Error("No Enhanced Windows build was found. Open Official Downloads to check availability.");
			builds = catalog.builds;
			warning = catalog.warning ?? "";
		} catch (caught) { if (active) { builds = []; error = String(caught); } }
		finally { if (active) { loading = false; retrying = false; } }
	}
	async function install() {
		if (!canInstall) return;
		error = "";
		try {
			const result = await installEnhancedArtifact(url, destination.trim());
			if (active) oninstalled(result);
		} catch (caught) { if (active) error = String(caught); }
	}
</script>

<section class="min-w-0 space-y-4 border-t border-border pt-6" aria-label="Enhanced Windows artifacts">
	<div class="flex flex-wrap items-center justify-between gap-3">
		<h2 class="text-lg font-semibold">FiveM for GTAV Enhanced</h2>
		<div class="flex gap-2">
			<Button variant="outline" size="sm" title="Official Enhanced downloads" aria-label="Official Enhanced downloads" onclick={() => openExternalUrl(enhancedDownloadUrl)}><ExternalLinkIcon /><span class="hidden sm:inline">Official Downloads</span></Button>
			<Button variant="outline" size="icon" onclick={() => refresh()} disabled={loading || installing} title="Refresh Enhanced builds" aria-label="Refresh Enhanced builds">{#if loading}<LoaderCircleIcon class="animate-spin" />{:else}<RefreshCwIcon />{/if}</Button>
		</div>
	</div>
	<Notice tone="warn" title="Enhanced is in early access" message="FiveM for GTAV Enhanced may be unstable and resources may not be compatible. Use a separate test server and keep independent backups before installing or updating." />
	{#if loading}<p role="status" class="text-sm text-muted-foreground">{retrying ? "Retrying Enhanced build lookup..." : "Checking official Enhanced builds..."}</p>{/if}
	{#if error}<Notice tone="error" message={error} />{/if}
	{#if warning}<Notice tone="warn" message={warning} />{/if}
	{#if blocked}<Notice tone="warn" message="This folder contains Legacy artifacts. Choose a separate folder for Enhanced." />{/if}
	<div class="grid gap-4 lg:grid-cols-2">
		<label class="grid min-w-0 gap-2">
			<span class="text-xs font-medium text-muted-foreground">Published Windows Build</span>
			<Select.Root type="single" value={selectedBuild?.downloadUrl ?? ""} onValueChange={(value) => { selected = value; }} items={options} disabled={installing || loading || !builds.length}>
				<Select.Trigger class="w-full min-w-0 font-mono text-xs" aria-label="Enhanced build"><span class="truncate">{selectedBuild?.version ?? (loading ? "Loading..." : "No build discovered")}</span></Select.Trigger>
				<Select.Content>{#each options as option}<Select.Item value={option.value} label={option.label}>{option.label}</Select.Item>{/each}</Select.Content>
			</Select.Root>
		</label>
		<label class="grid min-w-0 gap-2"><span class="text-xs font-medium text-muted-foreground">Official Download URL Override</span><Input bind:value={customUrl} disabled={installing} placeholder="https://downloads.cfx-services.net/prod/.../cfx-server_win_x64.zip" class="min-w-0 font-mono text-xs" /></label>
	</div>
	<p class="break-all font-mono text-xs text-muted-foreground">{destination || "No install folder selected"}</p>
	<Button class="h-auto min-h-9 max-w-full whitespace-normal py-2" onclick={install} disabled={!canInstall}>{#if installing}<LoaderCircleIcon class="animate-spin" />{:else}<DownloadIcon />{/if}{installing ? "Installing..." : "Install Enhanced Server"}</Button>
</section>
