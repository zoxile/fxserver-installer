<script lang="ts">
	import PlayIcon from "@lucide/svelte/icons/play";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import RotateCcwIcon from "@lucide/svelte/icons/rotate-cw";
	import ShieldIcon from "@lucide/svelte/icons/shield";
	import SquareIcon from "@lucide/svelte/icons/square";
	import { onMount } from "svelte";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import PasswordInput from "$lib/components/ui/password-input.svelte";
	import { clearFxserverRconPassword, getSavedFxserverRconPassword, saveFxserverRconPassword, sendFxserverCommand, type FxserverRconConfig } from "$lib/modules/fxserver";
	import { loadFxserverSettings, readSavedEnvironment } from "./fxserverSettings.svelte";

	let rcon = $state<FxserverRconConfig>({ host: "127.0.0.1", port: 30120, password: "" });
	let resourceName = $state("");
	let busy = $state(false);
	let message = $state("");
	let error = $state("");
	let recentCommands = $state<string[]>([]);

	onMount(() => {
		void initialize();
	});

	async function initialize() {
		loadFxserverSettings();
		const saved = readSavedEnvironment();
		rcon = {
			host: saved.TXHOST_RCON_HOST || "127.0.0.1",
			port: Number.parseInt(saved.TXHOST_RCON_PORT || "30120", 10) || 30120,
			password: await getSavedFxserverRconPassword(),
		};
	}

	async function runResourceCommand(action: "start" | "stop" | "restart" | "ensure" | "refresh") {
		const resource = resourceName.trim();
		if (!resource) {
			error = "Enter a resource name first.";
			return;
		}

		const command = action === "refresh" ? `refresh\nensure ${resource}` : `${action} ${resource}`;
		busy = true;
		error = "";
		message = "";
		try {
			if (rcon.password.trim()) await saveFxserverRconPassword(rcon.password);
			await sendFxserverCommand(command, rcon);
			recentCommands = [command, ...recentCommands].slice(0, 8);
			message = `Sent RCON command: ${command.replace("\n", " then ")}`;
		} catch (caught) {
			error = caught instanceof Error ? caught.message : String(caught);
		} finally {
			busy = false;
		}
	}

	async function clearPassword() {
		rcon = { ...rcon, password: "" };
		await clearFxserverRconPassword();
		message = "Saved RCON password cleared.";
	}
</script>

<section class="space-y-6">
	<div>
		<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">FXServer</p>
		<h1 class="mt-2 text-3xl font-semibold tracking-normal text-foreground">Resource Manager</h1>
		<p class="mt-2 max-w-2xl text-sm leading-6 text-muted-foreground">Start, stop, restart, refresh, and ensure resources through FXServer RCON.</p>
	</div>

	{#if message}<Notice tone="success" {message} onDismiss={() => (message = "")} />{/if}
	{#if error}<Notice tone="error" message={error} onDismiss={() => (error = "")} />{/if}

	<div class="grid gap-4 xl:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)]">
		<Card.Root class="rounded-md border-border bg-card shadow-sm">
			<Card.Header class="border-b border-border pb-4">
				<Card.Title>RCON Connection</Card.Title>
				<Card.Description>Uses the same saved RCON password as Manage Server.</Card.Description>
			</Card.Header>
			<Card.Content class="space-y-4">
				<div class="grid gap-3 sm:grid-cols-[minmax(0,1fr)_8rem]">
					<label class="grid gap-2">
						<span class="text-xs font-medium text-muted-foreground">Host</span>
						<Input bind:value={rcon.host} placeholder="127.0.0.1" class="rounded-sm font-mono text-xs" />
					</label>
					<label class="grid gap-2">
						<span class="text-xs font-medium text-muted-foreground">Port</span>
						<Input type="number" bind:value={rcon.port} placeholder="30120" class="rounded-sm font-mono text-xs" />
					</label>
				</div>
				<label class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">RCON Password</span>
					<PasswordInput bind:value={rcon.password} placeholder="server.cfg rcon_password" class="rounded-sm font-mono text-xs" />
				</label>
				<Button variant="outline" onclick={clearPassword} disabled={!rcon.password} title="Clear saved RCON password">Clear Saved Password</Button>
			</Card.Content>
		</Card.Root>

		<Card.Root class="rounded-md border-border bg-card shadow-sm">
			<Card.Header class="border-b border-border pb-4">
				<div class="flex items-center gap-3">
					<div class="flex size-9 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
						<ShieldIcon class="size-4" />
					</div>
					<div>
						<Card.Title>Resource Controls</Card.Title>
						<Card.Description>Enter the exact resource folder/name used by FXServer.</Card.Description>
					</div>
				</div>
			</Card.Header>
			<Card.Content class="space-y-4">
				<label class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">Resource</span>
					<Input bind:value={resourceName} placeholder="qbx_core" class="rounded-sm font-mono text-xs" />
				</label>
				<div class="grid gap-2 sm:grid-cols-5">
					<Button onclick={() => runResourceCommand("start")} disabled={busy || !resourceName.trim()} title="Start resource"><PlayIcon />Start</Button>
					<Button variant="destructive" onclick={() => runResourceCommand("stop")} disabled={busy || !resourceName.trim()} title="Stop resource"><SquareIcon />Stop</Button>
					<Button variant="outline" onclick={() => runResourceCommand("restart")} disabled={busy || !resourceName.trim()} title="Restart resource"><RotateCcwIcon />Restart</Button>
					<Button variant="outline" onclick={() => runResourceCommand("ensure")} disabled={busy || !resourceName.trim()} title="Ensure resource"><ShieldIcon />Ensure</Button>
					<Button variant="outline" onclick={() => runResourceCommand("refresh")} disabled={busy || !resourceName.trim()} title="Refresh resources and ensure this one"><RefreshCwIcon />Refresh</Button>
				</div>
			</Card.Content>
		</Card.Root>
	</div>

	<Card.Root class="rounded-md border-border bg-card shadow-sm">
		<Card.Header class="border-b border-border pb-4">
			<Card.Title>Recent Commands</Card.Title>
			<Card.Description>Commands sent from this page during the current app session.</Card.Description>
		</Card.Header>
		<Card.Content>
			{#if recentCommands.length}
				<div class="grid gap-2">
					{#each recentCommands as command}
						<code class="rounded-sm border border-border bg-background/70 px-3 py-2 font-mono text-xs text-foreground">{command}</code>
					{/each}
				</div>
			{:else}
				<p class="text-sm text-muted-foreground">No resource commands sent yet.</p>
			{/if}
		</Card.Content>
	</Card.Root>
</section>
