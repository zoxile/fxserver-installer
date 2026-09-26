<script lang="ts">
	import PlayIcon from "@lucide/svelte/icons/play";
	import TerminalIcon from "@lucide/svelte/icons/terminal";
	import WandSparklesIcon from "@lucide/svelte/icons/wand-sparkles";
	import XIcon from "@lucide/svelte/icons/x";
	import { onDestroy, tick, untrack } from "svelte";
	import { databaseSession } from "$lib/core/databaseSession.svelte";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import SavedQueries from "./SavedQueries.svelte";
	import { containsLibrarySecret, type SavedQuery } from "./databaseLibrary";
	import { currentLibrarySecrets } from "./databaseLibrarySession.svelte";
	import { prepareQueryDraft, queryParameters } from "./savedQueryParameters";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import * as Select from "$lib/components/ui/select/index.js";
	import type { MariaDBQueryResult } from "$lib/modules/mariadb";
	import type { Snippet } from "svelte";

	type Props = {
		busy: boolean;
		canExecute: boolean;
		databases: string[];
		selectedDatabase: string;
		query: string;
		result: MariaDBQueryResult | null;
		onExecute: () => void | Promise<void>;
		children?: Snippet;
	};

	let { busy, canExecute, databases, selectedDatabase = $bindable(), query = $bindable(), result, onExecute, children }: Props = $props();
	const globalQueryValue = "__global__";
	const booleanOptions = [{ value: "true", label: "true" }, { value: "false", label: "false" }];
	let selectedHelperId = $state("select-filtered");
	let helperTable = $state("users");
	let helperColumns = $state("*");
	let helperFilter = $state("id = 1");
	let helperLimit = $state("25");
	let parameterValues = $state<Record<string, string>>({});
	let review = $state<{ source: string; sql: string; database: string; session: typeof databaseSession.validated; revision: number } | null>(null);
	let confirmation = $state("");
	let queryNotice = $state("");
	let executing = $state(false);
	let active = true;
	const parameters = $derived.by(() => {
		try { return { items: queryParameters(query), error: "" }; }
		catch (error) { return { items: [], error: error instanceof Error ? error.message : "Invalid parameter template." }; }
	});
	const reviewCurrent = $derived(!!review && review.source === query && review.database === selectedDatabase && review.session === databaseSession.validated && review.revision === databaseSession.revision && canExecute);
	const blocked = $derived(busy || executing || !canExecute);
	onDestroy(() => { active = false; });
	$effect(() => {
		query; selectedDatabase; databaseSession.validated; databaseSession.revision;
		untrack(() => { if (!executing) { parameterValues = {}; review = null; confirmation = ""; } });
	});

	function loadSavedQuery(saved: SavedQuery) {
		if (blocked) return;
		if (saved.database !== globalQueryValue && !databases.includes(saved.database)) { queryNotice = "This saved query's database is not available on the current connection."; return; }
		query = saved.sql; selectedDatabase = saved.database;
		parameterValues = {}; review = null; confirmation = ""; queryNotice = "";
	}
	function reviewQuery() {
		if (blocked || !query.trim()) return;
		queryNotice = "";
		try {
			if (parameters.items.length && Object.values(parameterValues).some((value) => containsLibrarySecret(value, currentLibrarySecrets()))) throw new Error("Known credentials or secret-shaped parameter values are not allowed in drafts.");
			const sql = prepareQueryDraft(query, parameterValues);
			review = { source: query, sql, database: selectedDatabase, session: databaseSession.validated, revision: databaseSession.revision };
			confirmation = "";
		} catch (error) { queryNotice = error instanceof Error ? error.message : "Could not prepare the SQL draft."; }
	}
	function previewSql(sql: string) {
		return containsLibrarySecret(sql, currentLibrarySecrets())
			? "[Sensitive SQL preview withheld. Review the in-memory editor before confirming. This SQL cannot be saved.]"
			: sql;
	}
	async function executeReviewed() {
		if (blocked || !reviewCurrent || !review || confirmation !== "RUN") return;
		const selected = review;
		executing = true; review = null; confirmation = "";
		try {
			// Preserve the parent's execution/session/cache guards. Its callback reads the
			// bound editor, so temporarily supply the reviewed draft and restore the template.
			query = selected.sql;
			await tick();
			if (!active || !canExecute || selected.session !== databaseSession.validated || selected.revision !== databaseSession.revision || selected.database !== selectedDatabase || query !== selected.sql) return;
			await onExecute();
		} catch { if (active) queryNotice = "Query execution failed. Check the query result before retrying."; }
		finally {
			if (active) {
				if (query === selected.sql) query = selected.source;
				parameterValues = {}; executing = false;
			}
		}
	}

	const databaseOptions = $derived([
		{ value: globalQueryValue, label: "Global query" },
		...databases.map((database) => ({ value: database, label: database })),
	]);

	type QueryHelper = {
		id: string;
		label: string;
		category: string;
		description: string;
		build: () => string;
	};

	const queryHelpers: QueryHelper[] = [
		{
			id: "select-filtered",
			label: "Select rows",
			category: "Read",
			description: "Fetch rows from a table with an optional WHERE filter and LIMIT.",
			build: () => `SELECT ${columnList()}\nFROM ${tableName()}\nWHERE ${filterClause()}\nLIMIT ${limitCount()};`,
		},
		{
			id: "select-all",
			label: "Select latest rows",
			category: "Read",
			description: "Fetch the latest rows from a table. Change the ORDER BY column if needed.",
			build: () => `SELECT ${columnList()}\nFROM ${tableName()}\nORDER BY id DESC\nLIMIT ${limitCount()};`,
		},
		{
			id: "count-filtered",
			label: "Count rows",
			category: "Read",
			description: "Count all rows that match a filter.",
			build: () => `SELECT COUNT(*) AS total\nFROM ${tableName()}\nWHERE ${filterClause()};`,
		},
		{
			id: "distinct-values",
			label: "Distinct values",
			category: "Read",
			description: "List unique values from one column.",
			build: () => `SELECT DISTINCT ${firstColumn()}\nFROM ${tableName()}\nWHERE ${filterClause()}\nORDER BY ${firstColumn()}\nLIMIT ${limitCount()};`,
		},
		{
			id: "search-text",
			label: "Search text",
			category: "Read",
			description: "Search a text column with LIKE.",
			build: () => `SELECT ${columnList()}\nFROM ${tableName()}\nWHERE ${firstColumn()} LIKE '%search text%'\nLIMIT ${limitCount()};`,
		},
		{
			id: "show-databases",
			label: "Show databases",
			category: "Inspect",
			description: "List databases visible to the current user.",
			build: () => "SHOW DATABASES;",
		},
		{
			id: "show-tables",
			label: "Show tables",
			category: "Inspect",
			description: "List tables in the selected query scope.",
			build: () => "SHOW TABLES;",
		},
		{
			id: "describe-table",
			label: "Describe table",
			category: "Inspect",
			description: "Show columns, types, keys, defaults, and extra table metadata.",
			build: () => `DESCRIBE ${tableName()};`,
		},
		{
			id: "table-size",
			label: "Table sizes",
			category: "Inspect",
			description: "Find large tables in the selected database.",
			build: () =>
				"SELECT table_name, table_rows, ROUND((data_length + index_length) / 1024 / 1024, 2) AS size_mb\nFROM information_schema.tables\nWHERE table_schema = DATABASE()\nORDER BY size_mb DESC\nLIMIT 25;",
		},
		{
			id: "insert-row",
			label: "Insert row",
			category: "Write",
			description: "Insert a row. Replace the columns and values before running.",
			build: () => `INSERT INTO ${tableName()} (${insertColumns()})\nVALUES ('value_a', 'value_b');`,
		},
		{
			id: "update-filtered",
			label: "Update rows",
			category: "Write",
			description: "Update matching rows with a safety LIMIT.",
			build: () => `UPDATE ${tableName()}\nSET ${firstColumn()} = 'new value'\nWHERE ${filterClause()}\nLIMIT ${limitCount()};`,
		},
		{
			id: "delete-filtered",
			label: "Delete rows",
			category: "Write",
			description: "Delete matching rows with a WHERE filter and safety LIMIT.",
			build: () => `DELETE FROM ${tableName()}\nWHERE ${filterClause()}\nLIMIT ${limitCount()};`,
		},
		{
			id: "truncate-table",
			label: "Empty table",
			category: "Danger",
			description: "Remove every row in a table. Use only after a backup.",
			build: () => `TRUNCATE TABLE ${tableName()};`,
		},
		{
			id: "create-database",
			label: "Create database",
			category: "Admin",
			description: "Create a utf8mb4 database if it does not exist.",
			build: () => "CREATE DATABASE IF NOT EXISTS database_name CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;",
		},
		{
			id: "create-user",
			label: "Create user",
			category: "Admin",
			description: "Create a local MariaDB user. Replace the password before running.",
			build: () => "CREATE USER IF NOT EXISTS 'fxserver'@'localhost' IDENTIFIED BY 'change-me';",
		},
		{
			id: "grant-user",
			label: "Grant database access",
			category: "Admin",
			description: "Grant a user access to a database and flush privileges.",
			build: () => "GRANT ALL PRIVILEGES ON database_name.* TO 'fxserver'@'localhost';\nFLUSH PRIVILEGES;",
		},
		{
			id: "show-grants",
			label: "Show user grants",
			category: "Admin",
			description: "Inspect permissions for a MariaDB user.",
			build: () => "SHOW GRANTS FOR 'fxserver'@'localhost';",
		},
	];

	const helperOptions = queryHelpers.map((helper) => ({
		value: helper.id,
		label: `${helper.category}: ${helper.label}`,
	}));
	const selectedHelper = $derived(queryHelpers.find((helper) => helper.id === selectedHelperId) ?? queryHelpers[0]);
	const helperPreview = $derived(selectedHelper.build());
	const resultDisplayLimit = 200;
	const visibleRows = $derived(result?.rows.slice(0, resultDisplayLimit) ?? []);
	const hiddenResultRows = $derived(result?.success && result.rows.length > resultDisplayLimit ? result.rows.length - resultDisplayLimit : 0);

	function applyHelper(mode: "replace" | "append") {
		if (blocked) return;
		const sql = helperPreview.trim();
		if (!sql) return;

		query = mode === "append" && query.trim() ? `${query.trim()}\n\n${sql}` : sql;
	}

	function tableName() {
		return quoteIdentifier(helperTable.trim() || "table_name");
	}

	function columnList() {
		return helperColumns.trim() || "*";
	}

	function firstColumn() {
		const first = helperColumns
			.split(",")
			.map((column) => column.trim())
			.find(Boolean);

		return first && first !== "*" ? first : "column_name";
	}

	function insertColumns() {
		const columns = helperColumns
			.split(",")
			.map((column) => column.trim())
			.filter(Boolean);

		return columns.length && columns[0] !== "*" ? columns.join(", ") : "column_a, column_b";
	}

	function filterClause() {
		return helperFilter.trim() || "id = 1";
	}

	function limitCount() {
		const parsed = Number.parseInt(helperLimit, 10);
		return Number.isFinite(parsed) && parsed > 0 ? String(parsed) : "25";
	}

	function quoteIdentifier(value: string): string {
		if (!value) return "`table_name`";
		if (value.includes(".")) {
			return value
				.split(".")
				.map((part) => quoteIdentifier(part.trim()))
				.join(".");
		}
		return `\`${value.replaceAll("`", "``")}\``;
	}
