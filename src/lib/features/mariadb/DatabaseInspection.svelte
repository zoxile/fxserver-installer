<script lang="ts">
	import { onMount, onDestroy } from "svelte";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import {
		inspectDatabase, inspectionTabs, type InspectionView, type InspectionResult,
	} from "$lib/modules/databaseInspection";
	import type { MariaDBCredentials } from "$lib/modules/mariadb";

	type Props = { credentials: MariaDBCredentials; database: string; view: InspectionView };
	let { credentials, database, view }: Props = $props();
	let result = $state.raw<InspectionResult | null>(null);
	let filter = $state("");
	let error = $state("");
	let busy = $state(false);
	let active = true;
	const rows = $derived(result?.rows.filter((row) =>
		row.some((cell) => cell?.toLowerCase().includes(filter.toLowerCase()))
	) ?? []);

	onMount(() => { void load(); });
	onDestroy(() => { active = false; });

	async function load() {
		if (busy || !active) return;
		busy = true;
		error = "";
		try {
			const next = await inspectDatabase({ ...credentials }, view, database);
			if (active) result = next;
		} catch (caught) {
			if (active) {
				error = String(caught);
				result = null;
			}
		} finally {
			if (active) busy = false;
		}
	}
</script>

<section class="min-w-0 space-y-4" aria-label="Database inspection" aria-busy={busy}>
	<header class="flex flex-wrap items-center justify-between gap-3">
		<h2 class="text-base font-semibold">{inspectionTabs.find((tab) => tab.value === view)?.label}</h2>
		<div class="flex min-w-0 items-center gap-2">
			<Input
				class="w-56 max-w-full" aria-label="Filter inspection rows"
				placeholder="Filter snapshot" maxlength={128} bind:value={filter}
			/>
			<Button
				variant="outline" size="icon-sm" title="Refresh inspection"
				aria-label="Refresh inspection" disabled={busy} onclick={load}
			>
				<RefreshCwIcon class={busy ? "animate-spin" : ""} />
			</Button>
		</div>
	</header>
	{#if error}
		<Notice tone="error" message={error} onDismiss={() => error = ""} />
	{/if}
	{#if result}
		<p class="text-xs text-muted-foreground">{result.notice} Cells are limited to 1,024 characters.</p>
		{#if result.hasMore}
			<p class="text-xs text-amber-500" role="status">
				Showing the first {result.limit} records. Additional records are not included in this snapshot.
			</p>
		{/if}
		<div class="max-h-[34rem] overflow-auto border-y border-border">
			<table class="w-full text-left text-xs">
				<thead class="sticky top-0 bg-background">
					<tr>
						{#each result.columns as column}
							<th class="whitespace-nowrap border-b border-border px-3 py-2 font-medium">{column}</th>
						{/each}
					</tr>
				</thead>
				<tbody>
					{#each rows as row}
						<tr class="border-b border-border/50">
							{#each row as cell}
								<td class="max-w-80 truncate px-3 py-2 font-mono" title={cell ?? "SQL NULL"}>
									{cell ?? "NULL"}
								</td>
							{/each}
						</tr>
					{:else}
						<tr>
							<td colspan={result.columns.length} class="p-6 text-center text-muted-foreground">
								No visible records.
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
		<p class="text-xs text-muted-foreground">{rows.length} records</p>
	{:else if busy}
		<p class="py-8 text-sm text-muted-foreground">Loading snapshot...</p>
	{/if}
</section>
