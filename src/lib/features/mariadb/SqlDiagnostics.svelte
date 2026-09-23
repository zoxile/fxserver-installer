<script lang="ts">
	import { onDestroy } from "svelte";
	import SearchCheckIcon from "@lucide/svelte/icons/search-check";
	import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import type { MariaDBCredentials } from "$lib/modules/mariadb";
	import { diagnoseSqlError, inspectDatabaseSql, sqlInspectionAvailability, summarizeSqlInspection, type SqlInspection } from "$lib/modules/sqlDiagnostics";

	let { credentials, database, error = "", disabled = false, credentialsReady }: {
		credentials: MariaDBCredentials;
		database: string;
		error?: string;
		disabled?: boolean;
		credentialsReady?: boolean;
	} = $props();
	const id = $props.id();
	let table = $state("");
	let report = $state<SqlInspection | null>(null);
	let failure = $state("");
	let loading = $state(false);
	let reportScope = $state("");
	let failureScope = $state("");
	let active = true;
	let generation = 0;
	const ready = $derived(credentialsReady ?? !disabled);
	const diagnosis = $derived(diagnoseSqlError(error));
	const scope = $derived(JSON.stringify([credentials, database, table.trim(), ready]));
	const availability = $derived(sqlInspectionAvailability(database, ready, disabled));
	const visibleReport = $derived(reportScope === scope ? report : null);
	const visibleFailure = $derived(failureScope === scope ? failure : "");
	const summary = $derived(visibleReport ? summarizeSqlInspection(visibleReport, table) : null);

	// Invalidate in-flight reads too, including a switch away from and back to the same scope.
	$effect(() => {
		scope;
		generation++;
		report = null;
		failure = "";
		loading = false;
	});
	onDestroy(() => { active = false; generation++; });

	async function inspect() {
		if (!active || loading || availability.state !== "ready") return;
		const original = scope;
		const request = ++generation;
		loading = true;
		failure = "";
		report = null;
		try {
			const next = await inspectDatabaseSql(credentials, database, table);
			if (!active || request !== generation || original !== scope) return;
			report = next;
			reportScope = original;
		} catch (caught) {
			if (!active || request !== generation || original !== scope) return;
			failure = caught instanceof Error ? caught.message : String(caught);
			failureScope = original;
		} finally {
			if (active && request === generation) loading = false;
		}
	}
</script>

