<script lang="ts">
	// Enhanced install flow adapted from Huntercorlett's fork (53f1834).
	import DownloadIcon from "@lucide/svelte/icons/download";
	import ExternalLinkIcon from "@lucide/svelte/icons/external-link";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
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
	let loading = $state(false);
	let active = true;
	const installing = $derived(taskSession.items.some((task) => ["install_windows_artifact", "install_enhanced_artifact"].includes(task.command) && task.status === "running"));
	const url = $derived(customUrl.trim() || selected);
	const options = $derived(builds.map((build) => ({ value: build.downloadUrl, label: build.version })));
	onMount(() => { void refresh(); return () => { active = false; }; });
	async function refresh() {
		if (loading) return;
		loading = true; error = ""; builds = []; selected = "";
		try {
			const catalog = await fetchEnhancedCatalog();
			if (!active) return;
			builds = catalog.builds;
			selected = builds[0]?.downloadUrl ?? "";
		} catch (caught) { if (active) error = String(caught); }
		finally { if (active) loading = false; }
	}
	async function install() {
		if (!url || !destination.trim() || installing || blocked || loading) return;
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
			<Button variant="outline" size="icon" onclick={refresh} disabled={loading || installing} title="Refresh Enhanced builds" aria-label="Refresh Enhanced builds"><RefreshCwIcon class={loading ? "animate-spin" : undefined} /></Button>
		</div>
	</div>
	{#if error}<Notice tone="error" message={error} />{/if}
	{#if blocked}<Notice tone="warn" message="This folder contains Legacy artifacts. Choose a separate folder for Enhanced." />{/if}
	<div class="grid gap-4 lg:grid-cols-2">
		<label class="grid min-w-0 gap-2">
			<span class="text-xs font-medium text-muted-foreground">Published Windows Build</span>
			<Select.Root type="single" bind:value={selected} items={options} disabled={installing || !builds.length}>
				<Select.Trigger class="w-full min-w-0 font-mono text-xs" aria-label="Enhanced build"><span class="truncate">{builds.find((build) => build.downloadUrl === selected)?.version ?? (loading ? "Loading..." : "No build discovered")}</span></Select.Trigger>
				<Select.Content>{#each options as option}<Select.Item value={option.value} label={option.label}>{option.label}</Select.Item>{/each}</Select.Content>
			</Select.Root>
		</label>
		<label class="grid min-w-0 gap-2"><span class="text-xs font-medium text-muted-foreground">Official Download URL Override</span><Input bind:value={customUrl} disabled={installing} placeholder="https://downloads.cfx-services.net/prod/.../cfx-server_win_x64.zip" class="min-w-0 font-mono text-xs" /></label>
	</div>
	<p class="break-all font-mono text-xs text-muted-foreground">{destination || "No install folder selected"}</p>
	<Button class="h-auto min-h-9 max-w-full whitespace-normal py-2" onclick={install} disabled={installing || loading || blocked || !url || !destination.trim()}><DownloadIcon />{installing ? "Installing..." : "Install Enhanced Server"}</Button>
</section>