</script>

<Card.Root class="h-full rounded-md border-border bg-card shadow-sm">
	<Card.Header class="border-b border-border pb-4">
		<div class="flex items-center justify-between gap-4">
			<div class="flex min-w-0 items-center gap-3">
				<div class="flex size-9 shrink-0 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
					<TerminalIcon class="size-5" />
				</div>
				<div>
					<Card.Title>Query Console</Card.Title>
					<Card.Description>Run SQL against MariaDB using the connection above.</Card.Description>
				</div>
			</div>
			<Button onclick={reviewQuery} disabled={blocked || !query.trim() || !!parameters.error} title={canExecute ? "Review SQL before execution" : "Apply valid admin credentials before running queries"}>
				<PlayIcon />
				Review
			</Button>
		</div>
	</Card.Header>

	<Card.Content class="space-y-4">
		{#if queryNotice}<Notice tone="error" message={queryNotice} onDismiss={() => queryNotice = ""} />{/if}
		<SavedQueries {query} database={selectedDatabase} busy={blocked} onLoad={loadSavedQuery} />
		<div class="grid gap-2">
			<span class="text-xs font-medium text-muted-foreground">Query Scope</span>
			<Select.Root bind:value={selectedDatabase} type="single" items={databaseOptions} disabled={blocked}>
				<Select.Trigger title="Choose whether the query runs globally or inside one database" class="w-full rounded-sm font-mono text-xs">
					{selectedDatabase === globalQueryValue ? "Global query" : selectedDatabase || "Choose database"}
				</Select.Trigger>
				<Select.Content class="rounded-sm">
					{#each databaseOptions as option}
						<Select.Item value={option.value} label={option.label}>
							{option.label}
						</Select.Item>
					{/each}
				</Select.Content>
			</Select.Root>
		</div>

		<div class="rounded-sm border border-border bg-background/50 p-3">
			<div class="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
				<div class="min-w-0">
					<div class="flex items-center gap-2">
						<WandSparklesIcon class="size-4 text-muted-foreground" />
						<p class="text-sm font-medium text-foreground">Query Helper</p>
					</div>
					<p class="mt-1 text-xs leading-5 text-muted-foreground">{selectedHelper.description}</p>
				</div>
				<div class="flex shrink-0 gap-2">
					<Button variant="outline" size="sm" onclick={() => applyHelper("replace")} title="Replace the editor content with this helper query">Use</Button>
					<Button variant="outline" size="sm" onclick={() => applyHelper("append")} title="Append this helper query below the current SQL">Append</Button>
				</div>
			</div>

			<div class="mt-3 grid gap-3 lg:grid-cols-[minmax(0,1.1fr)_minmax(0,0.9fr)_minmax(0,1fr)_7rem]">
				<label class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">Helper</span>
					<Select.Root bind:value={selectedHelperId} type="single" items={helperOptions}>
						<Select.Trigger title="Choose a MariaDB query helper" class="w-full rounded-sm text-xs">
							{selectedHelper.category}: {selectedHelper.label}
						</Select.Trigger>
						<Select.Content class="rounded-sm">
							{#each helperOptions as option}
								<Select.Item value={option.value} label={option.label}>
									{option.label}
								</Select.Item>
							{/each}
						</Select.Content>
					</Select.Root>
				</label>
				<label class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">Table</span>
					<Input bind:value={helperTable} placeholder="users" title="Table used by helpers that need a table name." class="rounded-sm font-mono text-xs" />
				</label>
				<label class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">Columns</span>
					<Input bind:value={helperColumns} placeholder="*" title="Columns or expressions used by SELECT, INSERT, and UPDATE helpers." class="rounded-sm font-mono text-xs" />
				</label>
				<label class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">Limit</span>
					<Input bind:value={helperLimit} inputmode="numeric" placeholder="25" title="Safety limit for helper queries." class="rounded-sm font-mono text-xs" />
				</label>
			</div>

			<label class="mt-3 grid gap-2">
				<span class="text-xs font-medium text-muted-foreground">Filter / WHERE</span>
				<Input bind:value={helperFilter} placeholder="id = 1" title="WHERE filter used by read, update, and delete helpers." class="rounded-sm font-mono text-xs" />
			</label>

			<pre class="mt-3 max-h-32 overflow-auto rounded-sm border border-border bg-card/70 p-3 font-mono text-xs leading-5 text-muted-foreground whitespace-pre-wrap">{helperPreview}</pre>
		</div>

		<textarea
			bind:value={query}
			spellcheck="false"
			placeholder="SELECT * FROM users LIMIT 25;"
			title="SQL query to execute against MariaDB."
			disabled={blocked}
			autocomplete="off"
			aria-label="SQL query"
			class="min-h-40 w-full resize-y rounded-sm border border-input bg-background px-3 py-3 font-mono text-sm shadow-xs outline-none transition-[color,box-shadow] placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
		></textarea>
		{#if parameters.error}<Notice tone="error" message={parameters.error} />{/if}
		<details class="text-xs text-muted-foreground"><summary class="cursor-pointer">Parameter format</summary><p class="mt-2">Value placeholders: {"{{player_id:number}}"}, {"{{name:text}}"}, {"{{active:boolean}}"}, {"{{optional:null}}"}. Drafts use SQL literals, not server parameter binding. Identifiers cannot be parameters. Values stay in memory and are cleared after execution.</p></details>
		{#if parameters.items.length}
			<div class="grid gap-3 sm:grid-cols-2">
				{#each parameters.items as parameter (parameter.name)}
					<label class="grid min-w-0 gap-1 text-xs">{parameter.name} ({parameter.type})
						{#if parameter.type === "null"}<span class="font-mono">NULL</span>
						{:else if parameter.type === "boolean"}
							<Select.Root type="single" value={parameterValues[parameter.name] ?? ""} items={booleanOptions} disabled={blocked} onValueChange={(value) => { parameterValues[parameter.name] = value; review = null; }}>
								<Select.Trigger class="w-full min-w-0" aria-label={`${parameter.name} (boolean)`}>{parameterValues[parameter.name] || "Choose value"}</Select.Trigger>
								<Select.Content>{#each booleanOptions as option}<Select.Item value={option.value} label={option.label}>{option.label}</Select.Item>{/each}</Select.Content>
							</Select.Root>
						{:else}<Input value={parameterValues[parameter.name] ?? ""} oninput={(event) => { parameterValues[parameter.name] = event.currentTarget.value; review = null; }} maxlength={4096} autocomplete="off" spellcheck={false} disabled={blocked} />{/if}
					</label>
				{/each}
			</div>
		{/if}
		{#if review && reviewCurrent}
			<section class="space-y-3 border-y border-amber-500/50 py-4" aria-label="SQL execution confirmation">
				<h3 class="text-sm font-semibold">Confirm SQL execution</h3>
				<p class="break-all text-xs font-mono">{databaseSession.validated?.credentials.host}:{databaseSession.validated?.credentials.port} / {selectedDatabase === globalQueryValue ? "Global query" : selectedDatabase}</p>
				<p class="text-xs text-amber-400">SQL may change or permanently delete data, including from saved queries. Multi-statement failures can leave partial changes. No automatic retry.</p>
				{#if parameters.items.length}<p class="text-xs text-muted-foreground">Review-only parameter draft. Values are encoded as literals; the server does not bind them.</p>{/if}
				<pre class="max-h-60 overflow-auto whitespace-pre-wrap break-all bg-muted/30 p-3 font-mono text-xs">{previewSql(review.sql)}</pre>
				<div class="flex flex-wrap items-end gap-2"><label class="grid min-w-0 flex-1 gap-1 text-xs">Type RUN<Input bind:value={confirmation} autocomplete="off" maxlength={3} disabled={blocked} /></label><Button variant="destructive" disabled={blocked || confirmation !== "RUN"} onclick={executeReviewed}><PlayIcon />Execute</Button><Button variant="outline" size="icon" title="Cancel execution" aria-label="Cancel execution" disabled={blocked} onclick={() => { review = null; confirmation = ""; }}><XIcon /></Button></div>
			</section>
		{/if}

		{#if result}
			{#if result.success && result.columns.length}
				<div class="max-h-72 overflow-auto rounded-sm border border-border bg-background">
					<table class="w-full border-collapse text-left text-xs">
						<thead class="sticky top-0 bg-muted text-muted-foreground">
							<tr>
								{#each result.columns as column}
									<th class="border-b border-border px-3 py-2 font-medium whitespace-nowrap">{column}</th>
								{/each}
							</tr>
						</thead>
						<tbody>
							{#each visibleRows as row}
								<tr class="border-b border-border/70 last:border-0">
									{#each result.columns as _, index}
										<td class="px-3 py-2 align-top font-mono whitespace-nowrap">{row[index] ?? ""}</td>
									{/each}
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
				{#if hiddenResultRows}
					<p class="rounded-sm border border-border bg-muted/30 px-3 py-2 text-xs text-muted-foreground">
						Showing the first {resultDisplayLimit} rows. Add a `LIMIT` or narrower `WHERE` filter to reduce the result set. {hiddenResultRows} rows are hidden to keep the UI responsive.
					</p>
				{/if}
			{:else}
				<pre
					class={[
						"max-h-72 overflow-auto rounded-sm border p-4 font-mono text-xs leading-6 whitespace-pre-wrap",
						result.success
							? "border-border bg-muted/40 text-foreground"
							: "border-destructive/30 bg-destructive/10 text-destructive",
					]}
				>{result.stdout || result.stderr || "No output."}</pre>
			{/if}
		{/if}
		{@render children?.()}
	</Card.Content>
</Card.Root>