<details class="min-w-0 border-t border-border pt-4" open={Boolean(error.trim())}>
	<summary class="cursor-pointer text-sm font-medium">
		SQL diagnostics
		<span class="mt-1 block text-xs leading-5 font-normal text-muted-foreground">Explain SQL errors and check table definitions. Read-only.</span>
	</summary>
	<div class="mt-4 min-w-0 space-y-4 text-sm">
		<p class="text-muted-foreground">Use this after a failed query or import, or before comparing a resource's SQL with your database. Error guidance explains common causes; schema inspection checks visible tables, column types, character sets and collations.</p>
		<p class="text-xs leading-5 text-muted-foreground">Nothing is repaired or retried. Inspection reads metadata, not row data, and does not change your database or SQL file.</p>
		{#if error.trim()}
			<section aria-label="SQL error explanation" class="space-y-2">
				<p class="text-xs font-medium text-muted-foreground">Last SQL error</p>
				<h3 class="text-sm font-medium text-amber-300">{diagnosis.title}</h3>
				<p class="text-muted-foreground">{diagnosis.explanation}</p>
				<p class="text-xs font-medium">What to check next</p>
				<ol class="list-decimal space-y-2 pl-5 text-xs leading-5 text-muted-foreground">
					{#each diagnosis.checks as check}<li>{check}</li>{/each}
				</ol>
			</section>
		{:else}
			<p class="text-xs text-muted-foreground">No SQL error to explain. You can still inspect the selected database without running a query or file.</p>
		{/if}
		<div class="space-y-3 border-t border-border pt-4">
			<h3 class="text-sm font-medium">Inspect database schema</h3>
			{#if database && database !== "__global__"}
				<p class="text-xs text-muted-foreground">Selected database: <strong class="font-mono font-medium text-foreground [overflow-wrap:anywhere]">{database}</strong></p>
			{/if}
			<div class="flex flex-wrap items-end gap-3">
				<div class="grid min-w-0 flex-[1_1_14rem] gap-2">
					<label for={`${id}-table`} class="text-xs font-medium">Table name (optional)</label>
					<Input id={`${id}-table`} bind:value={table} aria-describedby={`${id}-table-help`} placeholder="All visible tables" disabled={loading || availability.state !== "ready"} />
				</div>
				<Button variant="outline" onclick={inspect} aria-describedby={`${id}-inspection-state`} disabled={loading || availability.state !== "ready"}>
					{#if loading}<LoaderCircleIcon class="animate-spin" />{:else}<SearchCheckIcon />{/if}
					{loading ? "Inspecting..." : "Inspect schema"}
				</Button>
			</div>
			<p id={`${id}-table-help`} class="text-xs leading-5 text-muted-foreground">Leave blank for all visible tables, or enter one exact table name (for example, players), without SQL or backticks. A named table also includes its existing foreign-key relationships.</p>
			<p class="text-xs leading-5 text-muted-foreground">Character sets control which text can be stored; collations control how text is compared and sorted.</p>
		</div>
		<div id={`${id}-inspection-state`} role="status" aria-live="polite" aria-atomic="true" class="space-y-1 text-xs leading-5 text-muted-foreground">
			{#if loading}
				<p class="font-medium text-foreground">Reading schema metadata...</p>
				<p>No SQL from the editor or file is being executed.</p>
			{:else if availability.message}
				<p>{availability.message}</p>
			{:else if summary}
				<p class="font-medium text-foreground">{summary.title}</p>
				<p>{summary.description}</p>
			{:else if !visibleFailure}
				<p>Not inspected yet. Inspect schema to see the current table definitions for this selection.</p>
			{/if}
		</div>
		{#if visibleFailure}
			<Notice tone="error" title="Schema inspection could not finish" message={visibleFailure} onDismiss={() => (failure = "")} />
			<p class="text-xs leading-5 text-muted-foreground">Check the connection, selected database and this account's permissions, then inspect again when ready. No SQL was repaired or retried.</p>
		{/if}
		{#if visibleReport && summary}
			<div class="min-w-0 space-y-3 text-xs leading-5">
				<p class="text-muted-foreground">{summary.nextStep}</p>
				{#if visibleReport.truncated}
					<p class="text-amber-300">Results are limited to 500 columns and 500 foreign-key pairs. Some metadata is omitted. Enter a table name to narrow an all-table inspection.</p>
				{/if}
				{#if summary.columnCount}
					<p class="text-muted-foreground">Text metadata: {summary.charsets.length} character set{summary.charsets.length === 1 ? "" : "s"} and {summary.collations.length} collation{summary.collations.length === 1 ? "" : "s"} returned. See Column details for each column's settings.</p>
					{#if summary.collations.length > 1}
						<p class="text-muted-foreground">Multiple collations are not automatically an error. Compare only the columns involved in the failing statement.</p>
					{/if}
					<details class="min-w-0 border-t border-border pt-3">
						<summary class="cursor-pointer font-medium">Column details ({summary.columnCount})</summary>
						<!-- svelte-ignore a11y_no_noninteractive_tabindex (Keyboard users need to scroll the metadata table.) -->
						<div class="mt-3 max-h-72 overflow-auto" role="region" aria-label="Inspected columns" tabindex="0">
							<table class="w-full text-left text-xs">
								<caption class="sr-only">Visible columns in {database}</caption>
								<thead class="sticky top-0 bg-muted">
									<tr>{#each ["Table / column", "Type", "Character set", "Collation", "Engine"] as label}<th scope="col" class="p-2 font-medium">{label}</th>{/each}</tr>
								</thead>
								<tbody>
									{#each visibleReport.columns as column}
										<tr class="border-b border-border">
											<td class="p-2 align-top font-mono [overflow-wrap:anywhere]">{column.table}.{column.column}</td>
											<td class="min-w-32 p-2 align-top [overflow-wrap:anywhere]">{column.columnType}</td>
											<td class="p-2 align-top">{column.charset ?? "Not applicable"}</td>
											<td class="p-2 align-top">{column.collation ?? "Not applicable"}</td>
											<td class="p-2 align-top">{column.engine ?? "Not available"}</td>
										</tr>
									{/each}
								</tbody>
							</table>
						</div>
					</details>
				{/if}
				<details class="border-t border-border pt-3">
					<summary class="cursor-pointer font-medium">Server details</summary>
					<dl class="mt-3 grid gap-x-4 gap-y-2 [overflow-wrap:anywhere] sm:grid-cols-[auto_minmax(0,1fr)]">
						<dt class="text-muted-foreground">Server version</dt><dd>{visibleReport.environment.version}</dd>
						<dt class="text-muted-foreground">Global SQL mode</dt><dd>{visibleReport.environment.sqlMode || "None"}</dd>
						<dt class="text-muted-foreground">Database default collation</dt><dd>{visibleReport.environment.schemaCollation ?? "Not available"}</dd>
						<dt class="text-muted-foreground">Inspection character set</dt><dd>{visibleReport.environment.charset}</dd>
						<dt class="text-muted-foreground">Inspection collation</dt><dd>{visibleReport.environment.collation}</dd>
					</dl>
					<p class="mt-2 text-muted-foreground">These are the inspection connection's settings, not necessarily those of the failed SQL session or your resource. The database default does not describe every existing column.</p>
				</details>
				{#if table.trim()}
					<details class="border-t border-border pt-3">
						<summary class="cursor-pointer font-medium">Existing foreign keys ({visibleReport.foreignKeys.length})</summary>
						<div class="mt-3 space-y-3">
							{#each visibleReport.foreignKeys as key}
								<div class="[overflow-wrap:anywhere]">
									<p class="font-medium">{key.constraint}</p>
									<p class="font-mono">{table.trim()}.{key.column} &rarr; {key.parentDatabase}.{key.parentTable}.{key.parentColumn}</p>
									<p class="text-muted-foreground">Referenced type: {key.parentType ?? "Not visible"}. Referenced collation: {key.parentCollation ?? "Not returned (non-text or not visible)"}.</p>
								</div>
							{:else}
								<p class="text-muted-foreground">No existing foreign keys were returned for this table. A relationship rejected by the server will not appear here.</p>
							{/each}
						</div>
					</details>
				{:else}
					<p class="text-muted-foreground">To review existing foreign keys, enter a table name and inspect again. Relationships are not fetched for an all-table inspection.</p>
				{/if}
				<p class="text-muted-foreground">Only metadata visible to this account is included. Inspection does not verify row data, indexes, SQL syntax or whether a failed script was partially applied.</p>
			</div>
		{/if}
	</div>
</details>
