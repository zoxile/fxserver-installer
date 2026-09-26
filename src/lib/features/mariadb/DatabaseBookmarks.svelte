<script lang="ts">
	import BookmarkPlusIcon from "@lucide/svelte/icons/bookmark-plus";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import SaveIcon from "@lucide/svelte/icons/save";
	import Trash2Icon from "@lucide/svelte/icons/trash-2";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import * as Select from "$lib/components/ui/select/index.js";
	import type { BrowserRequest } from "$lib/modules/databaseBrowser";
	import type { DatabaseBookmark } from "./databaseLibrary";
	import { createDatabaseLibrarySession } from "./databaseLibrarySession.svelte";
	import DatabaseLibraryReset from "./DatabaseLibraryReset.svelte";

	let { busy, canSave, request, view, onRestore }: { busy: boolean; canSave: boolean; request: () => BrowserRequest; view: string; onRestore: (bookmark: DatabaseBookmark) => void } = $props();
	const library = createDatabaseLibrarySession();
	let selectedId = $state("");
	let name = $state("");
	const bookmarks = $derived(library.state.library.bookmarks.filter((item) => item.scope === library.state.scope));
	const options = $derived(bookmarks.map((item) => ({ value: item.id, label: item.name })));
	const selected = $derived(bookmarks.find((item) => item.id === selectedId));
	const blocked = $derived(busy || !library.state.ready);
	$effect(() => { library.state.scope; selectedId = ""; name = ""; });
	function save(update: boolean) {
		if (blocked || !canSave || !["rows", "columns", "indexes"].includes(view)) return;
		const id = update ? selected?.id : crypto.randomUUID();
		if (!id) return;
		if (library.update((data, scope) => {
			const current = request();
			const filters = current.filters.map((filter) => ({ ...filter, value: ["isNull", "isNotNull"].includes(filter.operator) ? null : filter.value }));
			const item: DatabaseBookmark = { ...current, filters, id, scope, name: name.trim(), view: view as DatabaseBookmark["view"] };
			if (update) data.bookmarks = data.bookmarks.filter((entry) => entry.id !== id || entry.scope !== scope);
			data.bookmarks.push(item);
		})) selectedId = id;
	}
</script>

<details class="border-y border-border py-3">
	<summary class="cursor-pointer text-sm font-medium">Bookmarks ({library.state.readOnly ? "unavailable" : bookmarks.length})</summary>
	<div class="mt-3 space-y-3">
		{#if library.state.error}<Notice tone="error" message={library.state.error} />{/if}
		{#if library.state.readOnly}<DatabaseLibraryReset {busy} onReset={library.reset} />{/if}
		<div class="flex flex-wrap items-end gap-2">
			<label class="grid min-w-0 flex-1 basis-full gap-1 text-xs sm:basis-0">Saved view<Select.Root type="single" value={selectedId} items={options} disabled={blocked} onValueChange={(value) => { selectedId = value; name = bookmarks.find((item) => item.id === value)?.name ?? ""; }}><Select.Trigger class="w-full min-w-0"><span class="truncate">{selected?.name ?? "Choose bookmark"}</span></Select.Trigger><Select.Content>{#each options as option}<Select.Item value={option.value} label={option.label}>{option.label}</Select.Item>{/each}</Select.Content></Select.Root></label>
			<Button variant="outline" size="sm" disabled={blocked || !selected} onclick={() => { if (selected) onRestore(selected); }}><FolderOpenIcon />Restore</Button>
			<Button variant="ghost" size="icon-sm" disabled={blocked || !selected} title="Delete bookmark" aria-label="Delete bookmark" onclick={() => { if (library.update((data, scope) => { data.bookmarks = data.bookmarks.filter((item) => item.id !== selectedId || item.scope !== scope); })) selectedId = ""; }}><Trash2Icon /></Button>
		</div>
		{#if selected}<p class="wrap-anywhere font-mono text-xs text-muted-foreground">{selected.database}.{selected.table} / {selected.view} / {selected.filters.length} applied filters</p>{/if}
		<div class="flex flex-wrap items-end gap-2">
			<label class="grid min-w-0 flex-1 basis-full gap-1 text-xs sm:basis-0">Bookmark name<Input bind:value={name} maxlength={80} disabled={blocked} autocomplete="off" /></label>
			<Button variant="outline" size="sm" disabled={blocked || !canSave || !name.trim()} onclick={() => save(false)}><BookmarkPlusIcon />Save New</Button>
			<Button variant="outline" size="sm" disabled={blocked || !canSave || !selected || !name.trim()} onclick={() => save(true)}><SaveIcon />Update View</Button>
			<Button variant="ghost" size="sm" disabled={blocked || !selected || !name.trim()} onclick={() => library.update((data, scope) => { const item = data.bookmarks.find((entry) => entry.id === selectedId && entry.scope === scope); if (item) item.name = name.trim(); })}>Rename</Button>
		</div>
		<p class="text-xs text-muted-foreground">Local, unencrypted storage: table, view, applied filters, sort, and page only. No credentials, rows, or editing state. Secret checks are best-effort, not a guarantee; do not save sensitive filters. Limits: 100 bookmarks and 512 KiB shared with saved queries.</p>
	</div>
</details>
