<script lang="ts">
	import AlertCircleIcon from "@lucide/svelte/icons/alert-circle";
	import CheckCircle2Icon from "@lucide/svelte/icons/check-circle-2";
	import FileTextIcon from "@lucide/svelte/icons/file-text";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import KeyRoundIcon from "@lucide/svelte/icons/key-round";
	import ListPlusIcon from "@lucide/svelte/icons/list-plus";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import SaveIcon from "@lucide/svelte/icons/save";
	import SearchIcon from "@lucide/svelte/icons/search";
	import Undo2Icon from "@lucide/svelte/icons/undo-2";
	import { onMount } from "svelte";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import * as Select from "$lib/components/ui/select/index.js";
	import { chooseFolder } from "$lib/core/selectFolder";
	import { readServerConfig, saveServerConfig, type ServerConfigFile, type ServerConfigResult } from "$lib/modules/fxserver";
	import { fxserverSettings, loadFxserverSettings, refreshTxDataProfiles, setServerProfile, setTxDataPath } from "./fxserverSettings.svelte";

	let dataPath = $state("");
	let profile = $state("");
	let result = $state<ServerConfigResult | null>(null);
	let selectedPath = $state("");
	let editorContent = $state("");
	let query = $state("");
	let busy = $state(false);
	let saving = $state(false);
	let notice = $state("");
	let noticeLevel = $state<"success" | "error">("success");
	let drafts = $state<Record<string, string>>({});
	let editorElement = $state<HTMLTextAreaElement | null>(null);
	let gutterElement = $state<HTMLDivElement | null>(null);

	const profileOptions = $derived(fxserverSettings.profiles.map((profileName) => ({ value: profileName, label: profileName })));
	const filteredFiles = $derived(
		(result?.files ?? []).filter((file) => {
			const haystack = `${file.name} ${file.path} ${file.content}`.toLowerCase();
			return !query.trim() || haystack.includes(query.trim().toLowerCase());
		}),
	);
	const selectedFile = $derived((result?.files ?? []).find((file) => file.path === selectedPath) ?? null);
	const dirty = $derived(Boolean(selectedFile && editorContent !== selectedFile.content));
	const lineNumbers = $derived(Array.from({ length: Math.max(editorContent.split("\n").length, 1) }, (_, index) => index + 1));
	const rconLineInSelectedFile = $derived(selectedFile && result?.rconPasswordFile === selectedFile.name ? (result.rconPasswordLine ?? null) : null);
	const rconlogLineInSelectedFile = $derived(selectedFile?.name.toLowerCase() === "server.cfg" ? (result?.rconlogLine ?? null) : null);
	const stats = $derived({
		files: result?.files.length ?? 0,
		lines: editorContent ? editorContent.split("\n").length : 0,
		chars: editorContent.length,
	});
	const rconReady = $derived(Boolean(result?.rconPasswordFound && result?.rconlogFound));
	const rconCommands = [
		{ command: "say <message>", description: "Send a chat message to all players." },
		{ command: "start <resource-name>", description: "Start a server resource." },
		{ command: "stop <resource-name>", description: "Stop a server resource." },
		{ command: "restart <resource-name>", description: "Restart a server resource." },
		{ command: "ensure <resource-name>", description: "Start a resource if it is not already running." },
		{ command: "refresh", description: "Reload resources from the resource directory." },
		{ command: "clear", description: "Clear server console output." },
		{ command: "quit", description: "Shut down the server cleanly." },
	];

	onMount(() => {
		loadFxserverSettings();
		dataPath = fxserverSettings.txDataPath;
		profile = fxserverSettings.profile;
		void (async () => {
			await refreshTxDataProfiles();
			dataPath = fxserverSettings.txDataPath;
			profile = fxserverSettings.profile;
			if (dataPath.trim() && profile.trim()) {
				await loadConfig();
			}
		})();
	});

	async function loadConfig() {
		busy = true;
		notice = "";
		setTxDataPath(dataPath);
		setServerProfile(profile);

		try {
			const nextResult = await readServerConfig({
				txDataPath: dataPath.trim(),
				profile: profile.trim(),
			});
			result = nextResult;
			drafts = {};
			selectFile(nextResult.files.find((file) => file.name === "server.cfg")?.path ?? nextResult.files[0]?.path ?? "", false);
			notice = `Loaded ${nextResult.files.length} config file${nextResult.files.length === 1 ? "" : "s"}.`;
			noticeLevel = "success";
		} catch (error) {
			result = null;
			selectedPath = "";
			editorContent = "";
			notice = error instanceof Error ? error.message : String(error);
			noticeLevel = "error";
		} finally {
			busy = false;
		}
	}

	async function chooseTxDataFolder() {
		notice = "";
		const selectedFolder = await chooseFolder();
		if (!selectedFolder) return;

		dataPath = selectedFolder;
		profile = "";
		result = null;
		setTxDataPath(selectedFolder);
		setServerProfile("");
		await refreshTxDataProfiles();
	}

	async function handleTxDataChange(event: Event) {
		dataPath = (event.currentTarget as HTMLInputElement).value;
		profile = "";
		result = null;
		setTxDataPath(dataPath);
		setServerProfile("");
		await refreshTxDataProfiles();
	}

	function handleProfileChange(nextProfile: string) {
		profile = nextProfile;
		setServerProfile(nextProfile);
		if (dataPath.trim() && nextProfile.trim()) {
			void loadConfig();
		}
	}

	function selectFile(path: string, keepCurrentDraft = true) {
		if (!path) {
			selectedPath = "";
			editorContent = "";
			return;
		}

		if (keepCurrentDraft && selectedPath) {
			drafts = { ...drafts, [selectedPath]: editorContent };
		}

		const file = result?.files.find((item) => item.path === path);
		selectedPath = path;
		editorContent = drafts[path] ?? file?.content ?? "";
	}

	function revertFile() {
		if (!selectedFile) return;
		const { [selectedFile.path]: _removed, ...remainingDrafts } = drafts;
		drafts = remainingDrafts;
		editorContent = selectedFile.content;
		notice = `${selectedFile.name} reverted.`;
		noticeLevel = "success";
	}

	async function saveFile() {
		if (!selectedFile) return;

		saving = true;
		notice = "";

		try {
			const savedFile = await saveServerConfig(selectedFile.path, editorContent);
			const nextFiles = result?.files.map((file) => (file.path === savedFile.path ? savedFile : file)) ?? [];
			const rcon = findRconPassword(nextFiles);
			const rconlog = findRconlog(nextFiles);
			result = result
				? {
						...result,
						files: nextFiles,
						rconPasswordFound: Boolean(rcon),
						rconPasswordFile: rcon?.file.name ?? null,
						rconPasswordLine: rcon?.line ?? null,
						rconlogFound: Boolean(rconlog),
						rconlogLine: rconlog?.line ?? null,
					}
				: result;
			drafts = { ...drafts, [savedFile.path]: savedFile.content };
			editorContent = savedFile.content;
			notice = `${savedFile.name} saved.`;
			noticeLevel = "success";
		} catch (error) {
			notice = error instanceof Error ? error.message : String(error);
			noticeLevel = "error";
		} finally {
			saving = false;
		}
	}

	function findRconPassword(files: ServerConfigFile[]) {
		for (const file of files.filter((item) => item.name.toLowerCase() === "server.cfg")) {
			const index = file.content.split("\n").findIndex((line) => isRconPasswordLine(line));
			if (index >= 0) return { file, line: index + 1 };
		}

		return null;
	}

	function findRconlog(files: ServerConfigFile[]) {
		for (const file of files.filter((item) => item.name.toLowerCase() === "server.cfg")) {
			const index = file.content.split("\n").findIndex((line) => isRconlogLine(line));
			if (index >= 0) return { file, line: index + 1 };
		}

		return null;
	}

	function isRconPasswordLine(line: string) {
		const trimmed = line.trimStart();
		if (trimmed.startsWith("#") || trimmed.startsWith("//")) return false;
		const lower = trimmed.toLowerCase();
		return lower.startsWith("rcon_password") || lower.startsWith("set rcon_password");
	}

	function isRconlogLine(line: string) {
		const trimmed = line.trimStart();
		if (trimmed.startsWith("#") || trimmed.startsWith("//")) return false;
		const [command, resource, extra] = trimmed.split(/\s+/);
		return command?.toLowerCase() === "ensure" && resource?.toLowerCase() === "rconlog" && !extra;
	}

	function selectServerCfg() {
		const serverCfg = result?.files.find((file) => file.name.toLowerCase() === "server.cfg");
		if (serverCfg) selectFile(serverCfg.path);
		return serverCfg;
	}

	function autofillRconConfig() {
		const serverCfg = selectServerCfg();
		if (!serverCfg) {
			notice = "server.cfg was not found in the resolved server data path.";
			noticeLevel = "error";
			return;
		}

		const currentContent = drafts[serverCfg.path] ?? serverCfg.content;
		const lines = currentContent.split("\n");
		const additions = [];
		if (!lines.some(isRconlogLine)) additions.push("ensure rconlog");
		if (!lines.some(isRconPasswordLine)) additions.push('set rcon_password "your-secure-password"');

		if (!additions.length) {
			notice = "server.cfg already has the RCON setup lines.";
			noticeLevel = "success";
			return;
		}

		const base = currentContent.trimEnd();
		editorContent = `${base}${base ? "\n\n" : ""}# RCON setup\n${additions.join("\n")}\n`;
		drafts = { ...drafts, [serverCfg.path]: editorContent };
		notice = "Added missing RCON setup lines to server.cfg. Replace the password before saving.";
		noticeLevel = "success";
	}

	function autofillCommand(command: string) {
		const serverCfg = selectServerCfg();
		if (!serverCfg) {
			notice = "server.cfg was not found in the resolved server data path.";
			noticeLevel = "error";
			return;
		}

		const base = (drafts[serverCfg.path] ?? serverCfg.content).trimEnd();
		editorContent = `${base}${base ? "\n\n" : ""}${command}\n`;
		drafts = { ...drafts, [serverCfg.path]: editorContent };
		notice = `${command} added to server.cfg.`;
		noticeLevel = "success";
	}

	function formatModified(file: ServerConfigFile) {
		if (!file.modified) return "Unknown";
		return new Date(file.modified * 1000).toLocaleString(undefined, {
			month: "short",
			day: "2-digit",
			hour: "2-digit",
			minute: "2-digit",
		});
	}

	function syncLineNumbers() {
		if (!editorElement || !gutterElement) return;
		gutterElement.scrollTop = editorElement.scrollTop;
	}
