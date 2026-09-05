<script lang="ts">
	import { onMount } from "svelte";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import PlusIcon from "@lucide/svelte/icons/plus";
	import SaveIcon from "@lucide/svelte/icons/save";
	import Trash2Icon from "@lucide/svelte/icons/trash-2";
	import PencilIcon from "@lucide/svelte/icons/pencil";
	import ArrowRightLeftIcon from "@lucide/svelte/icons/arrow-right-left";
	import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import { chooseFolder } from "$lib/core/selectFolder";
	import { captureActiveWorkspace, initializeWorkspaces, removeWorkspace, saveWorkspace, switchWorkspace, workspaceSession } from "$lib/core/workspaces.svelte";
	import { emptyWorkspace, type Workspace } from "$lib/core/workspaceSettings";
	import { taskSession } from "$lib/core/tasks.svelte";

	let draft = $state<Workspace | null>(null);
	let busy = $state(false);
	let error = $state("");
	let message = $state("");
	onMount(() => { initializeWorkspaces(); captureActiveWorkspace(); });
	function edit(workspace: Workspace) {
		draft = JSON.parse(JSON.stringify(workspace));
		error = "";
	}
	async function run(action: () => void | Promise<void>) {
		busy = true; error = ""; message = "";
		try { await action(); } catch (caught) { error = caught instanceof Error ? caught.message : String(caught); }
		finally { busy = false; }
	}
	async function browse(key: "artifactPath" | "txDataPath") {
		if (!draft) return;
		const selected = await chooseFolder(draft[key]);
		if (selected && draft) draft[key] = selected;
	}
</script>

<div class="space-y-6">
	<header class="flex items-center justify-between gap-3 border-b border-border pb-4">
		<h1 class="text-2xl font-semibold">Workspaces</h1>
		<Button onclick={() => { draft = emptyWorkspace(crypto.randomUUID(), ""); }} disabled={busy}><PlusIcon class="size-4" />New workspace</Button>
	</header>
	{#if error}<Notice tone="error" title="Workspace" message={error} onDismiss={() => error = ""} />{/if}
	{#if message}<Notice tone="success" title="Workspace" {message} onDismiss={() => message = ""} />{/if}
	<div class="divide-y divide-border border-y border-border">
		{#each workspaceSession.items as workspace (workspace.id)}
			<div class="flex flex-wrap items-center gap-3 py-4">
				<div class="min-w-0 flex-1 basis-48">
					<div class="flex flex-wrap items-center gap-2"><h2 class="break-all text-sm font-semibold">{workspace.name}</h2>{#if workspace.id === workspaceSession.activeId}<span class="rounded-sm bg-emerald-500/10 px-2 py-0.5 text-xs text-emerald-400">Active</span>{/if}</div>
					<p class="mt-1 break-all text-xs text-muted-foreground">{workspace.txDataPath || "No txData folder"}{workspace.profile ? ` / ${workspace.profile}` : ""}</p>
				</div>
				<div class="flex items-center gap-2">
					<Button size="sm" variant="outline" disabled={busy || taskSession.switching || workspace.id === workspaceSession.activeId} onclick={() => run(async () => { await switchWorkspace(workspace.id); message = `Switched to ${workspace.name}.`; })}>{#if taskSession.switching}<LoaderCircleIcon class="size-4 animate-spin" />{:else}<ArrowRightLeftIcon class="size-4" />{/if}Switch</Button>
					<Button size="icon" variant="ghost" title={`Edit ${workspace.name}`} aria-label={`Edit ${workspace.name}`} disabled={busy} onclick={() => edit(workspace)}><PencilIcon class="size-4" /></Button>
					<Button size="icon" variant="ghost" title={`Remove ${workspace.name}`} aria-label={`Remove ${workspace.name}`} disabled={busy || workspace.id === workspaceSession.activeId} onclick={() => run(async () => { if (window.confirm(`Remove saved workspace "${workspace.name}", its backup schedules, and its saved RCON password? Server files, backups, and databases will not be deleted.`)) await removeWorkspace(workspace.id); })}><Trash2Icon class="size-4 text-destructive" /></Button>
				</div>
			</div>
		{/each}
	</div>
	{#if draft}
		<form class="space-y-5 border-b border-border pb-6" onsubmit={(event) => { event.preventDefault(); void run(async () => { if (!draft) return; await saveWorkspace(draft); draft = null; message = "Workspace saved."; }); }}>
			<h2 class="text-lg font-semibold">{workspaceSession.items.some((item) => item.id === draft?.id) ? "Edit workspace" : "New workspace"}</h2>
			<label class="grid gap-2 text-sm">Name<Input bind:value={draft.name} required maxlength={80} placeholder="Development" /></label>
			<div class="grid gap-4 md:grid-cols-2">
				{#each [{ key: "artifactPath", label: "Artifact folder" }, { key: "txDataPath", label: "txData folder" }] as field}
					<label class="grid gap-2 text-sm">{field.label}<div class="flex gap-2"><Input aria-label={field.label} bind:value={draft[field.key as "artifactPath" | "txDataPath"]} /><Button type="button" variant="outline" size="icon" title={`Browse ${field.label}`} aria-label={`Browse ${field.label}`} onclick={() => run(() => browse(field.key as "artifactPath" | "txDataPath"))}><FolderOpenIcon class="size-4" /></Button></div></label>
				{/each}
				<label class="grid gap-2 text-sm">txAdmin profile<Input bind:value={draft.profile} placeholder="default" /></label>
				<label class="grid gap-2 text-sm">RCON host<Input value={draft.environment.TXHOST_RCON_HOST ?? "127.0.0.1"} oninput={(event) => { if (draft) draft.environment.TXHOST_RCON_HOST = event.currentTarget.value; }} /></label>
				<label class="grid gap-2 text-sm">RCON port<Input type="number" min={1} max={65535} value={Number(draft.environment.TXHOST_RCON_PORT ?? 30120)} oninput={(event) => { if (draft) draft.environment.TXHOST_RCON_PORT = event.currentTarget.value; }} /></label>
			</div>
			<h3 class="border-t border-border pt-4 text-sm font-semibold">Database defaults</h3>
			<div class="grid gap-4 sm:grid-cols-2">
				<label class="grid gap-2 text-sm">Host<Input bind:value={draft.database.host} required /></label>
				<label class="grid gap-2 text-sm">Port<Input type="number" min={1} max={65535} bind:value={draft.database.port} required /></label>
				<label class="grid gap-2 text-sm">Username<Input bind:value={draft.database.username} required /></label>
				<label class="grid gap-2 text-sm">Database<Input bind:value={draft.database.database} /></label>
			</div>
			<div class="flex justify-end gap-2"><Button type="button" variant="ghost" onclick={() => draft = null} disabled={busy}>Cancel</Button><Button type="submit" disabled={busy}><SaveIcon class="size-4" />Save workspace</Button></div>
		</form>
	{/if}
</div>
