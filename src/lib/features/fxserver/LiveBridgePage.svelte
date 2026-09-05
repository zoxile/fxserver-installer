<script lang="ts">
	import { onMount } from "svelte";
	import PlugIcon from "@lucide/svelte/icons/plug";
	import DownloadIcon from "@lucide/svelte/icons/download";
	import Trash2Icon from "@lucide/svelte/icons/trash-2";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
	import SearchIcon from "@lucide/svelte/icons/search";
	import PlayIcon from "@lucide/svelte/icons/play";
	import SquareIcon from "@lucide/svelte/icons/square";
	import RotateCwIcon from "@lucide/svelte/icons/rotate-cw";
	import ShieldCheckIcon from "@lucide/svelte/icons/shield-check";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Checkbox } from "$lib/components/ui/checkbox/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import { Separator } from "$lib/components/ui/separator/index.js";
	import { bridgePreference, bridgeSession, saveBridgePreference } from "$lib/core/liveBridge.svelte";
	import { getWorkspaceId } from "$lib/core/workspaces.svelte";
	import { fxserverSettings } from "./fxserverSettings.svelte";
	import { applyBridge, inspectBridge, previewBridge, sendBridgeAction, type BridgeInstallation, type BridgePreview, type BridgeAction } from "$lib/modules/liveBridge";

	const workspaceId = getWorkspaceId();
	const initial = bridgePreference(workspaceId);
	let port = $state(initial.port);
	let autoConnect = $state(initial.enabled);
	let installation = $state<BridgeInstallation | null>(null);
	let preview = $state<BridgePreview | null>(null);
	let busy = $state(false);
	let pendingResourceAction = $state("");
	let error = $state("");
	let message = $state("");
	let safetyDismissed = $state(false);
	let resourceSearch = $state("");
	let playerSearch = $state("");
	let resourceLimit = $state(30);
	let playerLimit = $state(25);
	const snapshot = $derived(bridgeSession.workspaceId === workspaceId && bridgeSession.connected ? bridgeSession.snapshot : null);
	const resources = $derived((snapshot?.resources ?? []).filter((item) => item.name.toLowerCase().includes(resourceSearch.toLowerCase())));
	const players = $derived((snapshot?.players ?? []).filter((item) => `${item.id} ${item.name}`.toLowerCase().includes(playerSearch.toLowerCase())));
	const target = () => ({ workspaceId, txDataPath: fxserverSettings.txDataPath, profile: fxserverSettings.profile, port });
	let active = true;
	onMount(() => { void refresh(); return () => { active = false; }; });
	async function run(action: () => Promise<void>) {
		if (busy) return;
		busy = true; error = ""; message = "";
		try { await action(); } catch (caught) { if (active) error = String(caught); }
		finally { if (active) busy = false; }
	}
	async function refresh() {
		await run(async () => { const result = await inspectBridge(target()); if (active) installation = result; });
	}
	async function review(remove: boolean) {
		await run(async () => { const result = await previewBridge(target(), remove); if (active) preview = result; });
	}
	async function apply() {
		if (!preview) return;
		const selected = preview;
		await run(async () => {
			const result = await applyBridge(selected.id);
			saveBridgePreference(workspaceId, !selected.remove, port);
			if (!active) return;
			installation = result; preview = null; autoConnect = !selected.remove;
			message = selected.remove ? "Bridge removed. Other resources and server settings were kept." : "Bridge installed. Start FXServer to connect.";
		});
	}
	function connect() {
		if (!Number.isInteger(port) || port < 1 || port > 65535) { error = "Enter a port between 1 and 65535."; return; }
		saveBridgePreference(workspaceId, autoConnect, port);
		message = autoConnect ? "Local bridge connection enabled for this workspace." : "Live bridge disconnected.";
	}
	async function control(action: BridgeAction, resource: string) {
		if (["stop", "restart"].includes(action) && !window.confirm(`${action === "stop" ? "Stop" : "Restart"} ${resource}? Connected players may be affected.`)) return;
		pendingResourceAction = `${action}:${resource}`;
		try { await run(async () => { await sendBridgeAction(workspaceId, action, resource); if (active) message = `${action} accepted for ${resource}. Live status will confirm the result.`; }); }
		finally { pendingResourceAction = ""; }
	}
</script>