</script>

<section class="space-y-6">
	<div class="flex flex-col justify-between gap-4 lg:flex-row lg:items-end">
		<div>
			<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">FXServer</p>
			<h1 class="mt-2 text-3xl font-semibold tracking-normal text-foreground">Configure Server</h1>
			<p class="mt-2 max-w-2xl text-sm leading-6 text-muted-foreground">Edit the `.cfg` files from the server data path resolved through your selected txData profile.</p>
		</div>
		<Button variant="outline" onclick={loadConfig} disabled={busy || !dataPath.trim() || !profile.trim()} title="Reload config files from disk">
			<RefreshCwIcon class={busy ? "animate-spin" : undefined} />
			Reload
		</Button>
	</div>

	<Card.Root class="overflow-hidden rounded-sm border-border bg-card shadow-sm">
		<Card.Header class="border-b border-border pb-4">
			<div class="flex flex-col gap-4 xl:flex-row xl:items-end xl:justify-between">
				<div class="min-w-0">
					<Card.Title>Profile Source</Card.Title>
					<Card.Description class="mt-1 truncate font-mono text-xs">
						{result?.profileConfigPath ?? (dataPath.trim() && profile.trim() ? `${dataPath}\\${profile}\\config.json` : "Choose txData and profile")}
					</Card.Description>
				</div>
				<div class="grid gap-2 sm:grid-cols-[minmax(0,22rem)_auto]">
					<Input bind:value={dataPath} onchange={handleTxDataChange} placeholder="C:\FiveM\txData" title="Folder containing txAdmin profile folders." class="rounded-sm font-mono text-xs" />
					<Button variant="outline" onclick={chooseTxDataFolder} title="Browse for the txData folder">
						<FolderOpenIcon />
						Browse
					</Button>
				</div>
			</div>
		</Card.Header>
		<Card.Content class="space-y-4">
			<div class="grid gap-3 lg:grid-cols-[minmax(0,0.55fr)_minmax(0,1fr)_auto] lg:items-end">
				<label class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">Profile</span>
					<Select.Root bind:value={profile} type="single" items={profileOptions} onValueChange={handleProfileChange}>
						<Select.Trigger title="Choose the txData profile folder" class="w-full rounded-sm font-mono text-xs">
							{profile || "Choose profile"}
						</Select.Trigger>
						<Select.Content class="rounded-sm">
							{#if profileOptions.length}
								{#each profileOptions as option}
									<Select.Item value={option.value} label={option.label}>
										{option.label}
									</Select.Item>
								{/each}
							{:else}
								<Select.Item value="" label="No profiles detected" disabled>No profiles detected</Select.Item>
							{/if}
						</Select.Content>
					</Select.Root>
				</label>
				<div class="rounded-sm border border-border bg-background/60 px-3 py-2">
					<p class="text-xs text-muted-foreground">Resolved Server Data Path</p>
					<p class="mt-1 truncate font-mono text-xs text-foreground">{result?.dataPath ?? "Load a profile to resolve dataPath from config.json."}</p>
				</div>
				<div class="rounded-sm border border-border bg-background/60 px-3 py-2 text-right">
					<p class="text-xs text-muted-foreground">Config Files</p>
					<p class="mt-1 font-mono text-sm font-semibold text-foreground">{stats.files}</p>
				</div>
			</div>

			{#if fxserverSettings.profileError}
				<div class="rounded-sm border border-red-400/30 bg-red-400/10 px-3 py-2 text-xs text-red-100">{fxserverSettings.profileError}</div>
			{/if}

			{#if notice}
				<div class={`rounded-sm border px-3 py-2 text-xs ${noticeLevel === "success" ? "border-emerald-400/30 bg-emerald-400/10 text-emerald-100" : "border-red-400/30 bg-red-400/10 text-red-100"}`}>
					<div class="flex items-start gap-2">
						{#if noticeLevel === "success"}
							<CheckCircle2Icon class="mt-0.5 size-3.5 shrink-0" />
						{:else}
							<AlertCircleIcon class="mt-0.5 size-3.5 shrink-0" />
						{/if}
						<p>{notice}</p>
					</div>
				</div>
			{/if}
		</Card.Content>
	</Card.Root>

	{#if result}
		{#if rconReady}
			<div class="rounded-sm border border-emerald-400/30 bg-emerald-400/10 px-4 py-3 text-sm text-emerald-100">
				<div class="flex items-start gap-2">
					<KeyRoundIcon class="mt-0.5 size-4 shrink-0" />
					<p>
						RCON is configured in `server.cfg`: `ensure rconlog`{#if result.rconlogLine} on line {result.rconlogLine}{/if} and `rcon_password`{#if result.rconPasswordLine} on line
							{result.rconPasswordLine}{/if}. Use that password in Manage Server's Console card.
					</p>
				</div>
			</div>
		{:else}
			<div class="rounded-sm border border-red-400/30 bg-red-400/10 px-4 py-3 text-sm text-red-100">
				<div class="flex items-start gap-2">
					<AlertCircleIcon class="mt-0.5 size-4 shrink-0" />
					<p>
						`server.cfg` is missing {[
							!result.rconlogFound ? "`ensure rconlog`" : "",
							!result.rconPasswordFound ? "`set rcon_password \"your-secure-password\"`" : "",
						]
							.filter(Boolean)
							.join(" and ")}. Add the missing line{!result.rconlogFound && !result.rconPasswordFound ? "s" : ""} before using command input in Manage Server's Console card.
					</p>
				</div>
			</div>
		{/if}

		<Card.Root class="overflow-hidden rounded-sm border-border bg-card shadow-sm">
			<Card.Header class="border-b border-border pb-4">
				<div class="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
					<div>
						<Card.Title>RCON Autofill</Card.Title>
						<Card.Description>Add the recommended RCON setup lines and common command snippets to `server.cfg`.</Card.Description>
					</div>
					<Button variant="outline" onclick={autofillRconConfig} title="Add missing RCON setup lines to server.cfg">
						<ListPlusIcon />
						Add RCON Setup
					</Button>
				</div>
			</Card.Header>
			<Card.Content class="space-y-3">
				<div class="grid gap-2 md:grid-cols-2 xl:grid-cols-4">
					{#each rconCommands as item}
						<button
							type="button"
							class="rounded-sm border border-border bg-background/60 px-3 py-2 text-left transition-colors hover:bg-accent"
							onclick={() => autofillCommand(item.command)}
							title={`Add ${item.command} to server.cfg`}
						>
							<span class="block font-mono text-xs text-foreground">{item.command}</span>
							<span class="mt-1 block text-xs leading-5 text-muted-foreground">{item.description}</span>
						</button>
					{/each}
				</div>
			</Card.Content>
		</Card.Root>

		<div class="grid gap-4 xl:grid-cols-[20rem_minmax(0,1fr)]">
			<Card.Root class="overflow-hidden rounded-sm border-border bg-card shadow-sm">
				<Card.Header class="border-b border-border pb-4">
					<Card.Title>Config Files</Card.Title>
					<Card.Description>{filteredFiles.length} visible of {result.files.length} `.cfg` files.</Card.Description>
				</Card.Header>
				<Card.Content class="space-y-3">
					<div class="relative">
						<SearchIcon class="pointer-events-none absolute top-1/2 left-3 size-3.5 -translate-y-1/2 text-muted-foreground" />
						<Input bind:value={query} placeholder="Search files and content..." title="Filter config files." class="rounded-sm pl-9" />
					</div>
					<div class="max-h-152 overflow-auto rounded-sm border border-border bg-background/50">
						{#if filteredFiles.length}
							{#each filteredFiles as file (file.path)}
								<button
									type="button"
									class={`flex w-full items-start gap-3 border-b border-border/70 px-3 py-3 text-left transition-colors last:border-b-0 hover:bg-accent ${selectedPath === file.path ? "bg-accent text-accent-foreground" : ""}`}
									onclick={() => selectFile(file.path)}
									title={file.path}
								>
									<FileTextIcon class={`mt-0.5 size-4 shrink-0 ${file.hasRconPassword || file.hasRconlog ? "text-amber-200" : "text-muted-foreground"}`} />
									<span class="min-w-0 flex-1">
										<span class="flex items-center gap-2">
											<span class="truncate text-sm font-medium">{file.name}</span>
											{#if drafts[file.path] !== undefined && drafts[file.path] !== file.content}
												<span class="rounded-xs border border-primary/30 bg-primary/10 px-1.5 py-0.5 text-[10px] text-primary">edited</span>
											{/if}
										</span>
										<span class="mt-1 block truncate font-mono text-[11px] text-muted-foreground">{formatModified(file)}</span>
									</span>
								</button>
							{/each}
						{:else}
							<div class="px-3 py-8 text-center text-sm text-muted-foreground">No config files match the current search.</div>
						{/if}
					</div>
				</Card.Content>
			</Card.Root>

			<Card.Root class="overflow-hidden rounded-sm border-border bg-card shadow-sm">
				<Card.Header class="border-b border-border pb-4">
					<div class="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
						<div class="min-w-0">
							<Card.Title>{selectedFile?.name ?? "No File Selected"}</Card.Title>
							<Card.Description class="mt-1 truncate font-mono text-xs">{selectedFile?.path ?? "Choose a config file to edit."}</Card.Description>
						</div>
						<div class="flex flex-wrap gap-2">
							<Button variant="outline" onclick={revertFile} disabled={!dirty} title="Discard unsaved changes in this file">
								<Undo2Icon />
								Revert
							</Button>
							<Button onclick={saveFile} disabled={!selectedFile || !dirty || saving} title="Save this config file">
								<SaveIcon />
								{saving ? "Saving" : "Save"}
							</Button>
						</div>
					</div>
				</Card.Header>
				<Card.Content class="space-y-3">
					<div class="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
						<span class="rounded-sm border border-border bg-background/70 px-2 py-1">{stats.lines} lines</span>
						<span class="rounded-sm border border-border bg-background/70 px-2 py-1">{stats.chars} characters</span>
						{#if dirty}
							<span class="rounded-sm border border-primary/30 bg-primary/10 px-2 py-1 text-primary">Unsaved changes</span>
						{/if}
						{#if rconLineInSelectedFile}
							<span class="rounded-sm border border-amber-400/30 bg-amber-400/10 px-2 py-1 text-amber-100">rcon_password line {rconLineInSelectedFile}</span>
						{/if}
						{#if rconlogLineInSelectedFile}
							<span class="rounded-sm border border-emerald-400/30 bg-emerald-400/10 px-2 py-1 text-emerald-100">rconlog line {rconlogLineInSelectedFile}</span>
						{/if}
					</div>
					<div class="grid h-160 grid-cols-[3.5rem_minmax(0,1fr)] overflow-hidden rounded-sm border border-border bg-background/70 font-mono text-xs">
						<div bind:this={gutterElement} class="overflow-hidden border-r border-border bg-muted/30 py-3 text-right text-muted-foreground select-none">
							{#each lineNumbers as line}
								<div class={`h-5 px-2 leading-5 ${line === rconLineInSelectedFile ? "bg-amber-400/20 text-amber-100" : line === rconlogLineInSelectedFile ? "bg-emerald-400/20 text-emerald-100" : ""}`}>{line}</div>
							{/each}
						</div>
						<textarea
							bind:this={editorElement}
							bind:value={editorContent}
							disabled={!selectedFile}
							spellcheck="false"
							onscroll={syncLineNumbers}
							class="h-160 resize-none overflow-auto bg-transparent px-3 py-3 leading-5 text-foreground outline-none placeholder:text-muted-foreground disabled:cursor-not-allowed disabled:opacity-60"
							placeholder="Select a .cfg file to edit..."
							title="Server config editor"
						></textarea>
					</div>
				</Card.Content>
			</Card.Root>
		</div>
	{/if}
</section>
