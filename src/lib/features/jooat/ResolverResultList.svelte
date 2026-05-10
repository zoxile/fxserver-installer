<script lang="ts">
	import CircleAlertIcon from "@lucide/svelte/icons/circle-alert";
	import CircleCheckIcon from "@lucide/svelte/icons/circle-check";
	import SearchIcon from "@lucide/svelte/icons/search";
	import type { ResolveResult } from "./joaat";

	type Props = {
		results: ResolveResult[];
	};

	let { results }: Props = $props();
</script>

<div class="space-y-2">
	{#each results as result}
		<div class="rounded-sm border border-border bg-background/70 p-3">
			<div class="flex items-start justify-between gap-3">
				<div class="min-w-0">
					<p class="truncate font-mono text-xs text-foreground">{result.query}</p>
					{#if result.hash}
						<p class="mt-1 text-xs text-muted-foreground">{result.hash.hex} / {result.hash.unsigned}</p>
					{/if}
				</div>
				{#if result.error}
					<span class="inline-flex shrink-0 items-center gap-1 rounded-sm border border-destructive/30 bg-destructive/10 px-2 py-1 text-xs text-destructive">
						<CircleAlertIcon class="size-3" />
						Invalid
					</span>
				{:else if result.matches.length}
					<span class="inline-flex shrink-0 items-center gap-1 rounded-sm border border-emerald-400/30 bg-emerald-400/10 px-2 py-1 text-xs text-emerald-300">
						<CircleCheckIcon class="size-3" />
						Resolved
					</span>
				{:else}
					<span class="inline-flex shrink-0 items-center gap-1 rounded-sm border border-border bg-muted px-2 py-1 text-xs text-muted-foreground">
						<SearchIcon class="size-3" />
						No match
					</span>
				{/if}
			</div>

			{#if result.error}
				<p class="mt-3 text-xs text-destructive">{result.error}</p>
			{:else if result.matches.length}
				<div class="mt-3 flex flex-wrap gap-2">
					{#each result.matches as match}
						<span class="rounded-sm border border-border bg-muted px-2 py-1 font-mono text-xs text-foreground">{match}</span>
					{/each}
				</div>
			{:else}
				<p class="mt-3 text-xs text-muted-foreground">No candidate in the dictionary hashes to this value.</p>
			{/if}
		</div>
	{:else}
		<div class="rounded-sm border border-dashed border-border bg-background/60 p-6 text-center text-sm text-muted-foreground">
			Enter one or more hashes to resolve against the candidate dictionary.
		</div>
	{/each}
</div>
