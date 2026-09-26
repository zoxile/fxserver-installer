<script lang="ts">
	import { Dialog } from "bits-ui";
	import { onDestroy, onMount } from "svelte";
	import XIcon from "@lucide/svelte/icons/x";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import ShieldCheckIcon from "@lucide/svelte/icons/shield-check";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Checkbox } from "$lib/components/ui/checkbox/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import * as Select from "$lib/components/ui/select/index.js";
	import { buildResourceGraph, dependencyNames, dependencyPage, dependencyStartupOrder, resourcePathKey, type DependencyRequest, type ResourceGraph } from "$lib/modules/resourceDependencies";
	import { scanResourceDependencies } from "$lib/modules/resourceDependencyApi";
	import ResourceDependencyDetails from "./ResourceDependencyDetails.svelte";
	import ResourceDependencyList from "./ResourceDependencyList.svelte";
	import ResourceDependencyPager from "./ResourceDependencyPager.svelte";

	let { request, path = "", action, onclose, onconfirm }: {
		request: DependencyRequest; path?: string; action?: string;
		onclose: () => void; onconfirm?: () => void | Promise<void>;
	} = $props();
	let open = $state(true);
	let loading = $state(false);
	let busy = $state(false);
	let error = $state("");
	let graph = $state<ResourceGraph | null>(null);
	let selected = $state("");
	let resourceSearch = $state("");
	let resourcePage = $state(0);
	let issuesOpen = $state(false);
	let startupOpen = $state(false);
	let configuredOpen = $state(false);
	let warningsOpen = $state(false);
	let acknowledged = $state(false);
	let active = true;
	const sorted = $derived(graph ? [...graph.nodes].sort((a, b) => a[1].name.localeCompare(b[1].name) || a[0].localeCompare(b[0])) : []);
	const matchingResources = $derived(sorted.filter(([, resource]) => `${resource.name} ${resource.origin}`.toLowerCase().includes(resourceSearch.trim().toLowerCase())));
	const options = $derived(dependencyPage(matchingResources, resourcePage).items.map(([value, resource]) => ({ value, label: `${resource.name} / ${resource.origin}` })));
	const selectedResource = $derived(graph?.nodes.get(selected));
	const issues = $derived(graph ? [...graph.declarations].flatMap(([id, entries]) => entries.filter((entry) => ["missing", "unresolved", "ambiguous"].includes(entry.kind)).map((entry) => ({ id, ...entry }))) : []);
	const startup = $derived(graph ? dependencyStartupOrder(graph) : null);
	$effect(() => { resourceSearch; resourcePage = 0; });
	onMount(() => { void refresh(); });
	onDestroy(() => { active = false; });

	async function refresh() {
		if (loading || busy) return;
		loading = true; acknowledged = false; error = ""; graph = null;
		resourcePage = 0; resourceSearch = "";
		issuesOpen = false; startupOpen = false; configuredOpen = false; warningsOpen = false;
		try {
			const inventory = await scanResourceDependencies({ ...request });
			if (!active) return;
			graph = buildResourceGraph(inventory);
			selected = path ? resourcePathKey(path) : [...graph.nodes.keys()].sort()[0] ?? "";
		} catch (caught) { if (active) error = String(caught); }
		finally { if (active) loading = false; }
	}

	async function confirm() {
		if (!graph || loading || busy || !acknowledged || !onconfirm) return;
		busy = true; error = "";
		try { await onconfirm(); }
		catch (caught) { if (active) { error = String(caught); acknowledged = false; } }
		finally { if (active) busy = false; }
	}
</script>

