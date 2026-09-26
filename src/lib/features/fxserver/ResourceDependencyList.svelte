<script lang="ts" generics="T">
	import type { Snippet } from "svelte";
	import { dependencyPage } from "$lib/modules/resourceDependencies";
	import ResourceDependencyPager from "./ResourceDependencyPager.svelte";
	let { items, label, children, ordered = false, empty = "None declared in the scanned inventory." }: {
		items: readonly T[]; label: string; children: Snippet<[T]>; ordered?: boolean; empty?: string;
	} = $props();
	let page = $state(0);
	const visible = $derived(dependencyPage(items, page));
	$effect(() => { items; page = 0; });
</script>

<div class="min-w-0 space-y-2" data-dependency-list={label}>
	<ResourceDependencyPager total={items.length} bind:page {label} />
	{#if items.length}
		<ol aria-label={label} start={visible.start + 1} class={`max-h-48 space-y-2 overflow-auto break-all text-xs ${ordered ? "list-inside list-decimal" : "list-none"}`}>
			{#each visible.items as item}<li>{@render children(item)}</li>{/each}
		</ol>
	{:else}<p class="text-xs text-muted-foreground">{empty}</p>{/if}
</div>
