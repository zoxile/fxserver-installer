<script lang="ts">
	import SaveIcon from "@lucide/svelte/icons/save";
	import FolderPlusIcon from "@lucide/svelte/icons/folder-plus";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import PencilIcon from "@lucide/svelte/icons/pencil";
	import Trash2Icon from "@lucide/svelte/icons/trash-2";
	import PlusIcon from "@lucide/svelte/icons/plus";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import * as Select from "$lib/components/ui/select/index.js";
	import { createDatabaseLibrarySession } from "./databaseLibrarySession.svelte";
	import type { SavedQuery } from "./databaseLibrary";
	import DatabaseLibraryReset from "./DatabaseLibraryReset.svelte";

	let { query, database, busy, onLoad }: { query: string; database: string; busy: boolean; onLoad: (saved: SavedQuery) => void } = $props();
	const library = createDatabaseLibrarySession();
	let folderId = $state("");
	let folderName = $state("");
	let selectedId = $state("");
	let name = $state("");
	const folders = $derived(library.state.library.folders.filter((item) => item.scope === library.state.scope));
	const queries = $derived(library.state.library.queries.filter((item) => item.scope === library.state.scope));
	const selected = $derived(queries.find((item) => item.id === selectedId));
	const blocked = $derived(busy || !library.state.ready);
	const folderOptions = $derived([{ value: "__root__", label: "Unfiled" }, ...folders.map((folder) => ({ value: folder.id, label: folder.name }))]);
	const queryOptions = $derived(queries.map((item) => ({ value: item.id, label: `${folders.find((folder) => folder.id === item.folderId)?.name ?? "Unfiled"} / ${item.name}` })));
	$effect(() => { library.state.scope; selectedId = ""; folderId = ""; folderName = ""; name = ""; });

	function select(id: string) {
		const item = queries.find((entry) => entry.id === id);
		selectedId = id; name = item?.name ?? ""; folderId = item?.folderId ?? "";
	}
	function save(update: boolean) {
		if (blocked) return;
		const id = update ? selected?.id : crypto.randomUUID();
		if (!id) return;
		if (library.update((data, scope) => {
			const item: SavedQuery = { id, scope, name: name.trim(), folderId: folderId || null, database, sql: query };
			if (update) data.queries = data.queries.filter((entry) => entry.id !== id || entry.scope !== scope);
			data.queries.push(item);
		})) selectedId = id;
	}
	function rename() {
		if (!selected || blocked) return;
		library.update((data, scope) => {
			const item = data.queries.find((entry) => entry.id === selectedId && entry.scope === scope);
			if (item) { item.name = name.trim(); item.folderId = folderId || null; }
		});
	}
	function folderAction(action: "create" | "rename" | "delete") {
		if (blocked) return;
		const id = action === "create" ? crypto.randomUUID() : folderId;
		if (library.update((data, scope) => {
			if (action === "delete") {
				if (data.queries.some((item) => item.scope === scope && item.folderId === id)) throw new Error("Move or remove this folder's queries before deleting it.");
				data.folders = data.folders.filter((item) => item.id !== id || item.scope !== scope);
			} else if (action === "create") data.folders.push({ id, scope, name: folderName.trim() });
			else {
				const item = data.folders.find((entry) => entry.id === id && entry.scope === scope);
				if (item) item.name = folderName.trim();
			}
		})) { folderId = action === "delete" ? "" : id; folderName = ""; }
	}
</script>

<details class="border-y border-border py-3">
	<summary class="cursor-pointer text-sm font-medium">Saved Queries ({library.state.readOnly ? "unavailable" : queries.length})</summary>
	<div class="mt-3 space-y-3">
		{#if library.state.error}<Notice tone="error" message={library.state.error} />{/if}
		{#if library.state.readOnly}<DatabaseLibraryReset {busy} onReset={library.reset} />{/if}
		<div class="flex flex-wrap items-end gap-2">
			<label class="grid min-w-0 flex-1 basis-full gap-1 text-xs sm:basis-0">Saved query
				<Select.Root type="single" value={selectedId} items={queryOptions} disabled={blocked} onValueChange={select}><Select.Trigger class="w-full min-w-0"><span class="truncate">{queryOptions.find((item) => item.value === selectedId)?.label ?? "Choose query"}</span></Select.Trigger><Select.Content>{#each queryOptions as option}<Select.Item value={option.value} label={option.label}>{option.label}</Select.Item>{/each}</Select.Content></Select.Root>
			</label>
			<Button size="sm" variant="outline" disabled={blocked || !selected} onclick={() => { if (selected) onLoad(selected); }}><FolderOpenIcon />Load</Button>
			<Button size="icon-sm" variant="ghost" disabled={blocked || !selected} title="Delete saved query" aria-label="Delete saved query" onclick={() => { if (library.update((data, scope) => { data.queries = data.queries.filter((item) => item.id !== selectedId || item.scope !== scope); })) selectedId = ""; }}><Trash2Icon /></Button>
		</div>
		<div class="grid gap-2 sm:grid-cols-2">
			<label class="grid min-w-0 gap-1 text-xs">Query name<Input bind:value={name} maxlength={80} disabled={blocked} autocomplete="off" /></label>
			<label class="grid min-w-0 gap-1 text-xs">Folder<Select.Root type="single" value={folderId || "__root__"} items={folderOptions} disabled={blocked} onValueChange={(value) => folderId = value === "__root__" ? "" : value}><Select.Trigger class="w-full min-w-0"><span class="truncate">{folders.find((folder) => folder.id === folderId)?.name ?? "Unfiled"}</span></Select.Trigger><Select.Content>{#each folderOptions as option}<Select.Item value={option.value} label={option.label}>{option.label}</Select.Item>{/each}</Select.Content></Select.Root></label>
		</div>
		<div class="flex flex-wrap gap-2">
			<Button size="sm" variant="outline" disabled={blocked || !name.trim() || !query.trim()} onclick={() => save(false)}><PlusIcon />Save New</Button>
			<Button size="sm" variant="outline" disabled={blocked || !selected || !name.trim() || !query.trim()} onclick={() => save(true)}><SaveIcon />Update SQL</Button>
			<Button size="sm" variant="ghost" disabled={blocked || !selected || !name.trim()} onclick={rename}><PencilIcon />Rename / Move</Button>
		</div>
		<div class="flex flex-wrap items-end gap-2">
			<label class="grid min-w-0 flex-1 basis-full gap-1 text-xs sm:basis-0">Folder name<Input bind:value={folderName} maxlength={80} disabled={blocked} autocomplete="off" /></label>
			<Button size="icon-sm" variant="outline" disabled={blocked || !folderName.trim()} title="Create folder" aria-label="Create folder" onclick={() => folderAction("create")}><FolderPlusIcon /></Button>
			<Button size="icon-sm" variant="ghost" disabled={blocked || !folderId || !folderName.trim()} title="Rename folder" aria-label="Rename folder" onclick={() => folderAction("rename")}><PencilIcon /></Button>
			<Button size="icon-sm" variant="ghost" disabled={blocked || !folderId} title="Delete empty folder" aria-label="Delete empty folder" onclick={() => folderAction("delete")}><Trash2Icon /></Button>
		</div>
		<p class="text-xs text-muted-foreground">Local, unencrypted storage: SQL templates and names only, scoped to this workspace and connection. No credentials, parameter values, results, or query history. Secret checks are best-effort, not a guarantee; do not save sensitive SQL. Limits: 32 folders, 100 queries, 100 bookmarks, 512 KiB total.</p>
	</div>
</details>
