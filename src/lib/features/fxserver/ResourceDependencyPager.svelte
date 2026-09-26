<script lang="ts">
	import ChevronLeftIcon from "@lucide/svelte/icons/chevron-left";
	import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
	import { Button } from "$lib/components/ui/button/index.js";
	import { DEPENDENCY_PAGE_SIZE } from "$lib/modules/resourceDependencies";
	let { total, page = $bindable(0), label }: { total: number; page?: number; label: string } = $props();
	const pages = $derived(Math.max(1, Math.ceil(total / DEPENDENCY_PAGE_SIZE)));
	const current = $derived(Math.min(page, pages - 1));
</script>

<div class="flex min-w-0 flex-wrap items-center justify-between gap-2 text-xs text-muted-foreground">
	<span aria-live="polite">Showing {total ? current * DEPENDENCY_PAGE_SIZE + 1 : 0}-{Math.min(total, (current + 1) * DEPENDENCY_PAGE_SIZE)} of {total}</span>
	{#if total > DEPENDENCY_PAGE_SIZE}
		<div class="flex shrink-0 items-center gap-1">
			<Button size="icon-sm" variant="ghost" title={`Previous ${label} page`} aria-label={`Previous ${label} page`} disabled={current === 0} onclick={() => (page = current - 1)}><ChevronLeftIcon /></Button>
			<Button size="icon-sm" variant="ghost" title={`Next ${label} page`} aria-label={`Next ${label} page`} disabled={current >= pages - 1} onclick={() => (page = current + 1)}><ChevronRightIcon /></Button>
		</div>
	{/if}
</div>
