<script lang="ts">
	import DatabaseIcon from "@lucide/svelte/icons/database";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import PlayIcon from "@lucide/svelte/icons/play";
	import { onDestroy, onMount, untrack } from "svelte";
	import BackupCard from "./BackupCard.svelte";
	import ConnectionCard from "./ConnectionCard.svelte";
	import QueryConsole from "./QueryConsole.svelte";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import * as Select from "$lib/components/ui/select/index.js";
	import { databaseSession, rememberDatabaseCredentials } from "$lib/core/databaseSession.svelte";
	import { chooseSqlFile } from "$lib/core/selectFile";
	import {
		backupMariaDB,
		executeMariaDBQuery,
		getDefaultMariaDBBackupOutputDir,
		listMariaDBDatabases,
		listMariaDBTables,
		validateMariaDBCredentials,
		type MariaDBBackupOptions,
		type MariaDBCredentials,
		type MariaDBQueryResult,
	} from "$lib/modules/mariadb";
	import { readTextFile } from "$lib/modules/system";

	const globalScope = "__global__";
	let credentials = $state<MariaDBCredentials>({
		host: databaseSession.credentials?.host ?? databaseSession.defaults.host,
		port: databaseSession.credentials?.port ?? databaseSession.defaults.port,
		username: databaseSession.credentials?.username ?? databaseSession.defaults.username,
		password: databaseSession.credentials?.password ?? "",
		database: databaseSession.credentials?.database ?? databaseSession.defaults.database,
	});
	let databases = $state<string[]>([]);
	let selectedScope = $state(globalScope);
	let queryDatabase = $state(globalScope);
	let query = $state("SELECT VERSION();");
	let queryResult = $state<MariaDBQueryResult | null>(null);
	let sqlPath = $state("");
	let sqlContent = $state("");
	let busy = $state(false);
	let message = $state("");
	let error = $state("");
	let result = $state<MariaDBQueryResult | null>(null);
	let credentialsReady = $state(Boolean(databaseSession.credentials));
	let connectionError = $state("");
	let backupWarningDismissed = $state(false);
	let backupTables = $state<string[]>([]);
	let backupMode = $state<"database" | "tables" | "all">("database");
	let backupDatabaseName = $state(databaseSession.credentials?.database ?? "");
	let selectedBackupTable = $state("");
	let backupTableRequestId = 0;
	let active = true;
	onDestroy(() => { active = false; backupTableRequestId += 1; });
	let backupOptions = $state<MariaDBBackupOptions>({
		outputDir: "",
		fileName: "",
		database: "",
		tables: [],
		allDatabases: false,
		schemaOnly: false,
		dataOnly: false,
		includeRoutines: true,
		includeTriggers: true,
		includeEvents: false,
		singleTransaction: true,
		addDropStatements: false,
		whereClause: "",
	});

	const scopeOptions = $derived([
		{ value: globalScope, label: "Global connection" },
		...databases.map((database) => ({ value: database, label: database })),
	]);
	const canRun = $derived(sqlContent.trim() && credentials.host.trim() && credentials.username.trim());

	onMount(() => {
		void initializeBackupOutputDir();

		if (!credentialsReady) return;
		const loadTimer = window.setTimeout(() => {
			void validateAndLoadDatabases(false);
		}, 120);

		return () => window.clearTimeout(loadTimer);
	});

	$effect(() => {
		const database = backupDatabaseName.trim();
		const canLoadTables = credentialsReady && backupMode === "tables" && database;
		JSON.stringify(credentials);
		backupTableRequestId += 1;

		if (!canLoadTables) {
			backupTables = [];
			selectedBackupTable = "";
			return;
		}

		untrack(() => void refreshBackupTables(database));
	});

	async function initializeBackupOutputDir() {
		if (backupOptions.outputDir.trim()) return;

		const outputDir = await getDefaultMariaDBBackupOutputDir();
		if (outputDir && !backupOptions.outputDir.trim()) {
			backupOptions.outputDir = outputDir;
		}
	}

	async function browseSqlFile() {
		error = "";
		const selected = await chooseSqlFile(sqlPath || undefined);
		if (!selected) return;
		sqlPath = selected;
		sqlContent = await readTextFile(selected);
		message = `Loaded ${selected}.`;
	}

	async function validateAndLoadDatabases(showLoadedMessage = true) {
		if (busy || !active) return;
		const original = { ...credentials };
		const revision = databaseSession.revision;
		busy = true;
		error = "";
		message = "";
		connectionError = "";
		try {
			await validateMariaDBCredentials(original);
			if (!active || !rememberDatabaseCredentials(original, revision)) return;
			credentialsReady = true;
			databases = await listMariaDBDatabases(original);
			if (!active) return;
			selectedScope = credentials.database && databases.includes(credentials.database) ? credentials.database : selectedScope;
			queryDatabase = credentials.database && databases.includes(credentials.database) ? credentials.database : queryDatabase;
			backupDatabaseName ||= credentials.database && databases.includes(credentials.database) ? credentials.database : databases[0] || "";
			if (showLoadedMessage) message = `Loaded ${databases.length} database${databases.length === 1 ? "" : "s"}.`;
		} catch (caught) {
			credentialsReady = false;
			connectionError = caught instanceof Error ? caught.message : String(caught);
			error = connectionError;
		} finally {
			busy = false;
		}
	}

	async function runSql() {
		if (!canRun) return;
		busy = true;
		error = "";
		message = "";
		result = null;
		try {
			const scopedCredentials = {
				...credentials,
				database: selectedScope === globalScope ? null : selectedScope,
			};
			result = await executeMariaDBQuery(scopedCredentials, sqlContent);
			message = result.success ? "SQL file executed." : "SQL file returned an error.";
		} catch (caught) {
			error = caught instanceof Error ? caught.message : String(caught);
		} finally {
			busy = false;
		}
	}

	async function executeQuery() {
		if (!credentialsReady) {
			error = "Validate MariaDB credentials before running queries.";
			return;
		}

		busy = true;
		error = "";
		message = "";
		try {
			queryResult = await executeMariaDBQuery(
				{
					...credentials,
					database: queryDatabase === globalScope ? null : queryDatabase,
				},
				query,
			);
			message = queryResult.success ? "Query executed." : "Query returned an error.";
		} catch (caught) {
			error = caught instanceof Error ? caught.message : String(caught);
		} finally {
			busy = false;
		}
	}

	async function refreshBackupTables(database: string) {
		const requestId = ++backupTableRequestId;

		try {
			const tables = await listMariaDBTables({ ...credentials }, database);
			if (!active || requestId !== backupTableRequestId || backupDatabaseName.trim() !== database) return;

			backupTables = tables;
			backupOptions.tables = backupOptions.tables.filter((table) => tables.includes(table));
			if (selectedBackupTable && !tables.includes(selectedBackupTable)) selectedBackupTable = "";
		} catch (caught) {
			if (!active || requestId !== backupTableRequestId) return;
			error = caught instanceof Error ? caught.message : String(caught);
			backupTables = [];
		}
	}

	async function backupDatabase() {
		if (!credentialsReady) {
			error = "Validate MariaDB credentials before creating backups.";
			return;
		}

		if (backupMode === "tables" && backupOptions.tables.length === 0) {
			error = "Choose at least one table to back up.";
			return;
		}

		busy = true;
		error = "";
		message = "";
		try {
			const backup = await backupMariaDB(credentials, {
				...backupOptions,
				allDatabases: backupMode === "all",
				database: backupMode === "all" ? null : backupDatabaseName.trim() || credentials.database || null,
				tables: backupMode === "tables" ? backupOptions.tables : [],
				fileName: backupOptions.fileName?.trim() || null,
				whereClause: backupOptions.whereClause?.trim() || null,
			});
			message = `Backup created: ${backup.path}`;
		} catch (caught) {
			error = caught instanceof Error ? caught.message : String(caught);
		} finally {
			busy = false;
		}
	}
