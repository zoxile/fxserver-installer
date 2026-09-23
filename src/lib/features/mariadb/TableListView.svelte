<script lang="ts">
	import { onMount, onDestroy } from "svelte";
	import PlusIcon from "@lucide/svelte/icons/plus";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import EyeIcon from "@lucide/svelte/icons/eye";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import DatabaseAdminSelect from "./DatabaseAdminSelect.svelte";
	import CreateTableForm from "./CreateTableForm.svelte";
	import DangerConfirm from "./DangerConfirm.svelte";
	import {
		listAdminTables, previewAdminAction, applyAdminAction, isSystemDatabase,
		type AdminAction, type AdminRequest, type AdminPreview, type TableInfoPage, type AdminResult,
	} from "$lib/modules/databaseAdmin";
	import type { MariaDBCredentials } from "$lib/modules/mariadb";
	import { databaseSession, isDatabaseSessionValidated } from "$lib/core/databaseSession.svelte";
	import { getDatabaseBrowserCache } from "$lib/core/databaseBrowserCache";

	type Props = {
		credentials: MariaDBCredentials;
		database: string;
		workspaceId: string;
		blocked: boolean;
		onBusy: (busy: boolean) => void;
		onChanged: (result: AdminResult) => Promise<void>;
		onOpen: (table: string) => void;
	};
	let { credentials, database, workspaceId, blocked, onBusy, onChanged, onOpen }: Props = $props();
	let data = $state.raw<TableInfoPage>({ tables: [], hasMore: false });
	let filter = $state("");
	let selected = $state("");
	let action = $state<AdminAction>("check");
	let create = $state<"database" | "table" | null>(null);
	let databaseName = $state("");
	let preview = $state<AdminPreview | null>(null);
	let error = $state("");
	let busy = $state(false);
	let active = true;
	const protectedDatabase = $derived(isSystemDatabase(database));
	const rows = $derived(data.tables.filter((table) => table.name.toLowerCase().includes(filter.toLowerCase())));
	const target = $derived(data.tables.find((table) => table.name === selected));
	const canRepair = $derived(["MyISAM", "Aria", "ARCHIVE", "CSV"].includes(target?.engine ?? ""));
	const actions = [
		{ value: "check", label: "Check" },
		{ value: "analyze", label: "Analyze" },
		{ value: "optimize", label: "Optimize" },
		{ value: "repair", label: "Repair" },
		{ value: "empty", label: "Empty" },
		{ value: "drop", label: "Drop" },
	];

	onMount(() => { void load(); });
	onDestroy(() => { active = false; });

	function setBusy(value: boolean) {
		busy = value;
		onBusy(value);
	}

	async function load(force = false) {
		if (busy) return;
		busy = true;
		error = "";
		preview = null;
		try {
			if (!isDatabaseSessionValidated(credentials)) return;
			const cache = getDatabaseBrowserCache(databaseSession.validated!);
			if (force) cache.clear();
			const original = { ...credentials }; const selectedDatabase = database;
			const next = database
				? await cache.read(["statistics", database], () => listAdminTables(original, selectedDatabase))
				: { tables: [], hasMore: false };
			if (active) {
				data = next;
				selected = "";
			}
		} catch (caught) {
			if (active) error = String(caught);
		} finally {
			if (active) busy = false;
		}
	}

	async function review(request: Omit<AdminRequest, "workspaceId">) {
		if (busy || blocked || !active) return;
		setBusy(true);
		error = "";
		preview = null;
		try {
			const next = await previewAdminAction({ ...credentials }, { ...request, workspaceId });
			if (active) preview = next;
		} catch (caught) {
			if (active) error = String(caught);
		} finally {
			if (active) setBusy(false);
		}
	}

	async function apply(confirmation: string) {
		if (!preview || busy || blocked || confirmation !== preview.confirmation) return;
		const token = preview.token;
		setBusy(true);
		error = "";
		try {
			const result = await applyAdminAction(workspaceId, token, confirmation);
			if (active) {
				preview = null;
				create = null;
				setBusy(false);
				await onChanged(result);
			}
		} catch (caught) {
			if (active) {
				preview = null;
				error = String(caught);
			}
		} finally {
			if (active) setBusy(false);
		}
	}

	function bytes(value: number) {
		if (value < 1024) return `${value} B`;
		if (value < 1048576) return `${(value / 1024).toFixed(1)} KiB`;
		return `${(value / 1048576).toFixed(1)} MiB`;
	}
</script>

