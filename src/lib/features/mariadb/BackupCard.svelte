<script lang="ts">
	import ArchiveIcon from "@lucide/svelte/icons/archive";
	import DatabaseIcon from "@lucide/svelte/icons/database";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import PlusIcon from "@lucide/svelte/icons/plus";
	import XIcon from "@lucide/svelte/icons/x";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Checkbox } from "$lib/components/ui/checkbox/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import * as Select from "$lib/components/ui/select/index.js";
	import { chooseFolder } from "$lib/core/selectFolder";
	import type { MariaDBBackupOptions } from "$lib/modules/mariadb";

	type BackupMode = "database" | "tables" | "all";

	let {
		backupOptions = $bindable(),
		backupMode = $bindable(),
		backupDatabase = $bindable(),
		selectedTable = $bindable(),
		busy,
		canBackup,
		databases,
		tables,
		onBackup,
	}: {
		backupOptions: MariaDBBackupOptions;
		backupMode: BackupMode;
		backupDatabase: string;
		selectedTable: string;
		busy: boolean;
		canBackup: boolean;
		databases: string[];
		tables: string[];
		onBackup: () => void;
	} = $props();

	const backupModes = [
		{ value: "database", label: "Database" },
		{ value: "tables", label: "Tables" },
		{ value: "all", label: "All DBs" },
	] as const;

	const databaseOptions = $derived(databases.map((database) => ({ value: database, label: database })));
	const tableOptions = $derived(tables.map((table) => ({ value: table, label: table })));
	async function pickOutputFolder() {
		const selected = await chooseFolder(backupOptions.outputDir);
		if (!selected) return;
		backupOptions.outputDir = selected;
	}

	function setMode(nextMode: "database" | "tables" | "all") {
		backupMode = nextMode;
		backupOptions.allDatabases = nextMode === "all";
		if (nextMode !== "tables") {
			backupOptions.tables = [];
			backupOptions.whereClause = "";
			selectedTable = "";
		}
	}

	function addSelectedTable() {
		if (!selectedTable || backupOptions.tables.includes(selectedTable)) return;
		backupOptions.tables = [...backupOptions.tables, selectedTable];
	}

	function removeTable(table: string) {
		backupOptions.tables = backupOptions.tables.filter((selected) => selected !== table);
		if (backupOptions.tables.length !== 1) {
			backupOptions.whereClause = "";
		}
	}
</script>

