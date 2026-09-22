<script lang="ts">
	import { onDestroy } from "svelte";
	import SearchCheckIcon from "@lucide/svelte/icons/search-check";
	import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import type { MariaDBCredentials } from "$lib/modules/mariadb";
	import { diagnoseSqlError, inspectDatabaseSql, type SqlInspection } from "$lib/modules/sqlDiagnostics";

	let { credentials, database, error = "", disabled = false }: { credentials: MariaDBCredentials; database: string; error?: string; disabled?: boolean } = $props();
	let table = $state("");
	let report = $state<SqlInspection | null>(null);
	let failure = $state("");
	let loading = $state(false);
	let reportScope = $state("");
	let active = true;
	let generation = 0;
	const diagnosis = $derived(diagnoseSqlError(error));
	const scope = $derived(JSON.stringify([credentials, database, table]));
	const visibleReport = $derived(reportScope === scope ? report : null);
	const collations = $derived([...new Set(visibleReport?.columns.map((column) => column.collation).filter(Boolean) ?? [])]);
	onDestroy(() => { active = false; generation++; });

	async function inspect() {
		if (loading || disabled || !database || database === "__global__") return;
		const original = scope;
		const request = ++generation;
		loading = true;
		failure = "";
		try {
			const next = await inspectDatabaseSql(credentials, database, table);
			if (!active || request !== generation || original !== scope) return;
			report = next;
			reportScope = original;
		} catch (caught) {
			if (active && original === scope) failure = String(caught);
		} finally {
			if (active && request === generation) loading = false;
		}
	}
</script>

<details class="min-w-0 rounded-sm border border-border bg-background p-4" open={Boolean(error)}>
	<summary class="cursor-pointer text-sm font-medium">SQL diagnostics</summary>
	<div class="mt-4 space-y-4">
		{#if error}
			<div class="space-y-2 text-sm">
				<p class="font-medium text-amber-300">{diagnosis.title}</p>
				<ul class="list-disc space-y-1 pl-5 text-muted-foreground">
					{#each diagnosis.checks as check}<li>{check}</li>{/each}
				</ul>
			</div>
		{/if}
		<div class="flex flex-wrap items-end gap-2">
			<label class="grid min-w-0 flex-1 gap-2 text-xs text-muted-foreground">
				Table (optional)
				<Input bind:value={table} placeholder="All tables in selected database" disabled={loading} />
			</label>
			<Button variant="outline" onclick={inspect} disabled={disabled || loading || !database || database === "__global__"}>
				{#if loading}<LoaderCircleIcon class="animate-spin" />{:else}<SearchCheckIcon />{/if}Inspect schema
			</Button>
		</div>
		<p class="text-xs text-muted-foreground">Read-only inspection of the selected database. No scripts are retried or schemas changed. Results include only metadata visible to this account.</p>
		{#if failure}<Notice tone="error" message={failure} onDismiss={() => (failure = "")} />{/if}
		{#if visibleReport}
			<dl class="grid gap-x-4 gap-y-2 text-xs sm:grid-cols-[auto_minmax(0,1fr)]">
				<dt class="text-muted-foreground">Server</dt><dd class="break-all">{visibleReport.environment.version}</dd>
				<dt class="text-muted-foreground">Global SQL mode</dt><dd class="break-all">{visibleReport.environment.sqlMode || "None"}</dd>
				<dt class="text-muted-foreground">Schema default</dt><dd>{visibleReport.environment.schemaCollation}</dd>
				<dt class="text-muted-foreground">Inspection connection</dt><dd>{visibleReport.environment.collation}</dd>
				<dt class="text-muted-foreground">Column collations</dt><dd class="break-all">{collations.join(", ") || "No text columns found"}</dd>
			</dl>
			{#if visibleReport.truncated}<p class="text-xs text-amber-300">Limited to 500 columns or foreign-key pairs. Select a table to narrow the inspection.</p>{/if}
			<div class="max-h-72 overflow-auto">
				<table class="w-full text-left text-xs">
					<thead class="sticky top-0 bg-muted">
						<tr>
							{#each ["Table / column", "Type", "Collation", "Engine"] as label}
								<th class="p-2 font-medium">{label}</th>
							{/each}
						</tr>
					</thead>
					<tbody>
						{#each visibleReport.columns as column}
							<tr class="border-b border-border">
								<td class="p-2 font-mono">{column.table}.{column.column}</td>
								<td class="max-w-64 truncate p-2" title={column.columnType}>{column.columnType}</td>
								<td class="p-2">{column.collation ?? "-"}</td>
								<td class="p-2">{column.engine ?? "-"}</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
			{#if visibleReport.foreignKeys.length}
				<div class="space-y-2 text-xs">
					<p class="font-medium">Existing foreign keys</p>
					{#each visibleReport.foreignKeys as key}
						<p class="break-all font-mono">{key.column} &rarr; {key.parentDatabase}.{key.parentTable}.{key.parentColumn} ({key.parentType ?? "not visible"}, {key.parentCollation ?? "non-text"})</p>
					{/each}
				</div>
			{/if}
		{/if}
	</div>
</details>
