<script lang="ts">
	import ArchiveIcon from "@lucide/svelte/icons/archive";
	import DatabaseIcon from "@lucide/svelte/icons/database";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Checkbox } from "$lib/components/ui/checkbox/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { chooseFolder } from "$lib/core/selectFolder";
	import type { MariaDBBackupOptions } from "$lib/modules/mariadb";

	let {
		backupOptions = $bindable(),
		busy,
		canBackup,
		onBackup,
	}: {
		backupOptions: MariaDBBackupOptions;
		busy: boolean;
		canBackup: boolean;
		onBackup: () => void;
	} = $props();

	const backupModes = [
		{ value: "database", label: "Database" },
		{ value: "tables", label: "Tables" },
		{ value: "all", label: "All DBs" },
	] as const;

	const tableList = $derived(backupOptions.tables.join(", "));
	const mode = $derived(backupOptions.allDatabases ? "all" : backupOptions.tables.length ? "tables" : "database");

	async function pickOutputFolder() {
		const selected = await chooseFolder();
		if (!selected) return;
		backupOptions.outputDir = selected;
	}

	function setMode(nextMode: "database" | "tables" | "all") {
		backupOptions.allDatabases = nextMode === "all";
		if (nextMode !== "tables") {
			backupOptions.tables = [];
			backupOptions.whereClause = "";
		}
	}

	function updateTables(event: Event) {
		backupOptions.tables = (event.currentTarget as HTMLInputElement).value
			.split(",")
			.map((table) => table.trim())
			.filter(Boolean);
	}
</script>

<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-transform duration-300 hover:-translate-y-0.5">
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
				<Input bind:value={backupOptions.outputDir} placeholder="C:\Backups\MariaDB" title="Folder where the SQL backup should be written." class="rounded-sm font-mono text-xs" />
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
			<label class="grid gap-2">
				<span class="text-xs font-medium text-muted-foreground">Database</span>
				<Input bind:value={backupOptions.database} disabled={backupOptions.allDatabases} placeholder="fxserver" title="Database to back up when all-databases is off." class="rounded-sm font-mono text-xs" />
			</label>
		</div>

		<div class="flex flex-wrap gap-2">
			{#each backupModes as item}
				<Button variant={mode === item.value ? "default" : "outline"} size="sm" class="rounded-sm" onclick={() => setMode(item.value)} disabled={busy}>
					{item.label}
				</Button>
			{/each}
		</div>

		{#if mode === "tables"}
			<div class="grid gap-3 md:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
				<label class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">Tables</span>
					<Input value={tableList} oninput={updateTables} placeholder="players, owned_vehicles" title="Comma-separated table names to back up." class="rounded-sm font-mono text-xs" />
				</label>
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