<div class="space-y-6">
	<header class="flex items-center justify-between gap-3 border-b border-border pb-4">
		<h1 class="text-2xl font-semibold">Live Bridge</h1>
		<span class={`inline-flex shrink-0 items-center gap-2 text-sm ${snapshot ? "text-emerald-400" : "text-muted-foreground"}`}><span class={`size-2 rounded-full ${snapshot ? "bg-emerald-400" : "bg-zinc-500"}`}></span>{snapshot ? "Connected" : "Disconnected"}</span>
	</header>
	{#if !safetyDismissed}<Notice tone="info" title="Local pairing" message="Only authenticated loopback connections are accepted. A server-only pairing file is installed with the resource. Stop FXServer before installing or removing it, and do not share its token file." onDismiss={() => safetyDismissed = true} />{/if}
	{#if error}<Notice tone="error" title="Live Bridge" message={error} onDismiss={() => error = ""} />{/if}
	{#if message}<Notice tone="success" {message} onDismiss={() => message = ""} />{/if}
	<section class="space-y-4">
		<div class="flex flex-wrap items-center justify-between gap-3"><h2 class="text-lg font-semibold">Installation</h2><Button variant="ghost" size="icon" title="Refresh bridge installation" aria-label="Refresh bridge installation" disabled={busy} onclick={refresh}><RefreshCwIcon class="size-4" /></Button></div>
		<p class="break-all text-sm text-muted-foreground">{installation?.resourcePath ?? "Choose a txData folder and profile in Configure Server."}</p>
		{#if installation?.warning}<Notice tone="warn" message={installation.warning} onDismiss={() => { if (installation) installation.warning = null; }} />{/if}
		<div class="flex flex-wrap items-center gap-3">
			<span class="text-sm">{installation?.installed ? `Installed${installation.version ? ` ${installation.version}` : ""}` : "Not installed"}</span>
			{#if installation?.installed}<span class={installation.cfgEnabled && installation.keyAvailable ? "text-xs text-emerald-400" : "text-xs text-amber-400"}>{installation.cfgEnabled && installation.keyAvailable ? "Configured and paired" : "Pairing or configuration needs repair"}</span>{/if}
			<Button variant="outline" disabled={busy || !installation || (installation.installed && !installation.managed)} onclick={() => review(false)}><DownloadIcon class="size-4" />{installation?.installed ? "Re-install" : "Install bridge"}</Button>
			<Button variant="outline" disabled={busy || !installation?.managed} onclick={() => review(true)}><Trash2Icon class="size-4" />Remove bridge</Button>
		</div>
		{#if preview}
			<div class="space-y-3 border-l-2 border-amber-500 pl-4">
				<h3 class="font-medium">{preview.remove ? "Removal preview" : "Installation preview"}</h3>
				<p class="text-sm text-muted-foreground">{preview.files.join(", ")}</p>
				<p class="text-xs font-medium">{preview.remove ? "Remove marked server.cfg block" : "Set marked server.cfg block"}</p>
				<pre class="max-h-52 overflow-auto whitespace-pre-wrap break-all bg-muted p-3 text-xs">{preview.configLines.join("\n")}</pre>
				<div class="flex gap-2"><Button disabled={busy} onclick={apply}>{#if busy}<LoaderCircleIcon class="size-4 animate-spin" />{/if}Confirm {preview.remove ? "removal" : "installation"}</Button><Button variant="ghost" disabled={busy} onclick={() => preview = null}>Cancel</Button></div>
			</div>
		{/if}
	</section>
	<Separator />
	<section class="space-y-4">
		<h2 class="text-lg font-semibold">Connection</h2>
		<div class="flex flex-wrap items-end gap-4">
			<label class="grid w-36 gap-2 text-sm">FXServer HTTP port<Input type="number" min={1} max={65535} bind:value={port} /></label>
			<label class="flex h-9 items-center gap-2 text-sm"><Checkbox bind:checked={autoConnect} />Connect automatically</label>
			<Button variant="outline" disabled={busy || !installation?.keyAvailable} onclick={connect}><PlugIcon class="size-4" />Apply connection</Button>
		</div>
		{#if bridgeSession.error && bridgeSession.enabled}<p class="text-sm text-amber-300" role="status">{bridgeSession.error}</p>{/if}
	</section>
	{#if snapshot}
		<Separator />
		<section class="space-y-4">
			<h2 class="break-words text-lg font-semibold">{snapshot.hostname}</h2>
			<dl class="grid grid-cols-2 gap-4 text-sm sm:grid-cols-3 lg:grid-cols-6">
				{#each [["Players", `${snapshot.playerCount} / ${snapshot.maxPlayers}`], ["Resources", snapshot.resourceCount], ["Bridge uptime", `${Math.floor(snapshot.uptimeSeconds / 60)} min`], ["Scheduler delay", `${Math.round(snapshot.schedulerDelayMs)} ms`], ["Game build", snapshot.gameBuild], ["OneSync", snapshot.onesync]] as item}<div class="min-w-0"><dt class="text-xs text-muted-foreground">{item[0]}</dt><dd class="mt-1 break-all font-medium">{item[1]}</dd></div>{/each}
			</dl>
		</section>
		<Separator />
		<section class="space-y-3">
			<div class="flex flex-wrap items-center justify-between gap-3"><h2 class="text-lg font-semibold">Resources</h2><div class="relative w-64 max-w-full"><SearchIcon class="pointer-events-none absolute top-2.5 left-3 size-4 text-muted-foreground" /><Input class="pl-9" aria-label="Search live resources" placeholder="Search resources" bind:value={resourceSearch} /></div></div>
			<div class="divide-y divide-border border-y border-border">
				{#each resources.slice(0, resourceLimit) as resource (resource.name)}
					<div class="flex flex-wrap items-center gap-3 py-2"><div class="min-w-0 flex-1 basis-40"><p class="break-all text-sm font-medium">{resource.name}</p><span class={`text-xs ${resource.state === "started" ? "text-emerald-400" : resource.state === "stopped" ? "text-red-400" : "text-amber-400"}`}>{resource.state}</span></div><span class="text-xs text-muted-foreground">{resource.version}</span><div class="flex gap-1">
						{#each [{ action: "start", icon: PlayIcon }, { action: "stop", icon: SquareIcon }, { action: "restart", icon: RotateCwIcon }, { action: "ensure", icon: ShieldCheckIcon }] as controlItem}<Button variant="ghost" size="icon" title={`${controlItem.action} ${resource.name}`} aria-label={`${controlItem.action} ${resource.name}`} disabled={busy || resource.name === "fxserver_installer_bridge"} onclick={() => control(controlItem.action as BridgeAction, resource.name)}>{#if pendingResourceAction === `${controlItem.action}:${resource.name}`}<LoaderCircleIcon class="size-4 animate-spin" />{:else}<controlItem.icon class="size-4" />{/if}</Button>{/each}
					</div></div>
				{/each}
			</div>
			{#if resources.length > resourceLimit}<Button variant="outline" onclick={() => resourceLimit += 30}>Show more resources</Button>{/if}
		</section>
		<Separator />
		<section class="space-y-3">
			<div class="flex flex-wrap items-center justify-between gap-3"><h2 class="text-lg font-semibold">Players</h2><Input class="w-64 max-w-full" aria-label="Search live players" placeholder="Search players" bind:value={playerSearch} /></div>
			<div class="overflow-auto"><table class="w-full text-left text-sm"><thead class="text-xs text-muted-foreground"><tr><th class="w-20 py-2">ID</th><th>Name</th><th class="w-24 text-right">Ping</th></tr></thead><tbody>{#each players.slice(0, playerLimit) as player (player.id)}<tr class="border-t border-border"><td class="py-2">{player.id}</td><td class="max-w-64 break-words">{player.name}</td><td class="text-right tabular-nums">{player.ping} ms</td></tr>{/each}</tbody></table></div>
			{#if !players.length}<p class="text-sm text-muted-foreground">No players to display.</p>{/if}
			{#if players.length > playerLimit}<Button variant="outline" onclick={() => playerLimit += 25}>Show more players</Button>{/if}
		</section>
		<Separator />
		<section class="space-y-3"><h2 class="text-lg font-semibold">Recent Resource Events</h2><ol class="max-h-72 overflow-auto text-sm">{#each [...snapshot.events].reverse().slice(0, 50) as event (event.id)}<li class="flex gap-3 border-b border-border py-2"><time class="shrink-0 text-xs text-muted-foreground">{new Date(event.timestamp).toLocaleTimeString()}</time><span class="break-all">{event.resource}: {event.kind.replaceAll("-", " ")}</span></li>{/each}</ol></section>
	{/if}
</div>
