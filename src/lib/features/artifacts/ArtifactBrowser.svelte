<script lang="ts">
	import { onMount } from "svelte";
	import { Dialog } from "bits-ui";
	import ChevronLeftIcon from "@lucide/svelte/icons/chevron-left";
	import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
	import DownloadIcon from "@lucide/svelte/icons/download";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import ExternalLinkIcon from "@lucide/svelte/icons/external-link";
	import XIcon from "@lucide/svelte/icons/x";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Checkbox } from "$lib/components/ui/checkbox/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import * as Select from "$lib/components/ui/select/index.js";
	import { openExternalUrl } from "$lib/core/openExternal";
	import { fetchArtifactCatalog, type ArtifactBuild, type ArtifactCatalog } from "$lib/modules/artifact";

	let { destination, currentVersion, disabled = false, oninstall }: {
		destination: string; currentVersion?: string | null; disabled?: boolean;
		oninstall: (build: ArtifactBuild, destination: string) => Promise<void>;
	} = $props();
	let catalog = $state<ArtifactCatalog | null>(null);
	let loading = $state(false);
	let error = $state("");
	let search = $state("");
	let filter = $state("all");
	const filters = [{ value: "all", label: "All builds" }, { value: "known-issue", label: "Known issues" }, { value: "healthy", label: "JG healthy recommendation" }, { value: "unknown", label: "Unknown health" }, { value: "current", label: "Current build" }];
	let page = $state(0);
	let selected = $state<ArtifactBuild | null>(null);
	let selectedDestination = $state("");
	let acknowledged = $state(false);
	let open = $state(false);
	const filtered = $derived(catalog?.builds.filter((build) =>
		(build.version.includes(search.trim()) || build.issues.some((issue) => issue.reason.toLowerCase().includes(search.trim().toLowerCase()))) &&
		(filter === "all" || (filter === "current" ? build.version === currentVersion : (catalog?.stale && build.health !== "known-issue" ? "unknown" : build.health) === filter)),
	) ?? []);
	const pages = $derived(Math.max(1, Math.ceil(filtered.length / 25)));
	const currentPage = $derived(Math.min(page, pages - 1));
	const visible = $derived(filtered.slice(currentPage * 25, (currentPage + 1) * 25));
	onMount(() => { void refresh(false); });

	async function refresh(force: boolean) {
		if (loading) return;
		loading = true; error = "";
		try { catalog = await fetchArtifactCatalog(force); }
		catch (caught) { error = String(caught); }
		finally { loading = false; }
	}
	function select(build: ArtifactBuild) {
		selected = build; selectedDestination = destination.trim(); acknowledged = false; open = true;
	}
	async function confirm() {
		if (!selected || !acknowledged || disabled) return;
		const build = selected;
		open = false;
		await oninstall(build, selectedDestination);
	}
</script>

