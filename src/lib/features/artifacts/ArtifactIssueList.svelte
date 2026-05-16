<script lang="ts">
	import AlertTriangleIcon from "@lucide/svelte/icons/alert-triangle";
	import SearchIcon from "@lucide/svelte/icons/search";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import type { ArtifactIssue } from "$lib/modules/artifact";

	type Props = {
		issues: ArtifactIssue[];
	};

	let { issues }: Props = $props();
	let query = $state("");

	const filteredIssues = $derived(
		issues.filter((issue) => {
			const haystack = `${issue.artifact} ${issue.reason}`.toLowerCase();
			return !query.trim() || haystack.includes(query.trim().toLowerCase());
		}),
	);
</script>

<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-transform duration-300 hover:-translate-y-0.5">
	<div class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"></div>
	<Card.Header class="border-b border-border pb-4">
		<div class="flex flex-col gap-4 md:flex-row md:items-end md:justify-between">
			<div class="flex items-center gap-3">
				<div class="flex size-9 shrink-0 items-center justify-center rounded-sm border border-amber-400/30 bg-amber-400/10 text-amber-200">
					<AlertTriangleIcon class="size-4" />
				</div>
				<div>
					<Card.Title>Reported Artifact Issues</Card.Title>
					<Card.Description>Known broken versions and ranges from JG Scripts.</Card.Description>
				</div>
			</div>
			<div class="relative md:w-80">
				<SearchIcon class="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
				<Input bind:value={query} placeholder="Search version or issue..." title="Filter reported artifact issues." class="rounded-sm pl-9" />
			</div>
		</div>
	</Card.Header>
	<Card.Content>
		<div class="max-h-144 overflow-auto rounded-sm border border-border bg-background/60">
			{#if filteredIssues.length}
				<div class="divide-y divide-border/70">
					{#each filteredIssues.reverse() as issue}
						<article class="grid gap-2 px-4 py-3 text-sm md:grid-cols-4 md:items-start">
							<span class="w-fit rounded-xs border border-red-400/30 bg-red-400/10 px-2 py-0.5 font-mono text-xs font-semibold text-red-200 md:col-span-1">{issue.artifact}</span>
							<p class="min-w-0 text-muted-foreground md:col-span-3">{issue.reason}</p>
						</article>
					{/each}
				</div>
			{:else}
				<div class="flex min-h-40 items-center justify-center px-4 text-center text-sm text-muted-foreground">No reported artifact issues match that filter.</div>
			{/if}
		</div>
	</Card.Content>
</Card.Root>