<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-shadow duration-500 ease-[cubic-bezier(0.22,1,0.36,1)]">
	<div class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"></div>
	<Card.Header class="border-b border-border pb-4">
		<div class="flex items-center justify-between gap-3">
			<div class="flex min-w-0 items-center gap-3">
				<div class="flex size-9 shrink-0 items-center justify-center rounded-sm border border-emerald-400/30 bg-emerald-400/10 text-emerald-200">
					<ArchiveIcon class="size-4" />
				</div>
				<div class="min-w-0">
					<Card.Title>Backups</Card.Title>
					<Card.Description>Create SQL dumps for a whole server, one database, selected tables, or filtered table data.</Card.Description>
				</div>
			</div>
			<Button onclick={onBackup} disabled={busy || !canBackup} title={canBackup ? "Create MariaDB backup" : "Apply valid admin credentials before backing up"}>
				<DatabaseIcon />
				Backup
			</Button>
		</div>
	</Card.Header>
	<Card.Content class="space-y-4">
		<div class="grid gap-3 md:grid-cols-[minmax(0,1fr)_auto]">
			<label class="grid gap-2">
				<span class="text-xs font-medium text-muted-foreground">Output Folder</span>
				<Input bind:value={backupOptions.outputDir} placeholder="Downloads" title="Folder where the SQL backup should be written." class="rounded-sm font-mono text-xs" />
			</label>
			<div class="flex items-end">
				<Button variant="outline" onclick={pickOutputFolder} disabled={busy} title="Choose backup output folder">
					<FolderOpenIcon />
					Browse
				</Button>
			</div>
		</div>

		<div class="grid gap-3 md:grid-cols-[minmax(0,0.75fr)_minmax(0,1fr)]">
			<label class="grid gap-2">
				<span class="text-xs font-medium text-muted-foreground">File Name</span>
				<Input bind:value={backupOptions.fileName} placeholder="Auto-generated .sql name" title="Optional backup file name. .sql is added automatically." class="rounded-sm font-mono text-xs" />
			</label>
			<div class="grid gap-2">
				<span class="text-xs font-medium text-muted-foreground">Database</span>
				<Select.Root bind:value={backupDatabase} type="single" items={databaseOptions} disabled={backupOptions.allDatabases || busy}>
					<Select.Trigger title="Choose the database to back up" class="w-full rounded-sm font-mono text-xs">
						{backupDatabase || "Choose database"}
					</Select.Trigger>
					<Select.Content class="rounded-sm">
						{#if databaseOptions.length}
							{#each databaseOptions as option}
								<Select.Item value={option.value} label={option.label}>
									{option.label}
								</Select.Item>
							{/each}
						{:else}
							<Select.Item value="" label="No databases loaded" disabled>No databases loaded</Select.Item>
						{/if}
					</Select.Content>
				</Select.Root>
			</div>
		</div>

		<div class="flex flex-wrap gap-2">
			{#each backupModes as item}
				<Button variant={backupMode === item.value ? "default" : "outline"} size="sm" class="rounded-sm" onclick={() => setMode(item.value)} disabled={busy}>
					{item.label}
				</Button>
			{/each}
		</div>

		{#if backupMode === "tables"}
			<div class="grid gap-3 md:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
				<div class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">Table</span>
					<div class="grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto]">
						<Select.Root bind:value={selectedTable} type="single" items={tableOptions} disabled={busy || !backupDatabase || !tables.length}>
							<Select.Trigger title="Choose a table to include in the backup" class="w-full rounded-sm font-mono text-xs">
								{selectedTable || (backupDatabase ? "Choose table" : "Choose database first")}
							</Select.Trigger>
							<Select.Content class="rounded-sm">
								{#if tableOptions.length}
									{#each tableOptions as option}
										<Select.Item value={option.value} label={option.label}>
											{option.label}
										</Select.Item>
									{/each}
								{:else}
									<Select.Item value="" label="No tables loaded" disabled>No tables loaded</Select.Item>
								{/if}
							</Select.Content>
						</Select.Root>
						<Button variant="outline" onclick={addSelectedTable} disabled={busy || !selectedTable || backupOptions.tables.includes(selectedTable)} title="Add selected table to this backup">
							<PlusIcon />
							Add
						</Button>
					</div>
					<div class="flex min-h-9 flex-wrap gap-2 rounded-sm border border-border bg-background/70 p-2">
						{#if backupOptions.tables.length}
							{#each backupOptions.tables as table}
								<button
									type="button"
									class="inline-flex items-center gap-1 rounded-xs border border-border bg-card px-2 py-1 font-mono text-xs text-foreground transition-colors hover:border-destructive/50 hover:text-destructive"
									onclick={() => removeTable(table)}
									disabled={busy}
									title={`Remove ${table}`}
								>
									{table}
									<XIcon class="size-3" />
								</button>
							{/each}
						{:else}
							<span class="px-1 py-1 text-xs text-muted-foreground">No tables selected.</span>
						{/if}
					</div>
				</div>
				<label class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">WHERE Filter</span>
					<Input bind:value={backupOptions.whereClause} disabled={backupOptions.tables.length !== 1} placeholder="id >= 1000" title="Optional WHERE clause for exactly one selected table." class="rounded-sm font-mono text-xs" />
				</label>
			</div>
		{/if}

		<div class="grid gap-2 sm:grid-cols-2 xl:grid-cols-3">
			<label class="flex items-center gap-2 rounded-sm border border-border bg-background/70 px-3 py-2 text-xs text-muted-foreground">
				<Checkbox bind:checked={backupOptions.schemaOnly} />
				Schema only
			</label>
			<label class="flex items-center gap-2 rounded-sm border border-border bg-background/70 px-3 py-2 text-xs text-muted-foreground">
				<Checkbox bind:checked={backupOptions.dataOnly} />
				Data only
			</label>
			<label class="flex items-center gap-2 rounded-sm border border-border bg-background/70 px-3 py-2 text-xs text-muted-foreground">
				<Checkbox bind:checked={backupOptions.singleTransaction} />
				Single transaction
			</label>
			<label class="flex items-center gap-2 rounded-sm border border-border bg-background/70 px-3 py-2 text-xs text-muted-foreground">
				<Checkbox bind:checked={backupOptions.includeRoutines} />
				Routines
			</label>
			<label class="flex items-center gap-2 rounded-sm border border-border bg-background/70 px-3 py-2 text-xs text-muted-foreground">
				<Checkbox bind:checked={backupOptions.includeTriggers} />
				Triggers
			</label>
			<label class="flex items-center gap-2 rounded-sm border border-border bg-background/70 px-3 py-2 text-xs text-muted-foreground">
				<Checkbox bind:checked={backupOptions.includeEvents} />
				Events
			</label>
			<label class="flex items-center gap-2 rounded-sm border border-border bg-background/70 px-3 py-2 text-xs text-muted-foreground sm:col-span-2 xl:col-span-3">
				<Checkbox bind:checked={backupOptions.addDropStatements} />
				Add DROP statements before recreated databases and tables
			</label>
		</div>
	</Card.Content>
</Card.Root>