<section class="min-w-0 space-y-4 border-t border-border pt-6" aria-label="Official Windows artifact browser">
	<div class="flex flex-wrap items-center justify-between gap-3">
		<h2 class="text-lg font-semibold">Official Windows Builds</h2>
		<div class="flex gap-2">
			<Button variant="outline" size="sm" onclick={() => openExternalUrl("https://runtime.fivem.net/artifacts/fivem/build_server_windows/master/")}><ExternalLinkIcon />Official Listing</Button>
			<Button variant="outline" size="icon" title="Refresh official builds and JG issue metadata" aria-label="Refresh artifact catalog" disabled={loading || disabled} onclick={() => refresh(true)}><RefreshCwIcon class={loading ? "animate-spin" : undefined} /></Button>
		</div>
	</div>
	{#if error}<Notice tone="error" message={error} />{/if}
	{#if catalog?.warning}<Notice tone="warn" message={catalog.warning} />{/if}
	<div class="flex flex-wrap gap-3">
		<Input class="min-w-0 flex-1" value={search} oninput={(event) => { search = event.currentTarget.value; page = 0; }} aria-label="Search artifact builds" placeholder="Build number or issue" />
		<Select.Root type="single" bind:value={filter} items={filters} onValueChange={() => page = 0}>
			<Select.Trigger class="max-w-full" aria-label="Artifact health filter">{filters.find((option) => option.value === filter)?.label}</Select.Trigger>
			<Select.Content>{#each filters as option}<Select.Item value={option.value} label={option.label}>{option.label}</Select.Item>{/each}</Select.Content>
		</Select.Root>
	</div>
	<div class="overflow-auto border-y border-border">
		<table class="w-full table-fixed text-left text-sm">
			<thead class="bg-muted text-xs text-muted-foreground"><tr><th class="w-24 p-3">Build</th><th class="p-3">Status</th><th class="w-14 p-3"><span class="sr-only">Install</span></th></tr></thead>
			<tbody>
				{#each visible as build (build.version)}
					<tr class="border-t border-border align-top">
						<td class="p-3 font-mono">{build.version}</td>
						<td class="space-y-2 p-3">
							<div class="flex flex-wrap gap-2 text-xs">
								<span class={`rounded-sm border px-2 py-0.5 ${build.health === "known-issue" ? "border-red-400/40 bg-red-400/10 text-red-300" : build.health === "healthy" && !catalog?.stale ? "border-emerald-400/30 text-emerald-300" : "border-border text-muted-foreground"}`}>{build.health === "known-issue" ? "Known issue" : catalog?.stale ? "Health unknown (cached)" : build.health === "healthy" ? "Healthy (JG)" : "Health unknown"}</span>
								{#if build.recommended}<span class="text-emerald-300">{catalog?.stale ? "Cached recommendation" : "Recommended"}</span>{/if}
								{#if build.version === currentVersion}<span class="text-sky-300">Current</span>{/if}
							</div>
							{#each build.issues as issue}<p class="break-words text-xs text-red-300">{issue.reason}</p>{/each}
						</td>
						<td class="p-2"><Button variant="ghost" size="icon" title={`Install build ${build.version}`} aria-label={`Install build ${build.version}`} disabled={disabled || loading || !destination.trim()} onclick={() => select(build)}><DownloadIcon /></Button></td>
					</tr>
				{:else}<tr><td colspan="3" class="p-6 text-center text-muted-foreground">{loading ? "Loading official builds..." : "No matching builds."}</td></tr>{/each}
			</tbody>
		</table>
	</div>
	<div class="flex flex-wrap items-center justify-between gap-3 text-xs text-muted-foreground">
		<p>{filtered.length} builds{catalog ? ` / Fetched ${new Date(catalog.fetchedAt * 1000).toLocaleString()}` : ""}</p>
		{#if catalog?.metadataFetchedAt}<p>JG reports: {new Date(catalog.metadataFetchedAt * 1000).toLocaleString()}</p>{/if}
		<div class="flex items-center gap-3"><Button variant="outline" size="icon" title="Previous page" aria-label="Previous artifact page" disabled={currentPage === 0} onclick={() => (page = currentPage - 1)}><ChevronLeftIcon /></Button><span>{currentPage + 1} / {pages}</span><Button variant="outline" size="icon" title="Next page" aria-label="Next artifact page" disabled={currentPage >= pages - 1} onclick={() => (page = currentPage + 1)}><ChevronRightIcon /></Button></div>
	</div>
</section>

<Dialog.Root bind:open>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-[119] bg-black/65" />
		<Dialog.Content class="fixed top-1/2 left-1/2 z-[120] max-h-[85vh] w-[calc(100vw-2rem)] max-w-lg -translate-x-1/2 -translate-y-1/2 space-y-4 overflow-y-auto rounded-md border border-border bg-background p-5 shadow-xl">
			<div class="flex items-start justify-between gap-3"><Dialog.Title class="text-lg font-semibold">Install Windows Build {selected?.version}</Dialog.Title><Button variant="ghost" size="icon" title="Cancel install" aria-label="Cancel install" onclick={() => (open = false)}><XIcon /></Button></div>
			<Dialog.Description class="break-all text-sm text-muted-foreground">Artifact files will be replaced in {selectedDestination}. Stop FXServer and txAdmin first. Keep an independent backup.</Dialog.Description>
			{#if selected?.issues.length}<Notice tone="error" title="Known issues reported by JG Scripts" message={selected.issues.map((issue) => issue.reason).join("\n")} />
			{:else if selected?.health !== "healthy" || catalog?.stale}<Notice tone="warn" title="Health unknown" message="This build is not a verified current JG recommendation. An empty issue list does not establish that it is healthy." />{/if}
			<label class="flex items-start gap-3 text-sm"><Checkbox bind:checked={acknowledged} /><span>I have stopped the server, backed up its files, and accept the reported or unknown risks.</span></label>
			<div class="flex justify-end gap-2"><Button variant="outline" onclick={() => (open = false)}>Cancel</Button><Button variant={selected?.health === "known-issue" ? "destructive" : "default"} disabled={!acknowledged || disabled} onclick={confirm}><DownloadIcon />Confirm Install</Button></div>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
