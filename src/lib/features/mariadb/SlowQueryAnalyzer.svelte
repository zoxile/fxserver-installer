<script lang="ts">
	import { onDestroy } from "svelte";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import XIcon from "@lucide/svelte/icons/x";
	import ChevronLeftIcon from "@lucide/svelte/icons/chevron-left";
	import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
	import ArrowDownWideNarrowIcon from "@lucide/svelte/icons/arrow-down-wide-narrow";
	import ArrowUpNarrowWideIcon from "@lucide/svelte/icons/arrow-up-narrow-wide";
	import TextSelectIcon from "@lucide/svelte/icons/text-select";
	import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Checkbox } from "$lib/components/ui/checkbox/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import * as Select from "$lib/components/ui/select/index.js";
	import { readSelectedSlowLog } from "$lib/modules/slowQueryImport";
	import { slowQueryPage, slowQueryIndexAdvice, type SlowQueryReport, type SlowQuerySort } from "$lib/modules/slowQueries";

	let { database, disabled = false }: { database: string; disabled?: boolean } = $props();
	const id = $props.id();
	let report = $state.raw<SlowQueryReport | null>(null);
	let loading = $state(false);
	let failure = $state("");
	let search = $state("");
	let sort = $state<SlowQuerySort>("totalSeconds");
	let descending = $state(true);
	let page = $state(0);
	let selectedId = $state<number | null>(null);
	let reviewed = $state(false);
	let draftElement = $state<HTMLTextAreaElement>();
	let worker: Worker | null = null;
	let deadline: ReturnType<typeof setTimeout> | undefined;
	let generation = 0;
	let active = true;
	const sortOptions: { value: SlowQuerySort; label: string }[] = [
		{ value: "totalSeconds", label: "Cumulative time" }, { value: "count", label: "Count" },
		{ value: "averageSeconds", label: "Average time" }, { value: "maxSeconds", label: "Maximum time" },
		{ value: "rowsExamined", label: "Rows examined" },
	];
	const results = $derived(slowQueryPage(report?.groups ?? [], search, sort, descending, page));
	const selected = $derived(report?.groups.find((group) => group.id === selectedId) ?? null);
	const advice = $derived(selected ? slowQueryIndexAdvice(selected) : []);
	const integer = new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 });
	const seconds = new Intl.NumberFormat(undefined, { minimumFractionDigits: 3, maximumFractionDigits: 6 });
	const meanRows = new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 });

	$effect(() => { selectedId; reviewed = false; });

	function stopWorker() {
		worker?.terminate();
		worker = null;
		clearTimeout(deadline);
		deadline = undefined;
	}

	function clear() {
		generation++;
		stopWorker();
		report = null;
		selectedId = null;
		failure = "";
		search = "";
		page = 0;
		reviewed = false;
		loading = false;
	}

	onDestroy(() => { active = false; clear(); });

	async function importLog() {
		if (loading || disabled || !active) return;
		const request = ++generation;
		const isCurrent = () => active && request === generation;
		loading = true;
		failure = "";
		try {
			const content = await readSelectedSlowLog(isCurrent);
			if (!isCurrent()) return;
			if (content === null) { loading = false; return; }
			report = null;
			selectedId = null;
			search = "";
			page = 0;
			const next = new Worker(new URL("./slowQuery.worker.ts", import.meta.url), { type: "module" });
			worker = next;
			const fail = () => {
				if (!isCurrent()) return;
				stopWorker();
				loading = false;
				failure = "The log could not be analyzed. Import a smaller, complete UTF-8 slow query log.";
			};
			next.onerror = (event) => { event.preventDefault(); fail(); };
			next.onmessageerror = fail;
			next.onmessage = (event: MessageEvent<{ report?: SlowQueryReport; error?: string }>) => {
				if (!isCurrent()) return;
				stopWorker();
				loading = false;
				if (event.data.report) {
					report = event.data.report;
				} else failure = event.data.error ?? "The log could not be analyzed.";
			};
			deadline = setTimeout(fail, 15_000);
			next.postMessage(content);
		} catch {
			if (!isCurrent()) return;
			stopWorker();
			loading = false;
			failure = "Could not import the log. Choose a readable local UTF-8 .log file up to 4 MiB.";
		}
	}
</script>

