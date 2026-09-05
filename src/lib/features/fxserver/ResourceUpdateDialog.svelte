<script lang="ts">
	import { Dialog } from "bits-ui";
	import { onDestroy, onMount } from "svelte";
	import DownloadIcon from "@lucide/svelte/icons/download";
	import HistoryIcon from "@lucide/svelte/icons/history";
	import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
	import ShieldCheckIcon from "@lucide/svelte/icons/shield-check";
	import XIcon from "@lucide/svelte/icons/x";
	import TrashIcon from "@lucide/svelte/icons/trash-2";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Checkbox } from "$lib/components/ui/checkbox/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import {
		applyResourceUpdate, discardResourcePreview, listResourceSnapshots, previewResourceUpdate, rollbackResourceUpdate, deleteResourceSnapshot,
		type ResourceTarget, type ResourceSnapshot, type ResourceUpdatePreview,
	} from "$lib/modules/resourceUpdates";

	let { target, branch, name, reinstall = false, history = false, onclose, oncomplete }: {
		target: ResourceTarget; branch: string; name: string; reinstall?: boolean; history?: boolean;
		onclose: () => void; oncomplete: (message: string) => void;
	} = $props();
	let open = $state(true);
	let busy = $state(false);
	let loading = $state(true);
	let error = $state("");
	let warning = $state(true);
	let preview = $state<ResourceUpdatePreview | null>(null);
	let snapshots = $state<ResourceSnapshot[]>([]);
	let protectedPaths = $state<string[]>([]);
	let search = $state("");
	let page = $state(0);
	let selectedSnapshot = $state<ResourceSnapshot | null>(null);
	let deletingSnapshot = $state<ResourceSnapshot | null>(null);
	let active = true;
	const filtered = $derived(preview?.changes.filter((file) => file.path.toLowerCase().includes(search.trim().toLowerCase())) ?? []);
	const pages = $derived(Math.max(1, Math.ceil(filtered.length / 100)));
	const visible = $derived(filtered.slice(Math.min(page, pages - 1) * 100, (Math.min(page, pages - 1) + 1) * 100));
	const preserved = $derived(new Set(protectedPaths));

	onMount(() => { void initialize(); });
	onDestroy(() => {
		active = false;
		if (preview && !busy) void discardResourcePreview(preview.id).catch(() => {});
	});

	async function initialize() {
		try {
			if (history) {
				const result = await listResourceSnapshots(target);
				if (active) snapshots = result;
			} else {
				const result = await previewResourceUpdate(target, branch);
				if (!active) { await discardResourcePreview(result.id); return; }
				preview = result;
				protectedPaths = result.changes.filter((file) => file.preserve).map((file) => file.path);
			}
		} catch (caught) { if (active) error = String(caught); }
		finally { if (active) loading = false; }
	}

	function toggle(path: string, checked: boolean) {
		protectedPaths = checked ? [...protectedPaths, path] : protectedPaths.filter((value) => value !== path);
	}

	async function apply() {
		if (!preview || busy) return;
		busy = true; error = "";
		try {
			await applyResourceUpdate(target, preview.id, protectedPaths);
			if (active) oncomplete(`${name} ${reinstall ? "re-installed" : "updated"}. A verified rollback snapshot was saved.`);
		} catch (caught) { if (active) error = String(caught); }
		finally { busy = false; }
	}

	async function restore() {
		if (!selectedSnapshot || busy) return;
		busy = true; error = "";
		try {
			await rollbackResourceUpdate(target, selectedSnapshot.id);
			if (active) oncomplete(`${name} restored. The previous files were saved in a new snapshot.`);
		} catch (caught) { if (active) error = String(caught); }
		finally { busy = false; }
	}

	async function removeSnapshot() {
		if (!deletingSnapshot || busy) return;
		busy = true; error = "";
		try {
			await deleteResourceSnapshot(target, deletingSnapshot.id);
			snapshots = snapshots.filter((snapshot) => snapshot.id !== deletingSnapshot!.id);
			if (selectedSnapshot?.id === deletingSnapshot.id) selectedSnapshot = null;
			deletingSnapshot = null;
		} catch (caught) { error = String(caught); }
		finally { busy = false; }
	}

	function bytes(value: number) { return value < 1024 ? `${value} B` : value < 1048576 ? `${(value / 1024).toFixed(1)} KiB` : `${(value / 1048576).toFixed(1)} MiB`; }
</script>

