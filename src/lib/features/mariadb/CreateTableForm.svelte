<script lang="ts">
	import PlusIcon from "@lucide/svelte/icons/plus";
	import XIcon from "@lucide/svelte/icons/x";
	import EyeIcon from "@lucide/svelte/icons/eye";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Checkbox } from "$lib/components/ui/checkbox/index.js";
	import DatabaseAdminSelect from "./DatabaseAdminSelect.svelte";
	import { columnTypes, newColumn, primaryColumn, type ColumnSpec } from "$lib/modules/databaseAdmin";

	type Props = {
		busy: boolean;
		onReview: (name: string, columns: ColumnSpec[]) => void;
		onClose: () => void;
	};
	let { busy, onReview, onClose }: Props = $props();
	let name = $state("");
	let columns = $state<ColumnSpec[]>([primaryColumn(), newColumn("name")]);
	const types = columnTypes.map((value) => ({ value, label: value }));
	const defaults = [
		{ value: "none", label: "No default" },
		{ value: "null", label: "NULL" },
		{ value: "value", label: "Literal value" },
		{ value: "currentTimestamp", label: "CURRENT_TIMESTAMP" },
	];

	function changeType(column: ColumnSpec, type: string) {
		column.length = type === "VARCHAR" || type === "CHAR" ? "255" : type === "DECIMAL" ? "10,0" : null;
		column.autoIncrement = false;
		column.unsigned = false;
		column.defaultKind = "none";
		column.defaultValue = null;
	}

	function review() {
		onReview(name, columns.map((column) => ({
			...column,
			defaultValue: column.defaultKind === "value" ? column.defaultValue ?? "" : null,
		})));
	}
</script>

<section class="space-y-4 border-y border-border py-4" aria-label="Create table">
	<header class="flex items-center justify-between gap-2">
		<h3 class="text-sm font-semibold">Create Table</h3>
		<Button
			size="icon-sm" variant="ghost" title="Close create table"
			aria-label="Close create table" disabled={busy} onclick={onClose}
		>
			<XIcon />
		</Button>
	</header>
	<label class="grid max-w-md gap-2 text-xs">
		Table name
		<Input bind:value={name} maxlength={64} disabled={busy} />
	</label>
	<p class="text-xs text-muted-foreground">InnoDB / utf8mb4_unicode_ci</p>
	{#each columns as column, i}
		<fieldset class="min-w-0 space-y-3 border-t border-border pt-3" disabled={busy}>
			<legend class="px-1 text-xs text-muted-foreground">Column {i + 1}</legend>
			<div class="grid items-end gap-2 sm:grid-cols-[minmax(0,1fr)_9rem_7rem_auto]">
				<label class="grid min-w-0 gap-1 text-xs">
					Name
					<Input aria-label={`Column ${i + 1} name`} bind:value={column.name} maxlength={64} />
				</label>
				<div class="grid min-w-0 gap-1 text-xs">
					<span>Type</span>
					<DatabaseAdminSelect
						label={`Column ${i + 1} type`} options={types}
						bind:value={column.dataType} disabled={busy}
						onChange={(value) => changeType(column, value)}
					/>
				</div>
				<label class="grid min-w-0 gap-1 text-xs">
					Length / scale
					<Input
						aria-label={`Column ${i + 1} length`} value={column.length ?? ""}
						oninput={(event) => column.length = event.currentTarget.value}
						disabled={!["CHAR", "VARCHAR", "DECIMAL"].includes(column.dataType)} maxlength={8}
					/>
				</label>
				<Button
					size="icon-sm" variant="ghost" title="Remove column"
					aria-label={`Remove column ${i + 1}`} disabled={busy || columns.length === 1}
					onclick={() => columns = columns.filter((_, index) => i !== index)}
				>
					<XIcon />
				</Button>
			</div>
			<div class="flex flex-wrap items-center gap-x-5 gap-y-3 text-xs">
				<label class="flex items-center gap-2">
					<Checkbox
						bind:checked={column.primary}
						onCheckedChange={(value) => {
							if (value) column.nullable = false;
							else column.autoIncrement = false;
						}}
					/>
					Primary key
				</label>
				<label class="flex items-center gap-2">
					<Checkbox bind:checked={column.unique} />
					Unique
				</label>
				<label class="flex items-center gap-2">
					<Checkbox bind:checked={column.nullable} disabled={column.primary} />
					Nullable
				</label>
				<label class="flex items-center gap-2">
					<Checkbox
						bind:checked={column.unsigned}
						disabled={!["TINYINT", "SMALLINT", "INT", "BIGINT", "DECIMAL", "BOOLEAN"].includes(column.dataType)}
					/>
					Unsigned
				</label>
				<label class="flex items-center gap-2">
					<Checkbox
						bind:checked={column.autoIncrement}
						disabled={!column.primary || !["TINYINT", "SMALLINT", "INT", "BIGINT"].includes(column.dataType)}
						onCheckedChange={() => { column.defaultKind = "none"; column.defaultValue = null; }}
					/>
					Auto increment
				</label>
			</div>
			<div class="grid max-w-xl gap-2 sm:grid-cols-2">
				<DatabaseAdminSelect
					label={`Column ${i + 1} default`} options={defaults}
					bind:value={column.defaultKind} disabled={busy || column.autoIncrement}
				/>
				{#if column.defaultKind === "value"}
					<Input
						aria-label={`Column ${i + 1} default value`} value={column.defaultValue ?? ""}
						oninput={(event) => column.defaultValue = event.currentTarget.value} maxlength={512}
					/>
				{/if}
			</div>
		</fieldset>
	{/each}
	<div class="flex flex-wrap gap-2">
		<Button
			size="sm" variant="outline" disabled={busy || columns.length >= 32}
			onclick={() => columns = [...columns, newColumn()]}
		>
			<PlusIcon />Add Column
		</Button>
		<Button size="sm" disabled={busy || !name || columns.some((column) => !column.name)} onclick={review}>
			<EyeIcon />Review Create Table
		</Button>
	</div>
</section>