<details class="min-w-0 border-t border-border pt-4">
	<summary class="cursor-pointer text-sm font-medium">Slow query analyzer</summary>
	<section aria-label="Slow query analyzer" class="mt-4 min-w-0 space-y-4">
		<div class="flex flex-wrap items-center gap-2">
			<Button variant="outline" onclick={importLog} disabled={loading || disabled}>
				{#if loading}<LoaderCircleIcon class="animate-spin" />{:else}<FolderOpenIcon />{/if}
				{loading ? "Analyzing log..." : "Import slow query log"}
			</Button>
			{#if report || loading || failure}
				<Button variant="ghost" size="icon" title={loading ? "Cancel import and clear analysis" : "Clear analysis"} aria-label={loading ? "Cancel import and clear analysis" : "Clear analysis"} onclick={clear}><XIcon /></Button>
			{/if}
		</div>
		<p class="text-xs leading-5 text-muted-foreground">Local UTF-8 .log, up to 4 MiB. Analysis stays in memory; no imported SQL is executed. Literal values and comments are removed, but identifiers can still be sensitive.</p>
		{#if failure}<Notice tone="error" title="Slow query import failed" message={failure} onDismiss={() => failure = ""} />{/if}
		<div role="status" aria-live="polite" class="text-xs text-muted-foreground">
			{#if loading}Reading and grouping the selected log...
			{:else if report}{integer.format(report.accepted)} statements in {integer.format(report.groups.length)} groups. {integer.format(report.skipped)} records skipped.
			{:else}No slow query log imported.{/if}
		</div>
		{#if report}
			{#if report.truncated}<Notice tone="warn" title="Partial analysis" message="The 20,000-record or 2,000-group limit was reached. Totals cover accepted records only; import a smaller log segment for complete results." />{/if}
			{#if report.skipped}<p class="text-xs leading-5 text-amber-300">Malformed, incomplete, oversized, multi-statement or executable-comment records were omitted. A statement is limited to 64 Ki characters, 4,096 tokens and a 16 Ki-character fingerprint.</p>{/if}
			{#if !report.groups.length}
				<p class="text-sm">No complete supported slow-query records found.</p>
			{:else}
				<dl class="grid grid-cols-2 gap-3 border-y border-border py-3 text-xs">
					<div><dt class="text-muted-foreground">Cumulative query time</dt><dd class="mt-1 font-mono tabular-nums [overflow-wrap:anywhere]">{seconds.format(report.totalSeconds)} s</dd></div>
					<div><dt class="text-muted-foreground">Total rows examined</dt><dd class="mt-1 font-mono tabular-nums [overflow-wrap:anywhere]">{integer.format(report.rowsExamined)}</dd></div>
				</dl>
				<p class="text-xs leading-5 text-muted-foreground">Times are seconds, not wall-clock duration. Groups distinguish database and identifier case. Counts reflect this log sample, not the full workload.</p>
				<div class="flex flex-wrap items-end gap-3">
					<div class="grid min-w-0 flex-[1_1_12rem] gap-2"><label for={`${id}-search`} class="text-xs font-medium">Search fingerprints or databases</label><Input id={`${id}-search`} value={search} maxlength={256} oninput={(event) => { search = event.currentTarget.value; page = 0; }} /></div>
					<div class="grid min-w-0 flex-[0_1_12rem] gap-2"><label for={`${id}-sort`} class="text-xs font-medium">Sort by</label><Select.Root type="single" value={sort} items={sortOptions} onValueChange={(value) => { sort = value as SlowQuerySort; page = 0; }}><Select.Trigger id={`${id}-sort`} class="w-full min-w-0 text-xs">{sortOptions.find((option) => option.value === sort)?.label}</Select.Trigger><Select.Content>{#each sortOptions as option}<Select.Item value={option.value} label={option.label}>{option.label}</Select.Item>{/each}</Select.Content></Select.Root></div>
					<Button variant="outline" size="icon" title={descending ? "Sort ascending" : "Sort descending"} aria-label={descending ? "Sort ascending" : "Sort descending"} onclick={() => { descending = !descending; page = 0; }}>{#if descending}<ArrowDownWideNarrowIcon />{:else}<ArrowUpNarrowWideIcon />{/if}</Button>
				</div>
				<!-- svelte-ignore a11y_no_noninteractive_tabindex (The bounded table must be keyboard-scrollable.) -->
				<div class="max-h-96 min-w-0 overflow-auto" role="region" aria-label="Slow query groups" tabindex="0">
					<table class="w-full min-w-[44rem] table-fixed text-left text-xs">
						<caption class="sr-only">Normalized slow queries; all time columns are in seconds and rows examined is cumulative</caption>
						<thead class="sticky top-0 bg-muted"><tr><th scope="col" class="w-64 p-2 font-medium">Query / database</th>{#each ["Count", "Total (s)", "Average (s)", "Max (s)", "Rows examined"] as label}<th scope="col" class="w-28 p-2 text-right font-medium">{label}</th>{/each}</tr></thead>
						<tbody>{#each results.groups as group (group.id)}
							<tr class={selectedId === group.id ? "border-b border-border bg-muted/60" : "border-b border-border"}>
								<td class="p-2"><button class="w-full min-w-0 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring" aria-label={`Review query group ${group.id}`} aria-pressed={selectedId === group.id} onclick={() => selectedId = group.id}><span class="block truncate font-mono text-muted-foreground">{group.database ?? "Unknown database"}</span><span class="mt-1 line-clamp-2 break-all font-mono">{group.fingerprint.slice(0, 180)}{group.fingerprint.length > 180 ? "..." : ""}</span></button></td>
								{#each [integer.format(group.count), seconds.format(group.totalSeconds), seconds.format(group.averageSeconds), seconds.format(group.maxSeconds), integer.format(group.rowsExamined)] as value}<td class="p-2 text-right align-top font-mono tabular-nums [overflow-wrap:anywhere]">{value}</td>{/each}
							</tr>
						{:else}<tr><td colspan="6" class="p-4 text-muted-foreground">No matching query groups.</td></tr>{/each}</tbody>
					</table>
				</div>
				<footer class="flex flex-wrap items-center justify-between gap-2 text-xs text-muted-foreground"><span>{integer.format(results.total)} groups. Page {results.page + 1} of {results.pages}.</span><div class="flex gap-2"><Button variant="outline" size="icon-sm" title="Previous query groups" aria-label="Previous query groups" disabled={results.page === 0} onclick={() => page = results.page - 1}><ChevronLeftIcon /></Button><Button variant="outline" size="icon-sm" title="Next query groups" aria-label="Next query groups" disabled={results.page + 1 >= results.pages} onclick={() => page = results.page + 1}><ChevronRightIcon /></Button></div></footer>
			{/if}
		{/if}
		{#if selected}
			<div class="min-w-0 space-y-3 border-t border-border pt-4">
				<h4 class="text-sm font-medium">Query group {selected.id}</h4>
				<pre class="max-h-48 overflow-auto whitespace-pre-wrap break-all font-mono text-xs leading-5">{selected.fingerprint}</pre>
				<p class="text-xs text-muted-foreground">Rows examined per statement: {meanRows.format(selected.averageRowsExamined)} average, {integer.format(selected.maxRowsExamined)} maximum.</p>
				<h5 class="text-xs font-medium">Index review</h5>
				<ul class="list-disc space-y-2 pl-5 text-xs leading-5 text-muted-foreground">{#each advice as item}<li>{item}</li>{/each}</ul>
				<div class="min-w-0 space-y-3 border-t border-border pt-3">
					<h5 class="text-xs font-medium">EXPLAIN draft for query console</h5>
					{#if selected.draft}
						<p class="text-xs leading-5 text-muted-foreground">Review-only template, not a verified execution plan. Replace placeholders and check referenced views and functions before manually running EXPLAIN with a least-privileged account. ANALYZE is excluded because it executes the query.</p>
						<p class="text-xs leading-5 [overflow-wrap:anywhere]">Logged database: <span class="font-mono">{selected.database ?? "Unknown"}</span>. Console selection: <span class="font-mono">{database && database !== "__global__" ? database : "No database selected"}</span>.</p>
						{#if !selected.database || selected.database !== database}<p class="text-xs text-amber-300">Database scope is unknown or different. Confirm the correct scope in the query console before using this draft.</p>{/if}
						<label class="flex items-start gap-2 text-xs leading-5"><Checkbox bind:checked={reviewed} /><span>I reviewed the database scope and the draft's limitations.</span></label>
						{#if reviewed}
							<textarea bind:this={draftElement} readonly aria-label="Reviewed EXPLAIN draft" value={selected.draft} spellcheck="false" class="h-48 w-full min-w-0 resize-y rounded-sm border border-input bg-background p-3 font-mono text-xs leading-5 outline-none focus-visible:ring-2 focus-visible:ring-ring"></textarea>
							<Button variant="outline" size="sm" onclick={() => { draftElement?.focus(); draftElement?.select(); }}><TextSelectIcon />Select draft</Button>
						{/if}
					{:else}<p class="text-xs leading-5 text-muted-foreground">{selected.draftReason}</p>{/if}
				</div>
			</div>
		{/if}
	</section>
</details>
