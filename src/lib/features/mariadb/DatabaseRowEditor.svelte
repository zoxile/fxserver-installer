<script lang="ts">
	import { onDestroy } from "svelte";
	import CheckIcon from "@lucide/svelte/icons/check";
	import EyeIcon from "@lucide/svelte/icons/eye";
	import XIcon from "@lucide/svelte/icons/x";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Checkbox } from "$lib/components/ui/checkbox/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import type { MariaDBCredentials } from "$lib/modules/mariadb";
	import { applyBrowserChange, previewBrowserChange, type BrowserMetadata, type ChangeKind, type ChangePreview, type ColumnInput } from "$lib/modules/databaseBrowser";
	type Props = { metadata: BrowserMetadata; kind: ChangeKind; original: (string | null)[] | null; database: string; table: string; workspaceId: string; credentials: MariaDBCredentials; onClose: () => void; onBusy: (busy: boolean) => void; onApplied: () => Promise<void> };
	let { metadata, kind, original, database, table, workspaceId, credentials, onClose, onBusy, onApplied }: Props = $props();
	function initialFields() { return metadata.columns.map((column, index) => ({ column, included: kind === "update" || !column.extra.includes("auto_increment"), isNull: original ? original[index] === null : column.nullable, value: original?.[index] ?? "" })); }
	let fields = $state(initialFields());
	let preview = $state<ChangePreview | null>(null);
	let confirmation = $state("");
	let error = $state("");
	let busy = $state(false);
	let active = true;
	onDestroy(() => { active = false; });
	function numeric(type: string) { return /^(tinyint|smallint|mediumint|int|integer|bigint|decimal|numeric|float|double)(\(|\s|$)/.test(type); }
	async function review() {
		if (busy || !active) return;
		busy = true; onBusy(true); error = "";
		try {
			const values: ColumnInput[] = kind === "delete" ? [] : fields.flatMap((field, index) => {
				if (!field.included || (kind === "update" && (field.isNull ? null : field.value) === original?.[index])) return [];
				return [{ column: field.column.name, value: field.isNull ? { kind: "null" as const } : { kind: numeric(field.column.columnType) ? "number" as const : "text" as const, value: field.value } }];
			});
			const result = await previewBrowserChange({ ...credentials }, { workspaceId, database, table, kind, values, original: original ? [...original] : null });
			if (active) { preview = result; confirmation = ""; }
		} catch (caught) { if (active) error = String(caught); }
		finally { busy = false; if (active) onBusy(false); }
	}
	async function apply() {
		if (busy || !active || !preview || confirmation !== preview.confirmation) return;
		const selected = preview; busy = true; onBusy(true); error = "";
		try {
			await applyBrowserChange(workspaceId, selected.token, confirmation);
			if (active) { onBusy(false); await onApplied(); }
		} catch (caught) { if (active) { preview = null; error = String(caught); } }
		finally { busy = false; if (active) onBusy(false); }
	}
</script>

<section class="space-y-4 border-y border-amber-500/40 py-5" aria-label="Single-row editor">
	<header class="flex items-center justify-between gap-3"><h2 class="text-base font-semibold capitalize">{kind} Row</h2><Button variant="ghost" size="icon-sm" title="Close row editor" aria-label="Close row editor" disabled={busy} onclick={onClose}><XIcon /></Button></header>
	{#if error}<Notice tone="error" message={error} onDismiss={() => error = ""} />{/if}
	{#if preview}
		<h3 class="text-sm font-semibold">Pending Change</h3>
		<p class="wrap-anywhere text-xs text-muted-foreground">{preview.host}:{preview.port} / {preview.confirmation} / Expires {new Date(preview.expiresAt).toLocaleTimeString()}</p>
		<pre class="max-h-56 overflow-auto border border-border bg-muted/30 p-3 text-xs whitespace-pre-wrap wrap-anywhere">{preview.sql}</pre>
		{#if preview.parameters.length}<dl class="grid gap-2 text-xs sm:grid-cols-2">{#each preview.parameters as parameter}<div class="min-w-0"><dt class="font-mono text-muted-foreground">{parameter.column}</dt><dd class="max-h-24 overflow-auto whitespace-pre-wrap wrap-anywhere">{parameter.value.kind === "null" ? "SQL NULL" : `${parameter.value.kind}: ${parameter.value.value}`}</dd></div>{/each}</dl>{/if}
		<div class="flex flex-wrap items-end gap-3"><label class="grid min-w-0 flex-1 gap-2 text-xs font-medium">Confirm {preview.confirmation}<Input bind:value={confirmation} disabled={busy} autocomplete="off" /></label><Button variant="destructive" disabled={busy || confirmation !== preview.confirmation} onclick={apply}><CheckIcon />Confirm & Apply</Button><Button variant="outline" disabled={busy} onclick={() => { preview = null; confirmation = ""; }}><XIcon />Cancel Preview</Button></div>
	{:else}
		{#if kind !== "delete"}<div class="max-h-80 space-y-3 overflow-auto">{#each fields as field, index}<div class="grid items-center gap-2 sm:grid-cols-[minmax(0,1fr)_minmax(0,2fr)_auto]"><div class="min-w-0"><label for={`row-field-${index}`} class="text-xs font-medium wrap-anywhere">{field.column.name}</label><p class="text-xs text-muted-foreground">{field.column.columnType}</p></div><Input id={`row-field-${index}`} bind:value={field.value} maxlength={4096} inputmode={numeric(field.column.columnType) ? "decimal" : "text"} disabled={busy || field.isNull || !field.included} /><div class="flex items-center gap-3 text-xs">{#if kind === "insert"}<label class="flex items-center gap-1"><Checkbox bind:checked={field.included} disabled={busy} />Include</label>{/if}{#if field.column.nullable}<label class="flex items-center gap-1"><Checkbox bind:checked={field.isNull} disabled={busy || !field.included} />NULL</label>{/if}</div></div>{/each}</div>{:else}<p class="text-sm text-destructive">One row will be deleted from {database}.{table}.</p>{/if}
		<Button variant="outline" disabled={busy} onclick={review}><EyeIcon />Preview SQL</Button>
	{/if}
</section>