<Dialog.Root bind:open onOpenChange={(value) => { if (!value) onclose(); }}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-[119] bg-black/65 data-[state=open]:animate-in data-[state=open]:fade-in-0" />
		<Dialog.Content class="fixed top-1/2 left-1/2 z-[120] flex max-h-[85vh] w-[calc(100vw-2rem)] max-w-4xl -translate-x-1/2 -translate-y-1/2 flex-col gap-4 overflow-hidden rounded-md border border-border bg-background p-5 shadow-xl data-[state=open]:animate-in data-[state=open]:fade-in-0 data-[state=open]:zoom-in-95">
			<div class="flex items-start justify-between gap-4">
				<div class="min-w-0">
					<Dialog.Title class="break-all text-lg font-semibold">{history ? "Snapshots" : reinstall ? "Preview Re-install" : "Preview Update"}: {name}</Dialog.Title>
					<Dialog.Description class="mt-1 text-sm text-muted-foreground">{history ? "Restore a saved resource version." : "Review changed files and choose which local files to keep."}</Dialog.Description>
				</div>
				<Button variant="ghost" size="icon" title="Close preview" aria-label="Close preview" onclick={onclose}><XIcon /></Button>
			</div>
			{#if error}<Notice tone="error" message={error} onDismiss={() => (error = "")} />{/if}
			{#if warning}<Notice tone="warn" title="Stop the resource before replacing files" message="Config files can require manual migration after an update. Snapshots preserve resource files only, not database changes. Keep an independent backup." onDismiss={() => (warning = false)} />{/if}
			{#if loading}
				<div class="flex items-center gap-3 py-8 text-sm text-muted-foreground"><LoaderCircleIcon class="size-4 animate-spin" />{history ? "Loading snapshots..." : "Downloading, verifying, and comparing files..."}</div>
			{:else if preview}
				<div class="space-y-1 border-y border-border py-3 text-xs text-muted-foreground">
					<p class="break-all">{preview.repository} / {preview.branch} ({bytes(preview.archiveBytes)})</p>
					<p class="break-all font-mono">SHA-256: {preview.archiveSha256}</p>
				</div>
				<div class="flex flex-wrap items-center justify-between gap-2">
					<p class="text-sm">{preview.changes.length} changes <span class="text-muted-foreground">/ {protectedPaths.length} protected</span></p>
					<Button size="sm" variant="outline" disabled={busy} onclick={() => (protectedPaths = preview!.changes.filter((file) => file.preserve).map((file) => file.path))}><ShieldCheckIcon />Reset Protection</Button>
				</div>
				<Input bind:value={search} placeholder="Filter changed files" aria-label="Filter changed files" />
				<div class="min-h-0 overflow-auto rounded-sm border border-border">
					<table class="w-full table-fixed text-left text-xs">
						<thead class="sticky top-0 z-10 bg-muted text-muted-foreground"><tr><th class="w-16 p-3">Keep</th><th class="p-3">File</th><th class="w-24 p-3">Change</th><th class="hidden w-32 p-3 sm:table-cell">Size</th></tr></thead>
						<tbody>
							{#each visible as file (file.path)}
								<tr class="border-t border-border"><td class="p-3"><Checkbox checked={preserved.has(file.path)} disabled={!file.canPreserve || busy} onCheckedChange={(checked) => toggle(file.path, checked)} aria-label={`Preserve ${file.path}`} /></td><td class="break-all p-3 font-mono">{file.path}</td><td class={`p-3 ${preserved.has(file.path) ? "text-sky-400" : file.kind === "removed" ? "text-red-400" : file.kind === "added" ? "text-emerald-400" : "text-amber-400"}`}>{preserved.has(file.path) ? "Protected" : file.kind}</td><td class="hidden p-3 font-mono text-muted-foreground sm:table-cell">{bytes(file.oldSize ?? 0)} / {bytes(file.newSize ?? 0)}</td></tr>
							{:else}<tr><td colspan="4" class="p-5 text-center text-muted-foreground">{preview.changes.length ? "No matching files." : "All downloaded files match the local resource."}</td></tr>{/each}
						</tbody>
					</table>
				</div>
				{#if pages > 1}<div class="flex items-center justify-end gap-3 text-xs"><Button size="sm" variant="outline" disabled={page === 0} onclick={() => page--}>Previous</Button><span>{Math.min(page, pages - 1) + 1} / {pages}</span><Button size="sm" variant="outline" disabled={page >= pages - 1} onclick={() => page++}>Next</Button></div>{/if}
			{:else if history}
				<div class="min-h-0 space-y-2 overflow-auto">
					{#each snapshots as snapshot (snapshot.id)}
						<div class="flex flex-wrap items-center justify-between gap-3 border-b border-border py-3"><div><p class="text-sm">{new Date(snapshot.createdAt * 1000).toLocaleString()}</p><p class="mt-1 text-xs text-muted-foreground">{snapshot.reason} / {snapshot.fileCount} files / {bytes(snapshot.sizeBytes)}</p></div><div class="flex items-center gap-2"><Button variant={selectedSnapshot?.id === snapshot.id ? "default" : "outline"} size="sm" disabled={busy} onclick={() => { selectedSnapshot = snapshot; deletingSnapshot = null; }}><HistoryIcon />Select</Button><Button variant="ghost" size="icon" disabled={busy} title="Delete snapshot" aria-label={`Delete snapshot from ${new Date(snapshot.createdAt * 1000).toLocaleString()}`} onclick={() => (deletingSnapshot = snapshot)}><TrashIcon /></Button></div></div>
					{:else}<p class="py-6 text-sm text-muted-foreground">No snapshots have been created for this resource in this workspace.</p>{/each}
				</div>
				{#if selectedSnapshot}<p class="text-sm text-amber-400">Restoring replaces all resource files with the selected snapshot. Current files will be snapshotted first.</p>{/if}
				{#if deletingSnapshot}<div class="flex flex-wrap items-center justify-between gap-3 border-t border-border pt-3"><p class="text-sm text-destructive">Permanently delete the snapshot from {new Date(deletingSnapshot.createdAt * 1000).toLocaleString()}?</p><Button variant="destructive" size="sm" disabled={busy} onclick={removeSnapshot}><TrashIcon />Confirm Delete</Button></div>{/if}
			{/if}
			<div class="flex justify-end gap-2 border-t border-border pt-4">
				<Button variant="outline" onclick={onclose}>{busy ? "Close" : "Cancel"}</Button>
				{#if history}<Button onclick={restore} disabled={!selectedSnapshot || busy}>{#if busy}<LoaderCircleIcon class="animate-spin" />{:else}<HistoryIcon />{/if}{busy ? "Restoring..." : "Confirm Restore"}</Button>
				{:else}<Button onclick={apply} disabled={!preview || busy}>{#if busy}<LoaderCircleIcon class="animate-spin" />{:else}<DownloadIcon />{/if}{busy ? "Applying..." : reinstall ? "Snapshot & Re-install" : "Snapshot & Update"}</Button>{/if}
			</div>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
