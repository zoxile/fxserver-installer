<script lang="ts">
	import { onMount } from "svelte";
	import SearchIcon from "@lucide/svelte/icons/search";
	import Trash2Icon from "@lucide/svelte/icons/trash-2";
	import ArrowUpRightIcon from "@lucide/svelte/icons/arrow-up-right";
	import HistoryIcon from "@lucide/svelte/icons/history";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import * as Select from "$lib/components/ui/select/index.js";
	import { clearIncidents, incidentSession, initializeIncidents } from "$lib/core/incidents.svelte";
	import { filterIncidents, incidentTypes } from "$lib/core/incidentModel";
	import { workspaceSession } from "$lib/core/workspaces.svelte";
	import { getPageLabel, type PageId } from "$lib/navigation";

	let { onNavigate }: { onNavigate?: (page: PageId) => void } = $props();
	let workspaceId = $state("all");
	let type = $state("all");
	let search = $state("");
	let after = $state("");
	let before = $state("");
	let limit = $state(100);
	const workspaceOptions = $derived([
		{ value: "all", label: "All workspaces" },
		...workspaceSession.items.map((workspace) => ({ value: workspace.id, label: workspace.name })),
		...[...new Set(incidentSession.items.map((item) => item.workspaceId))]
			.filter((id) => !workspaceSession.items.some((workspace) => workspace.id === id))
			.map((id) => ({ value: id, label: `Removed workspace (${id.slice(0, 8)})` })),
	]);
	const typeOptions = [{ value: "all", label: "All types" }, ...incidentTypes.map((item) => ({ value: item, label: item[0].toUpperCase() + item.slice(1) }))];
	const visible = $derived(filterIncidents(incidentSession.items, { workspaceId: workspaceId === "all" ? undefined : workspaceId, type: type === "all" ? undefined : type, search, after: after ? Date.parse(after) : undefined, before: before ? Date.parse(before) : undefined }));
	onMount(initializeIncidents);
</script>

<div class="space-y-5">
	<header class="flex flex-wrap items-center justify-between gap-3 border-b border-border pb-4">
		<h1 class="flex items-center gap-2 text-2xl font-semibold"><HistoryIcon class="size-6" />Incident Timeline</h1>
		<Button variant="outline" size="sm" disabled={!incidentSession.items.length} onclick={() => { if (window.confirm(`Clear ${workspaceId === "all" ? "all" : "this workspace's"} incident history?`)) clearIncidents(workspaceId === "all" ? undefined : workspaceId); }}><Trash2Icon class="size-4" />Clear history</Button>
	</header>
	{#if incidentSession.persistenceError}<Notice tone="warn" title="History storage" message={incidentSession.persistenceError} />{/if}
	<div class="grid gap-3 sm:grid-cols-2 xl:grid-cols-5">
		<div class="grid min-w-0 gap-1.5">
			<label for="incident-workspace" class="text-xs text-muted-foreground">Workspace</label>
			<Select.Root type="single" bind:value={workspaceId} items={workspaceOptions}>
				<Select.Trigger id="incident-workspace" class="w-full min-w-0" aria-label="Workspace"><span class="truncate">{workspaceOptions.find((option) => option.value === workspaceId)?.label ?? "All workspaces"}</span></Select.Trigger>
				<Select.Content>{#each workspaceOptions as option}<Select.Item value={option.value} label={option.label}>{option.label}</Select.Item>{/each}</Select.Content>
			</Select.Root>
		</div>
		<div class="grid min-w-0 gap-1.5">
			<label for="incident-type" class="text-xs text-muted-foreground">Type</label>
			<Select.Root type="single" bind:value={type} items={typeOptions}>
				<Select.Trigger id="incident-type" class="w-full min-w-0" aria-label="Type"><span class="truncate">{typeOptions.find((option) => option.value === type)?.label ?? "All types"}</span></Select.Trigger>
				<Select.Content>{#each typeOptions as option}<Select.Item value={option.value} label={option.label}>{option.label}</Select.Item>{/each}</Select.Content>
			</Select.Root>
		</div>
		<label class="grid gap-1.5 text-xs text-muted-foreground">From<Input type="datetime-local" bind:value={after} /></label>
		<label class="grid gap-1.5 text-xs text-muted-foreground">Until<Input type="datetime-local" bind:value={before} /></label>
		<label class="grid gap-1.5 text-xs text-muted-foreground">Search<div class="relative"><SearchIcon class="pointer-events-none absolute top-2.5 left-2.5 size-4" /><Input class="pl-8" bind:value={search} placeholder="Search events" /></div></label>
	</div>
	<div class="flex items-center justify-between border-b border-border pb-2 text-xs text-muted-foreground"><span>{visible.length} events</span><span>{incidentSession.items.length} / 1000 retained</span></div>
	<div class="divide-y divide-border">
		{#each visible.slice(0, limit) as incident (`${incident.workspaceId}:${incident.id}`)}
			<article class="grid gap-2 py-3 sm:grid-cols-[10rem_1fr_auto]">
				<div class="space-y-1 text-xs text-muted-foreground"><time datetime={new Date(incident.timestamp).toISOString()}>{new Date(incident.timestamp).toLocaleString()}</time><div class="break-all">{workspaceSession.items.find((item) => item.id === incident.workspaceId)?.name ?? incident.workspaceId}</div></div>
				<div class="min-w-0"><div class="mb-1 flex items-center gap-2 text-xs"><span class="capitalize text-muted-foreground">{incident.type}</span><span class={incident.level === "error" ? "text-destructive" : incident.level === "warn" ? "text-amber-400" : incident.level === "success" ? "text-emerald-400" : "text-muted-foreground"}>{incident.level}</span></div><h2 class="break-words text-sm font-medium">{incident.title}</h2>{#if incident.detail}<details class="mt-2 text-xs"><summary class="cursor-pointer text-muted-foreground">Details</summary><pre class="mt-2 whitespace-pre-wrap break-all font-mono text-muted-foreground">{incident.detail}</pre></details>{/if}</div>
				{#if onNavigate}<Button size="icon" variant="ghost" disabled={incident.workspaceId !== workspaceSession.activeId} title={incident.workspaceId === workspaceSession.activeId ? `Open ${getPageLabel(incident.panel)}` : "Switch to this workspace to open its panel"} aria-label={`Open ${getPageLabel(incident.panel)}`} onclick={() => onNavigate?.(incident.panel)}><ArrowUpRightIcon class="size-4" /></Button>{/if}
			</article>
		{:else}<p class="py-12 text-center text-sm text-muted-foreground">No matching events.</p>{/each}
	</div>
	{#if visible.length > limit}<Button variant="outline" onclick={() => limit += 100}>Show more</Button>{/if}
</div>
