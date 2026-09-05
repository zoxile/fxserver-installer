<script lang="ts">
	import { onDestroy, onMount } from "svelte";
	import { open } from "@tauri-apps/plugin-dialog";
	import CopyIcon from "@lucide/svelte/icons/copy";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import ScanSearchIcon from "@lucide/svelte/icons/scan-search";
	import DownloadIcon from "@lucide/svelte/icons/download";
	import UploadIcon from "@lucide/svelte/icons/upload";
	import XIcon from "@lucide/svelte/icons/x";
	import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Checkbox } from "$lib/components/ui/checkbox/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import { chooseFolder } from "$lib/core/selectFolder";
	import { emptyWorkspace, type Workspace } from "$lib/core/workspaceSettings";
	import { saveWorkspace, workspaceSession } from "$lib/core/workspaces.svelte";
	import { hasRunningTasks, taskSession } from "$lib/core/tasks.svelte";
	import { appendIncident } from "$lib/core/incidents.svelte";
	import { discardClonePreview, executeClone, listCloneChoices, previewClone, type CloneChoices, type CloneMode, type ClonePreview, type CloneResult } from "$lib/modules/workspaceClone";

	let { source, onClose }: { source: Workspace; onClose: () => void } = $props();
	let mode = $state<CloneMode>("clone");
	let sourcePath = $state("");
	let destinationParent = $state("");
	let folderName = $state("server-clone");
	let workspaceName = $state("");
	let sourceServerPort = $state(30120);
	let sourceTxAdminPort = $state(40120);
	let serverPort = $state(30121);
	let txAdminPort = $state(40121);
	let resources = $state<string[]>([]);
	let configs = $state<string[]>([]);
	let choices = $state<CloneChoices | null>(null);
	let preview = $state<ClonePreview | null>(null);
	let result = $state<CloneResult | null>(null);
	let error = $state("");
	let busy = $state(false);
	let confirmedDestination = $state("");
	let rightsConfirmed = $state(false);
	let includeDatabase = $state(false);
	let dumpPath = $state("");
	let sourceDatabase = $state("");
	let targetHost = $state("localhost");
	let targetPort = $state(3306);
	let targetUsername = $state("root");
	let targetPassword = $state("");
	let confirmedDatabase = $state("");
	let saved = $state(false);
	const destinationPath = $derived(destinationParent ? `${destinationParent.replace(/[\\/]+$/, "")}/${folderName}` : "");
	const blocked = $derived(busy || hasRunningTasks() || taskSession.switching);
	const locked = $derived(Boolean(preview || result));
	const database = $derived(includeDatabase ? { dumpPath, sourceDatabase, host: targetHost, port: targetPort, username: targetUsername } : null);
	const formKey = $derived(JSON.stringify({ mode, sourcePath, destinationPath, resources, configs, serverPort, txAdminPort, sourceServerPort, sourceTxAdminPort, database }));
	let reviewedKey = $state("");
	onMount(() => {
		workspaceName = `${source.name} clone`;
		sourceDatabase = source.database.database;
		sourceServerPort = Number(source.environment.TXHOST_FXS_PORT || 30120);
		sourceTxAdminPort = Number(source.environment.TXHOST_TXA_PORT || 40120);
		serverPort = sourceServerPort < 65535 ? sourceServerPort + 1 : 30121;
		txAdminPort = sourceTxAdminPort < 65535 ? sourceTxAdminPort + 1 : 40121;
	});

	async function run(action: () => Promise<void>) {
		busy = true; error = "";
		try { await action(); } catch (caught) { error = caught instanceof Error ? caught.message : String(caught); }
		finally { busy = false; }
	}
	async function clearPreview() {
		const old = preview; preview = null; confirmedDestination = ""; rightsConfirmed = false; confirmedDatabase = ""; targetPassword = "";
		if (old) await discardClonePreview(old.id);
	}
	async function browseSource() {
		const selected = await chooseFolder(sourcePath || source.txDataPath);
		if (selected) { sourcePath = selected; choices = null; resources = []; configs = []; }
	}
	async function loadChoices() {
		choices = await listCloneChoices(sourcePath);
		resources = []; configs = [];
	}
	function selection(values: string[], value: string, checked: boolean) { return checked ? [...values.filter((item) => item !== value), value] : values.filter((item) => item !== value); }
	async function prepare() {
		if (!destinationParent || !folderName || !sourcePath) throw new Error("Choose an explicit source and a new destination folder.");
		if (mode !== "import" && (!choices || !resources.length && !configs.length)) throw new Error("Load the source and select at least one resource or configuration.");
		if (mode !== "export" && (!workspaceName.trim() || workspaceSession.items.some((item) => item.name.toLowerCase() === workspaceName.trim().toLowerCase()))) throw new Error("Enter a unique name for the new workspace.");
		preview = await previewClone({ mode, sourcePath, destinationPath, resources, configs, serverPort, txAdminPort, sourceServerPort, sourceTxAdminPort, database });
		reviewedKey = formKey;
	}
	async function copy() {
		if (!preview || reviewedKey !== formKey) throw new Error("The form changed. Prepare a new preview.");
		const reviewed = preview;
		try {
			const credentials = reviewed.database?.target ? { host: targetHost, port: targetPort, username: targetUsername, password: targetPassword, database: reviewed.database.target.database } : undefined;
			result = await executeClone(reviewed, confirmedDestination, rightsConfirmed, credentials, confirmedDatabase);
			appendIncident({ workspaceId: source.id, type: "workspace", panel: "workspaces", level: "success", title: mode === "export" ? "Private clone package exported" : "Private server clone created", detail: `${result.fileCount} files copied. No server launched.${result.database ? " Selected dump imported into a new app-owned database." : " No database restored."}` });
		} finally { preview = null; targetPassword = ""; }
		if (mode !== "export") await registerWorkspace();
	}
	async function registerWorkspace() {
		if (!result) return;
		const workspace = emptyWorkspace(crypto.randomUUID(), workspaceName.trim());
		workspace.txDataPath = result.txDataPath;
		workspace.artifactPath = result.artifactPath;
		workspace.environment = { TXHOST_DATA_PATH: result.txDataPath, TXHOST_FXS_PORT: String(serverPort), TXHOST_TXA_PORT: String(txAdminPort), TXHOST_RCON_PORT: String(serverPort), TXHOST_RCON_HOST: "127.0.0.1" };
		if (result.database) workspace.database = { ...result.database };
		await saveWorkspace(workspace);
		saved = true;
	}
	onDestroy(() => { if (preview) void discardClonePreview(preview.id).catch(() => {}); });
