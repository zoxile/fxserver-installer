<script lang="ts">
	import { onMount } from "svelte";
	import { save } from "@tauri-apps/plugin-dialog";
	import DatabaseIcon from "@lucide/svelte/icons/database";
	import DownloadIcon from "@lucide/svelte/icons/download";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import ChevronLeftIcon from "@lucide/svelte/icons/chevron-left";
	import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
	import ArrowUpDownIcon from "@lucide/svelte/icons/arrow-up-down";
	import PlusIcon from "@lucide/svelte/icons/plus";
	import XIcon from "@lucide/svelte/icons/x";
	import FilterIcon from "@lucide/svelte/icons/filter";
	import PencilIcon from "@lucide/svelte/icons/pencil";
	import Trash2Icon from "@lucide/svelte/icons/trash-2";
	import * as Select from "$lib/components/ui/select/index.js";
	import * as Tabs from "$lib/components/ui/tabs/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import { Checkbox } from "$lib/components/ui/checkbox/index.js";
	import ConnectionCard from "./ConnectionCard.svelte";
	import DatabaseRowEditor from "./DatabaseRowEditor.svelte";
	import { getWorkspaceId } from "$lib/core/workspaces.svelte";
	import { databaseSession, rememberDatabaseCredentials } from "$lib/core/databaseSession.svelte";
	import { listMariaDBDatabases, listMariaDBTables, validateMariaDBCredentials, type MariaDBCredentials } from "$lib/modules/mariadb";
	import { exportBrowserCsv, getBrowserMetadata, getBrowserRows, type BrowserFilter, type BrowserMetadata, type BrowserPage, type BrowserRequest, type FilterOperator } from "$lib/modules/databaseBrowser";

	let credentials = $state<MariaDBCredentials>({ ...databaseSession.defaults, password: "", ...databaseSession.credentials });
	let validated = $state("");
	let databases = $state<string[]>([]);
	let tables = $state<string[]>([]);
	let database = $state("");
	let table = $state("");
	let metadata = $state.raw<BrowserMetadata>({ columns: [], indexes: [] });
	let page = $state.raw<BrowserPage>({ rows: [], hasMore: false, truncatedCells: false });
	let filters = $state<BrowserFilter[]>([]);
	let appliedFilters = $state<BrowserFilter[]>([]);
	let sortColumn = $state<string | null>(null);
	let descending = $state(false);
	let offset = $state(0);
	let pageSize = $state("25");
	let view = $state<"rows" | "columns" | "indexes">("rows");
	let busy = $state(false);
	let error = $state("");
	let message = $state("");
	let connectionError = $state("");
	let editMode = $state(false);
	let editor = $state<{ kind: "insert" | "update" | "delete"; original: (string | null)[] | null } | null>(null);
	const workspaceId = getWorkspaceId();
	let active = true;
	const credentialsReady = $derived(Boolean(validated) && JSON.stringify(credentials) === validated);
	const operators: { value: FilterOperator; label: string }[] = [
		{ value: "eq", label: "Equals" }, { value: "ne", label: "Not equal" }, { value: "contains", label: "Contains" },
		{ value: "lt", label: "Less than" }, { value: "lte", label: "At most" }, { value: "gt", label: "Greater than" },
		{ value: "gte", label: "At least" }, { value: "isNull", label: "IS NULL" }, { value: "isNotNull", label: "IS NOT NULL" },
	];
	const maxPageRows = $derived(Math.min(200, Math.floor(4000 / Math.max(1, metadata.columns.length))));
	const pageSizes = $derived([...new Set([25, 50, 100, 200, maxPageRows])].filter((value) => value <= maxPageRows).sort((a, b) => a - b).map((value) => ({ value: String(value), label: String(value) })));
	const databaseOptions = $derived(databases.map((value) => ({ value, label: value })));
	const tableOptions = $derived(tables.map((value) => ({ value, label: value })));
	const columnOptions = $derived(metadata.columns.map(({ name }) => ({ value: name, label: name })));

	onMount(() => { active = true; if (databaseSession.credentials) void connect(); return () => { active = false; }; });

	async function action(work: () => Promise<void>) {
		if (busy) return;
		busy = true; error = ""; message = "";
		try { await work(); } catch (caught) { if (active) error = String(caught); }
		finally { if (active) busy = false; }
	}
	function resetTable() {
		editMode = false; editor = null;
		metadata = { columns: [], indexes: [] }; page = { rows: [], hasMore: false, truncatedCells: false };
		filters = []; appliedFilters = []; offset = 0; sortColumn = null; descending = false;
	}
	async function connect() {
		await action(async () => {
			const original = { ...credentials }; const signature = JSON.stringify(original);
			connectionError = ""; validated = ""; databases = []; tables = []; database = ""; table = ""; resetTable();
			try {
				await validateMariaDBCredentials({ ...original, database: null });
				const available = await listMariaDBDatabases({ ...original, database: null });
				if (!active || signature !== JSON.stringify(credentials)) return;
				validated = signature; databases = available; rememberDatabaseCredentials(original);
				database = available.includes(original.database ?? "") ? original.database! : available.find((name) => !["mysql", "sys", "information_schema", "performance_schema"].includes(name)) ?? available[0] ?? "";
				await loadTables();
			} catch (caught) { connectionError = String(caught); throw caught; }
		});
	}
	async function loadTables() {
		const selected = database; const signature = JSON.stringify(credentials);
		table = ""; tables = []; resetTable();
		if (!selected || !credentialsReady) return;
		const available = await listMariaDBTables({ ...credentials, database: null }, selected);
		if (!active || selected !== database || signature !== JSON.stringify(credentials)) return;
		tables = available; table = available[0] ?? "";
		await loadMetadata();
	}
	async function loadMetadata() {
		resetTable();
		if (!database || !table || !credentialsReady) return;
		const key = `${database}/${table}`; const signature = JSON.stringify(credentials);
		const result = await getBrowserMetadata({ ...credentials }, database, table);
		if (!active || key !== `${database}/${table}` || signature !== JSON.stringify(credentials)) return;
		metadata = result; sortColumn = result.indexes.find((index) => index.name === "PRIMARY")?.column ?? result.columns[0]?.name ?? null;
		pageSize = String(Math.min(Number(pageSize), Math.floor(4000 / Math.max(1, result.columns.length))));
		await loadRows();
	}
	function request(): BrowserRequest { return { database, table, filters: appliedFilters.map((filter) => ({ ...filter })), sortColumn, descending, offset, pageSize: Number(pageSize) }; }
	async function loadRows() {
		if (!credentialsReady || !table) return;
		const query = request(); const signature = JSON.stringify(credentials);
		const result = await getBrowserRows({ ...credentials }, query);
		if (active && signature === JSON.stringify(credentials) && JSON.stringify(query) === JSON.stringify(request())) {
			page = result;
			if (result.pageSize) pageSize = String(result.pageSize);
		}
	}
	async function applyFilters() { await action(async () => { appliedFilters = filters.map((filter) => ({ ...filter })); offset = 0; await loadRows(); }); }
	async function sort(name: string) { await action(async () => { descending = sortColumn === name ? !descending : false; sortColumn = name; offset = 0; await loadRows(); }); }
	async function paginate(direction: number) { await action(async () => { offset = Math.max(0, offset + direction * Number(pageSize)); await loadRows(); }); }
	async function exportCsv() {
		await action(async () => {
			const query = request(); const original = { ...credentials };
			const outputPath = await save({ defaultPath: `${table.replace(/[^a-zA-Z0-9_-]/g, "_")}.csv`, filters: [{ name: "CSV", extensions: ["csv"] }] });
			if (!outputPath) return;
			const result = await exportBrowserCsv(original, query, outputPath);
			if (active) message = `${result.rows} rows exported${result.hasMore ? " (5,000-row limit reached)" : ""}: ${result.path}. SQL NULL is \\N; spreadsheet formulas are prefixed with an apostrophe.`;
		});
	}
