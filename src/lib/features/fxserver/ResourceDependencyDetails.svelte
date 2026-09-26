<script lang="ts">
	import { dependencyImpact, dependencyNames, dependencyStartupOrder, type ResourceGraph } from "$lib/modules/resourceDependencies";
	import ResourceDependencyList from "./ResourceDependencyList.svelte";
	let { graph, path }: { graph: ResourceGraph; path: string } = $props();
	let startupOpen = $state(false);
	const impact = $derived(dependencyImpact(graph, path));
	const order = $derived(dependencyStartupOrder(graph, [impact.id, ...impact.direct, ...impact.transitive]));
	const declarations = $derived(graph.declarations.get(impact.id) ?? []);
	const related = $derived(new Set([impact.id, ...impact.dependencies]));
	const cycles = $derived(graph.cycles.filter((cycle) => cycle.some((id) => related.has(id))));
</script>

{#snippet names(title: string, ids: string[])}
	<section class="min-w-0 space-y-2">
		<h3 class="text-sm font-medium">{title} <span class="text-muted-foreground">({ids.length})</span></h3>
		<ResourceDependencyList items={ids} label={title}>
			{#snippet children(id)}<span class="font-mono">{graph.nodes.get(id)?.name}</span><span class="text-muted-foreground"> / {graph.nodes.get(id)?.origin} / {graph.nodes.get(id)?.path}</span>{/snippet}
		</ResourceDependencyList>
	</section>
{/snippet}

{#if !impact.resource}
	<p role="alert" class="text-sm text-amber-400">This resource was not found in the fresh scan. Its dependency impact is unknown.</p>
{:else}
	<div class="space-y-4">
		<div class="space-y-1 text-xs text-muted-foreground">
			<p class="break-all font-mono">{impact.resource.path}</p>
			<p>{impact.resource.origin === "artifact" ? "Artifact stock resource (read-only inventory)" : "Server data resource"}</p>
			{#if impact.resource.provides.length}<p class="break-all">Provides: {impact.resource.provides.slice(0, 8).join(", ")}{impact.resource.provides.length > 8 ? ` (8 of ${impact.resource.provides.length} shown)` : ""}</p>{/if}
			{#if impact.resource.dynamic}<p class="text-amber-400">Dynamic manifest metadata: some declarations could not be resolved without executing Lua.</p>{/if}
		</div>
		<section class="space-y-2 border-t border-border pt-3">
			<h3 class="text-sm font-medium">Declared Direct Dependencies ({declarations.length})</h3>
			<ResourceDependencyList items={declarations} label="Declared Direct Dependencies" empty="None declared. Runtime relationships remain unknown.">
				{#snippet children(entry)}
					<span class="font-mono">{entry.name}</span><span class={entry.kind === "resolved" ? "text-muted-foreground" : "text-amber-400"}> / {entry.kind === "constraint" ? "runtime constraint, not verified" : entry.kind === "ambiguous" ? "ambiguous provider, runtime selection unknown" : entry.kind === "unresolved" ? "not verified, incomplete inventory" : entry.kind}</span>
					{#if entry.targets.length}<p class="mt-1 text-muted-foreground">{dependencyNames(graph, entry.targets)}</p>{/if}
				{/snippet}
			</ResourceDependencyList>
		</section>
		<div class="grid min-w-0 gap-4 border-t border-border pt-3 sm:grid-cols-2">
			{@render names("Transitive Dependencies (Indirect)", impact.indirectDependencies)}
			{@render names("Direct Dependents", impact.direct)}
			{@render names("Transitive Dependents (Indirect)", impact.transitive)}
			{@render names("Possible Dependents (Provider Ambiguity)", impact.possible)}
		</div>
		{#if cycles.length}
			<section class="space-y-2 border-t border-border pt-3 text-amber-400"><h3 class="text-sm font-medium">Dependency Cycles</h3><p class="text-xs">Each group is mutually dependent; no dependency-first order exists within it.</p>
				<ResourceDependencyList items={cycles} label="Dependency Cycles">
					{#snippet children(cycle)}<span class="font-mono">{dependencyNames(graph, cycle)}</span>{/snippet}
				</ResourceDependencyList>
			</section>
		{/if}
		<details bind:open={startupOpen} class="border-t border-border pt-3">
			<summary class="cursor-pointer text-sm font-medium">Suggested Startup Order / Selected Resource and Dependents</summary>
			{#if startupOpen}
			<p class="my-2 text-xs text-muted-foreground">Declared dependencies first. This is not the live server order. Runtime constraints and undeclared relationships are unverified. No commands are run automatically.</p>
			<ResourceDependencyList items={order.order} label="Selected Startup Order" ordered>
				{#snippet children(id)}<span class="font-mono">{graph.nodes.get(id)?.name}</span>{/snippet}
			</ResourceDependencyList>
			{#if order.blocked.length}<div class="mt-3 text-amber-400">{@render names("Unordered (Missing, Dynamic, Ambiguous or Cyclic)", order.blocked)}</div>{/if}
			{/if}
		</details>
	</div>
{/if}