<section class="min-w-0 space-y-4" aria-label="Table administration" aria-busy={busy}>
	<fieldset class="min-w-0 space-y-4" disabled={blocked}>
		<header class="flex flex-wrap items-center justify-between gap-3">
			<h2 class="text-base font-semibold">Tables</h2>
			<div class="flex flex-wrap gap-2">
				<Button
					size="sm" variant="outline" disabled={busy}
					onclick={() => { create = create === "database" ? null : "database"; preview = null; }}
				>
					<PlusIcon />Create Database
				</Button>
				<Button
					size="sm" variant="outline" disabled={busy || !database || protectedDatabase}
					onclick={() => { create = create === "table" ? null : "table"; preview = null; }}
				>
					<PlusIcon />Create Table
				</Button>
				<Button
					size="icon-sm" variant="outline" title="Refresh table statistics"
					aria-label="Refresh table statistics" disabled={busy} onclick={() => load(true)}
				>
					<RefreshCwIcon />
				</Button>
			</div>
		</header>

		{#if error}
			<Notice tone="error" message={error} onDismiss={() => error = ""} />
		{/if}
		{#if preview}
			{#key preview.token}
				<DangerConfirm {preview} {busy} onConfirm={apply} onCancel={() => preview = null} />
			{/key}
		{:else if create === "table"}
			<CreateTableForm
				{busy} onClose={() => create = null}
				onReview={(name, columns) => review({ database, table: name, action: "createTable", columns })}
			/>
		{:else if create === "database"}
			<div class="flex flex-wrap items-end gap-3 border-y border-border py-4">
				<label class="grid min-w-0 flex-1 gap-2 text-xs">
					New database name
					<Input bind:value={databaseName} disabled={busy} maxlength={64} />
				</label>
				<Button
					disabled={busy || !databaseName || isSystemDatabase(databaseName)}
					onclick={() => review({ database: databaseName, table: null, action: "createDatabase", columns: [] })}
				>
					<EyeIcon />Review Create Database
				</Button>
			</div>
		{/if}

		{#if protectedDatabase}
			<p class="text-xs text-muted-foreground">System database: administration is disabled.</p>
		{/if}
		<Input
			class="max-w-sm" aria-label="Filter table statistics"
			placeholder="Filter tables" bind:value={filter} maxlength={128}
		/>
		{#if data.hasMore}
			<p class="text-xs text-amber-500">First 500 tables only. Additional tables are not included.</p>
		{/if}

		<div class="max-h-[28rem] overflow-auto border-y border-border">
			<table class="w-full text-left text-xs">
				<thead class="sticky top-0 bg-background">
					<tr>
						{#each ["Table", "Kind", "Engine", "Estimated rows", "Data", "Indexes", "Free", "Collation"] as heading}
							<th class="whitespace-nowrap border-b border-border px-3 py-2">{heading}</th>
						{/each}
					</tr>
				</thead>
				<tbody>
					{#each rows as table}
						<tr class="border-b border-border/50" class:bg-muted={selected === table.name}>
							<td class="max-w-72 px-3 py-2">
								<button
									class="max-w-full truncate text-left font-mono underline-offset-4 hover:underline"
									disabled={busy || !!preview}
									onclick={() => { selected = table.name; create = null; }}
								>
									{table.name}
								</button>
							</td>
							<td class="whitespace-nowrap px-3 py-2">{table.kind}</td>
							<td class="px-3 py-2">{table.engine ?? "-"}</td>
							<td class="px-3 py-2">{table.rows?.toLocaleString() ?? "-"}</td>
							<td class="whitespace-nowrap px-3 py-2">{bytes(table.dataBytes)}</td>
							<td class="whitespace-nowrap px-3 py-2">{bytes(table.indexBytes)}</td>
							<td class="whitespace-nowrap px-3 py-2">{bytes(table.freeBytes)}</td>
							<td class="px-3 py-2">{table.collation ?? "-"}</td>
						</tr>
					{:else}
						<tr>
							<td colspan="8" class="p-6 text-center text-muted-foreground">
								{busy ? "Loading tables..." : "No visible tables."}
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>

		{#if selected && !preview}
			<div class="flex flex-wrap items-center gap-3">
				<span class="min-w-0 break-all font-mono text-xs">{database}.{selected}</span>
				<Button
					variant="outline" size="sm" disabled={busy || target?.kind !== "BASE TABLE"}
					onclick={() => onOpen(selected)}
				>
					<EyeIcon />Browse Rows
				</Button>
				<div class="w-36">
					<DatabaseAdminSelect
						label="Table action" options={actions} value={action}
						onChange={(value) => action = value as AdminAction}
						disabled={busy || protectedDatabase}
					/>
				</div>
				<Button
					variant={action === "empty" || action === "drop" ? "destructive" : "outline"}
					size="sm"
					disabled={busy || protectedDatabase || target?.kind !== "BASE TABLE" || (action === "repair" && !canRepair)}
					onclick={() => review({ database, table: selected, action, columns: [] })}
				>
					<EyeIcon />Review Action
				</Button>
			</div>
		{/if}
	</fieldset>
</section>