<Dialog.Root bind:open onOpenChange={(value) => { if (!value) onclose(); }}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-[129] bg-black/65" />
		<Dialog.Content class="fixed top-1/2 left-1/2 z-[130] flex max-h-[90dvh] w-[calc(100vw-1.5rem)] max-w-4xl -translate-x-1/2 -translate-y-1/2 flex-col gap-4 overflow-hidden rounded-md border border-border bg-background p-4 shadow-xl sm:p-5">
			<div class="flex items-start justify-between gap-3">
				<div class="min-w-0"><Dialog.Title class="break-words text-lg font-semibold">{action ? `${action}: Dependency Review` : "Resource Dependency Inspector"}</Dialog.Title>
					<Dialog.Description class="mt-1 text-xs text-muted-foreground">Declared manifest relationships only. Runtime exports, events, dynamic Lua and external services are unknown.</Dialog.Description>
				</div>
				<Button variant="ghost" size="icon" aria-label="Close dependency inspector" title="Close dependency inspector" disabled={busy} onclick={onclose}><XIcon /></Button>
			</div>
			<div class="min-h-0 space-y-4 overflow-y-auto overscroll-contain">
				{#if loading}<p role="status" class="py-6 text-sm text-muted-foreground">Inspecting manifests and startup configuration...</p>{/if}
				{#if error}<p role="alert" class="break-words text-sm text-destructive">Dependency review failed: {error}</p>{/if}
				{#if graph}
					<p class="text-xs text-muted-foreground">{graph.nodes.size} resources / {graph.inventory.resources.filter((resource) => resource.origin === "artifact").length} artifact stock / {issues.length} unresolved declarations / {graph.cycles.length} cycles</p>
					{#if graph.inventory.resourcesIncomplete || graph.truncated}<p role="alert" class="text-sm text-amber-400">Incomplete inventory: skipped or unavailable resources may hide dependencies and cycles. An empty result is not proof of safety.</p>{/if}
					{#if action}<p class="break-all text-sm font-semibold">{graph.nodes.get(selected)?.name ?? path}</p><p class="text-xs text-amber-400">Only the selected resource will be changed. Dependents are not automatically stopped or restarted. RCON may target a different server from this local inventory.</p>
					{:else}
						<div class="grid min-w-0 gap-2">
							<Input bind:value={resourceSearch} aria-label="Search dependency resources" placeholder="Search resources" />
							<Select.Root type="single" bind:value={selected} items={options} disabled={!options.length}>
								<Select.Trigger aria-label="Inspect resource" class="w-full min-w-0"><span class="truncate">{selectedResource ? `${selectedResource.name} / ${selectedResource.origin}` : "Choose resource"}</span></Select.Trigger>
								<Select.Content class="z-[140] max-h-72 w-(--bits-select-anchor-width) max-w-[calc(100vw-2rem)]">
									{#each options as option (option.value)}<Select.Item value={option.value} label={option.label}><span class="min-w-0 break-all">{option.label}</span></Select.Item>{/each}
								</Select.Content>
							</Select.Root>
							<ResourceDependencyPager total={matchingResources.length} bind:page={resourcePage} label="resources" />
						</div>
					{/if}
					{#key selected}<ResourceDependencyDetails {graph} path={selected} />{/key}
					{#if !action}
						<details bind:open={issuesOpen} class="border-t border-border pt-3"><summary class="cursor-pointer text-sm font-medium">All Missing Dependencies, Ambiguities and Cycles</summary>
							{#if issuesOpen}<div class="mt-2 space-y-3">
								<ResourceDependencyList items={issues} label="Dependency Issues" empty="No unresolved declarations found.">
									{#snippet children(issue)}{graph!.nodes.get(issue.id)?.name}: {issue.name} / {issue.kind}{/snippet}
								</ResourceDependencyList>
								<ResourceDependencyList items={graph.cycles} label="All Dependency Cycles" empty="No cycles found in the scanned declarations.">
									{#snippet children(cycle)}Cycle group: {dependencyNames(graph!, cycle)}{/snippet}
								</ResourceDependencyList>
							</div>{/if}
						</details>
						<details bind:open={startupOpen} class="border-t border-border pt-3"><summary class="cursor-pointer text-sm font-medium">All Resources / Suggested Startup Order</summary>
							{#if startupOpen}<div class="mt-2">
								<ResourceDependencyList items={startup?.order ?? []} label="All Startup Order" ordered>
									{#snippet children(id)}<span class="font-mono">{graph!.nodes.get(id)?.name}</span>{/snippet}
								</ResourceDependencyList>
								<p class="mt-2 text-xs text-amber-400">{startup?.blocked.length ?? 0} unordered. Suggestions only; runtime constraints are unverified.</p>
							</div>{/if}
						</details>
					{/if}
					<details bind:open={configuredOpen} class="border-t border-border pt-3"><summary class="cursor-pointer text-sm font-medium">Configured Startup Sequence ({graph.inventory.startup.length})</summary>
						{#if configuredOpen}<p class="my-2 text-xs text-muted-foreground">Literal commands in config execution order, including exec files. Group member order and implicit dependency starts are not known. This is not live execution history.</p>
						{#if graph.inventory.configsIncomplete}<p class="text-xs text-amber-400">Configuration scan incomplete.</p>{/if}
						<ResourceDependencyList items={graph.inventory.startup} label="Configured Startup Sequence" ordered>
							{#snippet children(entry)}<span class="font-mono">{entry.command} {entry.target}</span>{/snippet}
						</ResourceDependencyList>{/if}
					</details>
					{#if graph.inventory.warnings.length}<details bind:open={warningsOpen} class="border-t border-border pt-3"><summary class="cursor-pointer text-sm font-medium">Scan Warnings ({graph.inventory.warnings.length})</summary>
						{#if warningsOpen}<div class="mt-2 text-amber-400"><ResourceDependencyList items={graph.inventory.warnings} label="Scan Warnings">
							{#snippet children(warning)}{warning.detail}{/snippet}
						</ResourceDependencyList></div>{/if}
					</details>{/if}
				{/if}
			</div>
			<div class="shrink-0 space-y-3 border-t border-border pt-3">
				{#if action && graph}<label class="flex items-start gap-2 text-xs"><Checkbox bind:checked={acknowledged} disabled={busy} aria-label="Acknowledge dependency impact" /><span>I reviewed the affected resources and accept the unknown runtime impact.</span></label>{/if}
				<div class="flex flex-wrap justify-end gap-2">
					<Button size="sm" variant="outline" onclick={refresh} disabled={loading || busy} title="Rescan dependencies"><RefreshCwIcon class={loading ? "animate-spin" : undefined} />Rescan</Button>
					<Button size="sm" variant="outline" onclick={onclose} disabled={busy}>{action ? "Cancel" : "Close"}</Button>
					{#if action}<Button size="sm" onclick={confirm} disabled={!graph || !acknowledged || loading || busy}><ShieldCheckIcon />{busy ? "Working..." : `Confirm ${action}`}</Button>{/if}
				</div>
			</div>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