</script>

<section class="space-y-6">
	<div>
		<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">MariaDB</p>
		<h1 class="mt-2 text-3xl font-semibold tracking-normal text-foreground">Queries & Files</h1>
		<p class="mt-2 max-w-2xl text-sm leading-6 text-muted-foreground">Run SQL files, create backups, and use the query console from one database-focused workspace.</p>
	</div>

	{#if message}<Notice tone={result?.success === false ? "warn" : "success"} {message} onDismiss={() => (message = "")} />{/if}
	{#if error}<Notice tone="error" message={error} onDismiss={() => (error = "")} />{/if}
	{#if !backupWarningDismissed}
		<Notice
			tone="warn"
			title="Back up before changing MariaDB"
			message="Before running SQL files, ad hoc queries, or restore/import scripts, create a fresh backup of any databases you care about."
			onDismiss={() => (backupWarningDismissed = true)}
			class="px-4 py-3 text-sm"
		/>
	{/if}

	<div class="grid gap-4 xl:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)]">
		<ConnectionCard bind:credentials {busy} {credentialsReady} {connectionError} onApply={() => validateAndLoadDatabases()} />

		<Card.Root class="rounded-md border-border bg-card shadow-sm">
			<Card.Header class="border-b border-border pb-4">
				<div class="flex items-center gap-3">
					<div class="flex size-9 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
						<DatabaseIcon class="size-4" />
					</div>
					<div>
						<Card.Title>SQL File</Card.Title>
						<Card.Description>Review the file contents before running it.</Card.Description>
					</div>
				</div>
			</Card.Header>
			<Card.Content class="space-y-4">
				<label class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">Execution Scope</span>
					<Select.Root bind:value={selectedScope} type="single" items={scopeOptions}>
						<Select.Trigger class="rounded-sm font-mono text-xs" title="Choose SQL execution scope">
							{selectedScope === globalScope ? "Global connection" : selectedScope}
						</Select.Trigger>
						<Select.Content class="rounded-sm">
							{#each scopeOptions as option}
								<Select.Item value={option.value} label={option.label}>{option.label}</Select.Item>
							{/each}
						</Select.Content>
					</Select.Root>
					<span class="text-xs leading-5 text-muted-foreground">Use a global connection for files that create databases, or choose a database for resource migrations.</span>
				</label>
				<div class="grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto]">
					<Input bind:value={sqlPath} readonly placeholder="Choose a .sql file..." class="rounded-sm font-mono text-xs" />
					<Button variant="outline" onclick={browseSqlFile} disabled={busy} title="Browse for a .sql file">
						<FolderOpenIcon />
						Browse
					</Button>
				</div>
				<textarea bind:value={sqlContent} spellcheck="false" class="h-40 min-h-32 w-full resize-y rounded-sm border border-input bg-background px-3 py-3 font-mono text-xs leading-5 outline-none focus-visible:ring-3 focus-visible:ring-ring/50" placeholder="SQL file contents will appear here."></textarea>
				<Button onclick={runSql} disabled={busy || !canRun} title="Run SQL file">
					<PlayIcon />
					Run SQL
				</Button>
			</Card.Content>
		</Card.Root>
	</div>

	{#if result}
		<pre class={["max-h-80 overflow-auto rounded-sm border p-4 font-mono text-xs leading-6 whitespace-pre-wrap", result.success ? "border-border bg-card text-foreground" : "border-destructive/30 bg-destructive/10 text-destructive"]}>{result.stdout || result.stderr || "No output."}</pre>
	{/if}

	<BackupCard
		bind:backupOptions
		bind:backupMode
		bind:backupDatabase={backupDatabaseName}
		bind:selectedTable={selectedBackupTable}
		{busy}
		canBackup={credentialsReady}
		{databases}
		tables={backupTables}
		onBackup={backupDatabase}
	/>

	<QueryConsole bind:query bind:selectedDatabase={queryDatabase} {busy} canExecute={credentialsReady} {databases} result={queryResult} onExecute={executeQuery} />
</section>