</script>

<section class="min-w-0 space-y-5">
	<header class="flex flex-wrap items-center justify-between gap-3">
		<div class="flex flex-wrap items-center gap-3"><DatabaseIcon class="size-6 text-muted-foreground" /><h1 class="text-2xl font-semibold">Database Browser</h1><span class={editMode ? "text-xs text-amber-400" : "text-xs text-muted-foreground"}>{editMode ? "Editing enabled" : "Read-only"}</span></div>
		<Button size="icon" variant="outline" disabled={busy || !credentialsReady || !table} onclick={() => action(loadMetadata)} title="Refresh table" aria-label="Refresh table"><RefreshCwIcon class={busy ? "animate-spin" : ""} /></Button>
	</header>
	{#if error}<Notice tone="error" message={error} onDismiss={() => error = ""} />{/if}
	{#if message}<Notice tone="success" {message} onDismiss={() => message = ""} />{/if}
	<details open={!credentialsReady}><summary class="mb-3 cursor-pointer text-sm font-medium">Connection {credentialsReady ? ` / ${credentials.host}:${credentials.port}` : ""}</summary><ConnectionCard bind:credentials {busy} {credentialsReady} {connectionError} stretch={false} onApply={connect} /></details>
	<div class="grid gap-4 border-y border-border py-4 sm:grid-cols-2">
		<div class="grid min-w-0 gap-2"><label for="browser-database" class="text-xs font-medium">Database</label><Select.Root type="single" value={database} items={databaseOptions} disabled={busy || !credentialsReady} onValueChange={(value) => { database = value; void action(loadTables); }}><Select.Trigger id="browser-database" class="w-full min-w-0 font-mono text-xs"><span class="truncate">{database || "Choose database"}</span></Select.Trigger><Select.Content>{#each databaseOptions as option}<Select.Item value={option.value} label={option.label}>{option.label}</Select.Item>{/each}</Select.Content></Select.Root></div>
		<div class="grid min-w-0 gap-2"><label for="browser-table" class="text-xs font-medium">Table</label><Select.Root type="single" value={table} items={tableOptions} disabled={busy || !credentialsReady || !database} onValueChange={(value) => { table = value; void action(loadMetadata); }}><Select.Trigger id="browser-table" class="w-full min-w-0 font-mono text-xs"><span class="truncate">{table || "Choose table"}</span></Select.Trigger><Select.Content>{#each tableOptions as option}<Select.Item value={option.value} label={option.label}>{option.label}</Select.Item>{/each}</Select.Content></Select.Root></div>
	</div>
	{#if table && credentialsReady}
		<Tabs.Root bind:value={view} class="space-y-5" loop>
		<div class="flex flex-wrap items-center justify-between gap-3 border-b border-border">
			<Tabs.List aria-label="Table views">{#each ["rows", "columns", "indexes"] as tab}<Tabs.Trigger value={tab}>{tab}</Tabs.Trigger>{/each}</Tabs.List>
			<Button variant="outline" size="sm" disabled={busy} onclick={exportCsv} title="Export up to 5,000 filtered rows, maximum 8 MiB"><DownloadIcon />Export CSV</Button>
		</div>
		<div class="flex flex-wrap items-center justify-between gap-3 text-xs"><label class="flex items-center gap-2"><Checkbox bind:checked={editMode} disabled={busy || !metadata.editable} onCheckedChange={() => editor = null} />Enable row editing</label>{#if metadata.editReason}<span class="text-muted-foreground">{metadata.editReason}</span>{/if}{#if editMode}<Button variant="outline" size="sm" disabled={busy} onclick={() => editor = { kind: "insert", original: null }}><PlusIcon />Insert Row</Button>{/if}</div>
		{#if editMode && editor}{#key editor}<DatabaseRowEditor {metadata} kind={editor.kind} original={editor.original} {database} {table} {workspaceId} {credentials} onClose={() => editor = null} onBusy={(value) => busy = value} onApplied={async () => { editor = null; busy = false; await action(loadRows); message = "One row changed."; }} />{/key}{/if}
		{#if view === "rows"}
			<Tabs.Content value="rows" class="space-y-4">
			<div class="space-y-2">
				{#each filters as filter, i}
					<div class="grid items-center gap-2 sm:grid-cols-[minmax(0,1fr)_10rem_minmax(0,1fr)_auto]">
						<Select.Root type="single" bind:value={filter.column} items={columnOptions} disabled={busy}><Select.Trigger class="w-full min-w-0" aria-label={`Filter ${i + 1} column`}><span class="truncate">{filter.column}</span></Select.Trigger><Select.Content>{#each columnOptions as option}<Select.Item value={option.value} label={option.label}>{option.label}</Select.Item>{/each}</Select.Content></Select.Root>
						<Select.Root type="single" bind:value={filter.operator} items={operators} disabled={busy}><Select.Trigger class="w-full" aria-label={`Filter ${i + 1} operator`}>{operators.find((op) => op.value === filter.operator)?.label}</Select.Trigger><Select.Content>{#each operators as option}<Select.Item value={option.value} label={option.label}>{option.label}</Select.Item>{/each}</Select.Content></Select.Root>
						<Input bind:value={filter.value} aria-label={`Filter ${i + 1} value`} maxlength={512} disabled={busy || ["isNull", "isNotNull"].includes(filter.operator)} />
						<Button size="icon-sm" variant="ghost" title="Remove filter" aria-label={`Remove filter ${i + 1}`} disabled={busy} onclick={() => filters = filters.filter((_, index) => index !== i)}><XIcon /></Button>
					</div>
				{/each}
				<div class="flex gap-2"><Button variant="ghost" size="sm" disabled={busy || filters.length >= 8 || !metadata.columns.length} onclick={() => filters = [...filters, { column: metadata.columns[0].name, operator: "eq", value: "" }]}><PlusIcon />Filter</Button><Button variant="outline" size="sm" onclick={applyFilters} disabled={busy}><FilterIcon />Apply</Button></div>
			</div>
			{#if page.truncatedCells}<p class="text-xs text-amber-400">Some cells exceed the 4,096-character preview limit.</p>{/if}
			<div class="max-h-[32rem] overflow-auto border-y border-border" aria-busy={busy}>
				<table class="w-full border-collapse text-left text-xs"><thead class="sticky top-0 bg-background"><tr>{#each metadata.columns as column}<th class="min-w-32 border-b border-border px-3 py-2 font-medium"><button type="button" class="flex items-center gap-2 whitespace-nowrap" disabled={busy} onclick={() => sort(column.name)} title={`Sort ${column.name}`}><span>{column.name}</span><ArrowUpDownIcon class="size-3" />{sortColumn === column.name ? (descending ? "DESC" : "ASC") : ""}</button></th>{/each}{#if editMode}<th class="w-20 border-b border-border px-3 py-2">Actions</th>{/if}</tr></thead><tbody>{#each page.rows as row}<tr class="border-b border-border/50 hover:bg-muted/40">{#each row as cell}<td class="max-w-80 truncate px-3 py-2 font-mono" class:text-muted-foreground={cell === null} title={cell === null ? "SQL NULL" : cell}>{cell === null ? "NULL" : cell === "" ? '""' : cell}</td>{/each}{#if editMode}<td class="whitespace-nowrap px-2"><Button variant="ghost" size="icon-sm" title="Edit this row" aria-label="Edit this row" disabled={busy || page.truncatedCells} onclick={() => editor = { kind: "update", original: [...row] }}><PencilIcon /></Button><Button variant="ghost" size="icon-sm" title="Delete this row" aria-label="Delete this row" disabled={busy || page.truncatedCells} onclick={() => editor = { kind: "delete", original: [...row] }}><Trash2Icon /></Button></td>{/if}</tr>{:else}<tr><td colspan={Math.max(1, metadata.columns.length + (editMode ? 1 : 0))} class="px-3 py-8 text-center text-muted-foreground">{busy ? "Loading rows..." : "No rows match."}</td></tr>{/each}</tbody></table>
			</div>
			<footer class="flex flex-wrap items-center justify-between gap-3 text-xs text-muted-foreground"><span>{page.rows.length ? `${offset + 1}-${offset + page.rows.length}` : "0"} rows</span><div class="flex items-center gap-2"><Select.Root type="single" value={pageSize} items={pageSizes} disabled={busy} onValueChange={(value) => { pageSize = value; offset = 0; void action(loadRows); }}><Select.Trigger class="w-20" aria-label="Rows per page">{pageSize}</Select.Trigger><Select.Content>{#each pageSizes as option}<Select.Item value={option.value} label={option.label}>{option.label}</Select.Item>{/each}</Select.Content></Select.Root><Button variant="outline" size="icon-sm" title="Previous page" aria-label="Previous page" disabled={busy || !offset} onclick={() => paginate(-1)}><ChevronLeftIcon /></Button><Button variant="outline" size="icon-sm" title="Next page" aria-label="Next page" disabled={busy || !page.hasMore || offset + Number(pageSize) > 1000000} onclick={() => paginate(1)}><ChevronRightIcon /></Button></div></footer>
			</Tabs.Content>
		{:else if view === "columns"}
			<Tabs.Content value="columns">
			<div class="overflow-auto"><table class="w-full text-left text-xs"><thead><tr>{#each ["Column", "Type", "Nullable", "Default", "Extra"] as heading}<th class="border-b border-border p-3">{heading}</th>{/each}</tr></thead><tbody>{#each metadata.columns as column}<tr class="border-b border-border/50"><td class="p-3 font-mono">{column.name}</td><td class="p-3 font-mono">{column.columnType}</td><td class="p-3">{column.nullable ? "Yes" : "No"}</td><td class="max-w-64 truncate p-3" title={column.defaultValue ?? "NULL"}>{column.defaultValue ?? "NULL"}</td><td class="p-3">{column.extra || "-"}</td></tr>{/each}</tbody></table></div>
			</Tabs.Content>
		{:else}
			<Tabs.Content value="indexes">
			<div class="overflow-auto"><table class="w-full text-left text-xs"><thead><tr>{#each ["Index", "Column", "Position", "Unique", "Type"] as heading}<th class="border-b border-border p-3">{heading}</th>{/each}</tr></thead><tbody>{#each metadata.indexes as index}<tr class="border-b border-border/50"><td class="p-3 font-mono">{index.name}</td><td class="p-3 font-mono">{index.column ?? "Expression"}</td><td class="p-3">{index.sequence}</td><td class="p-3">{index.unique ? "Yes" : "No"}</td><td class="p-3">{index.indexType}</td></tr>{:else}<tr><td colspan="5" class="p-6 text-center text-muted-foreground">No indexes.</td></tr>{/each}</tbody></table></div>
			</Tabs.Content>
		{/if}
		</Tabs.Root>
	{:else if credentialsReady}<p class="py-8 text-sm text-muted-foreground">No accessible base tables in the selected database.</p>{/if}
</section>