</script>

<section class="space-y-5 border-y border-border py-5" aria-label="Server cloning and migration">
	<header class="flex items-center justify-between gap-3"><h2 class="text-lg font-semibold">Clone &amp; Migration</h2><Button size="icon" variant="ghost" title="Close clone panel" aria-label="Close clone panel" disabled={busy} onclick={onClose}><XIcon class="size-4" /></Button></header>
	{#if error}<Notice tone="error" title="Clone" message={error} onDismiss={() => error = ""} />{/if}
	<div class="flex flex-wrap gap-1 border-b border-border pb-3" role="group" aria-label="Clone operation">
		{#each [{ id: "clone", label: "Private clone", icon: CopyIcon }, { id: "export", label: "Export package", icon: UploadIcon }, { id: "import", label: "Import package", icon: DownloadIcon }] as item}<Button size="sm" variant={mode === item.id ? "secondary" : "ghost"} aria-pressed={mode === item.id} disabled={locked || blocked} onclick={() => { mode = item.id as CloneMode; choices = null; resources = []; configs = []; sourcePath = ""; }}><item.icon class="size-4" />{item.label}</Button>{/each}
	</div>
	<Notice tone="warn" title="Private user copy" message="Copy only resources you have permission to use. Keep license notices and respect commercial or escrow terms. Packages stay local; no publishing or upload is performed. Known secrets, license keys, links, and generated files are excluded. Database dumps require separate opt-in. Review exclusions before copying." />
	<fieldset disabled={locked || blocked} class="grid min-w-0 gap-4 sm:grid-cols-2">
		<label class="grid min-w-0 gap-2 text-sm sm:col-span-2">{mode === "import" ? "Source package folder" : `Source server-data folder (${source.name})`}<div class="flex min-w-0 gap-2"><Input value={sourcePath} oninput={(event) => { sourcePath = event.currentTarget.value; choices = null; resources = []; configs = []; }} placeholder={mode === "import" ? "Folder containing clone-manifest.json" : "Folder containing server.cfg and resources"} /><Button size="icon" variant="outline" title="Browse source" aria-label="Browse source" onclick={() => run(browseSource)}><FolderOpenIcon class="size-4" /></Button></div></label>
		<label class="grid min-w-0 gap-2 text-sm">Destination parent folder<div class="flex min-w-0 gap-2"><Input bind:value={destinationParent} /><Button size="icon" variant="outline" title="Browse destination parent" aria-label="Browse destination parent" onclick={() => run(async () => { destinationParent = await chooseFolder(destinationParent) || destinationParent; })}><FolderOpenIcon class="size-4" /></Button></div></label>
		<label class="grid min-w-0 gap-2 text-sm">New folder name<Input bind:value={folderName} maxlength={100} /></label>
		{#if mode !== "export"}<label class="grid min-w-0 gap-2 text-sm sm:col-span-2">New workspace name<Input bind:value={workspaceName} maxlength={80} placeholder={`${source.name} clone`} /></label>{/if}
		<label class="grid gap-2 text-sm">Source server port<Input type="number" min={1} max={65535} bind:value={sourceServerPort} /></label><label class="grid gap-2 text-sm">Source txAdmin port<Input type="number" min={1} max={65535} bind:value={sourceTxAdminPort} /></label>
		<label class="grid gap-2 text-sm">Destination server port<Input type="number" min={1} max={65535} bind:value={serverPort} /></label><label class="grid gap-2 text-sm">Destination txAdmin port<Input type="number" min={1} max={65535} bind:value={txAdminPort} /></label>
	</fieldset>
	{#if mode !== "import" && !result}
		<Button size="sm" variant="outline" disabled={locked || blocked || !sourcePath} onclick={() => run(loadChoices)}><ScanSearchIcon class="size-4" />Load source files</Button>
		{#if choices}<div class="grid gap-4 sm:grid-cols-2">{#each [{ label: "Resources", items: choices.resources, kind: "resource" }, { label: "Configurations", items: choices.configs, kind: "config" }] as group}<fieldset disabled={locked || blocked} class="min-w-0"><legend class="mb-2 text-sm font-semibold">{group.label}</legend><div class="max-h-52 space-y-2 overflow-auto border-y border-border py-2">{#each group.items as item}<label class="flex items-start gap-2 text-sm"><Checkbox checked={(group.kind === "resource" ? resources : configs).includes(item)} onCheckedChange={(checked) => { if (group.kind === "resource") resources = selection(resources, item, checked === true); else configs = selection(configs, item, checked === true); }} /><span class="break-all">{item}</span></label>{:else}<p class="text-xs text-muted-foreground">None found</p>{/each}</div></fieldset>{/each}</div>{/if}
	{/if}
	<label class="flex items-start gap-2 text-sm"><Checkbox bind:checked={includeDatabase} disabled={locked || blocked} /><span>Include a reviewed database dump</span></label>
	{#if includeDatabase}
		<Notice tone="warn" title="Opt-in database copy" message="Only reviewed, secret-free UTF-8 SQL dumps up to 32 MiB are accepted. Unsupported SQL is rejected without rewriting. Data may contain private player information. Export includes the dump locally; clone/import creates a new fxsi_clone_ database only, never the source or an existing database." />
		<fieldset disabled={locked || blocked} class="grid min-w-0 gap-4 sm:grid-cols-2">
			{#if mode !== "import"}<label class="grid min-w-0 gap-2 text-sm sm:col-span-2">SQL dump file<div class="flex min-w-0 gap-2"><Input bind:value={dumpPath} /><Button variant="outline" size="icon" title="Choose SQL dump" aria-label="Choose SQL dump" onclick={() => run(async () => { const selected = await open({ multiple: false, filters: [{ name: "SQL dump", extensions: ["sql"] }] }); if (typeof selected === "string") dumpPath = selected; })}><FolderOpenIcon class="size-4" /></Button></div></label><label class="grid gap-2 text-sm sm:col-span-2">Source database name<Input bind:value={sourceDatabase} /></label>{/if}
			{#if mode !== "export"}<label class="grid gap-2 text-sm">Target database host<Input bind:value={targetHost} /></label><label class="grid gap-2 text-sm">Target database port<Input type="number" min={1} max={65535} bind:value={targetPort} /></label><label class="grid gap-2 text-sm sm:col-span-2">Target database username<Input bind:value={targetUsername} /></label>{/if}
		</fieldset>
	{/if}
	{#if preview}
		<div class="space-y-3 border-t border-border pt-4"><h3 class="text-sm font-semibold">Reviewed manifest</h3><dl class="grid gap-2 text-xs sm:grid-cols-[7rem_1fr]"><dt class="text-muted-foreground">Source</dt><dd class="break-all">{preview.sourcePath}</dd><dt class="text-muted-foreground">Destination</dt><dd class="break-all">{preview.destinationPath}</dd><dt class="text-muted-foreground">Payload</dt><dd>{preview.files.length} files / {(preview.totalBytes / 1024 / 1024).toFixed(2)} MiB</dd><dt class="text-muted-foreground">Ports</dt><dd>Server {preview.serverPort} / txAdmin {preview.txAdminPort}</dd></dl>
			<details><summary class="cursor-pointer text-sm">Files ({preview.files.length})</summary><div class="mt-2 max-h-60 overflow-auto">{#each preview.files as file}<div class="flex justify-between gap-4 border-b border-border py-1 text-xs"><span class="break-all">{file.path}</span><span class="shrink-0 tabular-nums">{file.size} B</span></div>{/each}</div></details>
			<details open={preview.excluded.length > 0}><summary class="cursor-pointer text-sm">Exclusions &amp; sanitized settings ({preview.excluded.length})</summary><div class="mt-2 max-h-60 space-y-2 overflow-auto">{#each preview.excluded as item}<div class="text-xs"><p class="break-all font-medium">{item.path}</p><p class="text-muted-foreground">{item.reason}</p></div>{/each}</div></details>
			{#if preview.database}<div class="space-y-3 border-y border-border py-3"><h4 class="text-sm font-semibold">Database manifest</h4><dl class="grid gap-2 text-xs sm:grid-cols-[7rem_1fr]"><dt class="text-muted-foreground">Source</dt><dd class="break-all">{preview.database.sourceDatabase} / {preview.database.sourcePath}</dd><dt class="text-muted-foreground">Size</dt><dd>{preview.database.sizeBytes} B / {preview.database.tableCount} tables</dd><dt class="text-muted-foreground">SHA-256</dt><dd class="break-all font-mono">{preview.database.sha256}</dd>{#if preview.database.target}<dt class="text-muted-foreground">New target</dt><dd class="break-all">{preview.database.target.database} on {preview.database.target.host}:{preview.database.target.port}</dd>{/if}</dl>{#if preview.database.target}<label class="grid gap-2 text-sm">Confirm new database name<Input bind:value={confirmedDatabase} disabled={blocked} placeholder={preview.database.target.database} /></label><label class="grid gap-2 text-sm">Target database password<Input type="password" bind:value={targetPassword} disabled={blocked} autocomplete="off" /></label><p class="text-xs text-muted-foreground">If import or file promotion fails, cleanup is restricted to this new database after verifying its ownership marker. Credentials are not saved in the package or workspace.</p>{/if}</div>{/if}
			<Notice tone="warn" title="Separate setup required" message="Artifact binaries and txAdmin profiles are not copied. Configure the new txAdmin profile to use the cloned server-data folder, install artifacts, review excluded resource dependencies, and supply new secrets before starting. Only one active server is supported." />
			<label class="grid gap-2 text-sm">Confirm destination path<Input bind:value={confirmedDestination} disabled={blocked} placeholder={preview.destinationPath} /></label>
			<label class="flex items-start gap-2 text-sm"><Checkbox bind:checked={rightsConfirmed} disabled={blocked} /><span>I have permission to make this private copy and have reviewed the manifest and exclusions.</span></label>
			<div class="flex flex-wrap justify-end gap-2"><Button variant="ghost" disabled={blocked} onclick={() => run(clearPreview)}>Revise selection</Button><Button disabled={blocked || !rightsConfirmed || confirmedDestination !== preview.destinationPath || Boolean(preview.database?.target && confirmedDatabase !== preview.database.target.database)} onclick={() => run(copy)}>{#if busy}<LoaderCircleIcon class="size-4 animate-spin" />{:else}<CopyIcon class="size-4" />{/if}{mode === "export" ? "Export local package" : "Create private clone"}</Button></div>
		</div>
	{:else if result}
		<Notice tone="success" title={mode === "export" ? "Local package exported" : "Clone created"} message={`${result.fileCount} files created in ${result.destinationPath}. No server started.${result.database ? ` Database created: ${result.database.database}.` : " No database restored."}${saved ? " Workspace saved without switching." : ""}`} />
		{#if mode !== "export" && !saved}<Button disabled={blocked} onclick={() => run(registerWorkspace)}>Register created workspace</Button>{/if}
	{:else}<div class="flex justify-end"><Button disabled={blocked || !sourcePath || !destinationPath} onclick={() => run(prepare)}>{#if busy}<LoaderCircleIcon class="size-4 animate-spin" />{:else}<ScanSearchIcon class="size-4" />{/if}Preview manifest</Button></div>{/if}
</section>
