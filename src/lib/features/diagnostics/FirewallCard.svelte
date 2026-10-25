<script lang="ts">
	import { onDestroy } from "svelte";
	import ShieldCheckIcon from "@lucide/svelte/icons/shield-check";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
	import Trash2Icon from "@lucide/svelte/icons/trash-2";
	import FileSearchIcon from "@lucide/svelte/icons/file-search";
	import * as Card from "$lib/components/ui/card/index.js";
	import * as Select from "$lib/components/ui/select/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import { fxserverSettings } from "$lib/features/fxserver/fxserverSettings.svelte";
	import { configureFirewall, firewallStateLabels, getFirewallPorts, inspectFirewall, removeFirewall, validFirewallPort, type FirewallBackend, type FirewallSnapshot } from "$lib/modules/firewall";

	let { linux = false }: { linux?: boolean } = $props();
	let tcpPort = $state<number | undefined>(30120);
	let udpPort = $state<number | undefined>(30120);
	let backend = $state<FirewallBackend>("auto");
	let snapshot = $state<FirewallSnapshot | null>(null);
	let busy = $state("");
	let error = $state("");
	let message = $state("");
	let portWarnings = $state<string[]>([]);
	let portSource = $state("Default game ports; confirm your server's configured ports.");
	let confirmation = $state<"configure" | "remove" | null>(null);
	let active = true;
	const backends = $derived([
		{ value: "auto", label: "Detect automatically" },
		...(linux ? [{ value: "ufw", label: "UFW" }, { value: "firewalld", label: "firewalld" }] : [{ value: "windows", label: "Windows Firewall" }]),
	]);
	const selection = $derived(JSON.stringify([tcpPort, udpPort, backend, fxserverSettings.txDataPath, fxserverSettings.profile]));
	const validPorts = $derived(validFirewallPort(tcpPort) && validFirewallPort(udpPort));
	$effect(() => { selection; snapshot = null; confirmation = null; message = ""; });
	$effect(() => {
		fxserverSettings.txDataPath; fxserverSettings.profile;
		portWarnings = [];
		portSource = "Confirm the ports for this profile, or read them from its configuration.";
	});
	onDestroy(() => { active = false; });

	async function loadPorts() {
		if (busy || !fxserverSettings.profile || !fxserverSettings.txDataPath) return;
		busy = "ports";
		error = "";
		const context = selection;
		try {
			const result = await getFirewallPorts(fxserverSettings.txDataPath, fxserverSettings.profile);
			if (!active || context !== selection) return;
			portWarnings = result.warnings;
			if (result.tcpPorts.length === 1) tcpPort = result.tcpPorts[0];
			if (result.udpPorts.length === 1) udpPort = result.udpPorts[0];
			portSource = `Profile ports: TCP ${result.tcpPorts.join(", ") || "not found"}; UDP ${result.udpPorts.join(", ") || "not found"}.`;
		} catch (caught) { if (active && context === selection) error = String(caught); }
		finally { busy = ""; }
	}

	async function run(action: "inspect" | "elevated" | "configure" | "remove") {
		if (busy || !validFirewallPort(tcpPort) || !validFirewallPort(udpPort)) return;
		if (action === "configure" && !snapshot?.canConfigure || action === "remove" && !snapshot?.canRemove) return;
		const target = { tcpPort, udpPort, backend };
		const context = selection;
		busy = action;
		error = "";
		message = "";
		confirmation = null;
		try {
			const result = action === "configure" ? await configureFirewall(target) : action === "remove" ? await removeFirewall(target) : await inspectFirewall(target, action === "elevated");
			if (!active || context !== selection) return;
			snapshot = result;
			if (action === "configure") message = "Firewall action finished. Review the refreshed status below for any remaining restrictions.";
			if (action === "remove") message = "Removal finished. Existing external rules and default policies were left unchanged.";
		} catch (caught) { if (active && context === selection) { error = String(caught); snapshot = null; } }
		finally { busy = ""; }
	}
</script>

<Card.Root aria-label="FiveM firewall">
	<Card.Header class="border-b border-border">
		<div class="flex flex-wrap items-center justify-between gap-3">
			<div class="flex items-center gap-3"><ShieldCheckIcon class="size-5 text-muted-foreground" /><Card.Title>FiveM Firewall</Card.Title></div>
			<Button size="sm" variant="outline" onclick={() => run("inspect")} disabled={!!busy || !validPorts}>
				{#if busy === "inspect"}<LoaderCircleIcon class="animate-spin" />{:else}<RefreshCwIcon />{/if}Check firewall
			</Button>
		</div>
	</Card.Header>
	<Card.Content class="space-y-4">
		<div class="grid gap-3 sm:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_minmax(12rem,1.4fr)]">
			<label class="space-y-1.5 text-sm" for="firewall-tcp"><span>TCP game port</span><Input id="firewall-tcp" type="number" min={1} max={65535} step={1} bind:value={tcpPort} disabled={!!busy} /></label>
			<label class="space-y-1.5 text-sm" for="firewall-udp"><span>UDP game port</span><Input id="firewall-udp" type="number" min={1} max={65535} step={1} bind:value={udpPort} disabled={!!busy} /></label>
			<div class="space-y-1.5 text-sm"><label for="firewall-backend">Firewall</label><Select.Root type="single" bind:value={backend} disabled={!!busy}>
				<Select.Trigger id="firewall-backend" class="w-full">{backends.find((item) => item.value === backend)?.label ?? backend}</Select.Trigger>
				<Select.Content>{#each backends as item}<Select.Item value={item.value}>{item.label}</Select.Item>{/each}</Select.Content>
			</Select.Root></div>
		</div>
		<div class="flex flex-wrap items-center justify-between gap-2">
			<p class="min-w-0 text-xs text-muted-foreground wrap-anywhere">{portSource}</p>
			<Button size="sm" variant="ghost" onclick={loadPorts} disabled={!!busy || !fxserverSettings.profile || !fxserverSettings.txDataPath}>
				{#if busy === "ports"}<LoaderCircleIcon class="animate-spin" />{:else}<FileSearchIcon />{/if}Read profile ports
			</Button>
		</div>
		{#if !validPorts}<p class="text-sm text-red-300" role="alert">Both ports must be whole numbers from 1 to 65535.</p>{/if}
		{#each portWarnings as warning}<Notice tone="warn" message={warning} />{/each}
		{#if error}<Notice tone="error" title="Firewall action failed" message={error} onDismiss={() => error = ""} />{/if}
		{#if message}<Notice tone="success" message={message} onDismiss={() => message = ""} />{/if}
		{#if snapshot}
			<div class="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
				<span>{backends.find((item) => item.value === snapshot?.backend)?.label ?? "Unsupported firewall"}</span>
				<span aria-hidden="true">/</span><span>{snapshot.active === true ? "Active" : snapshot.active === false ? "Inactive" : "Activity not confirmed"}</span>
			</div>
			<div class="overflow-x-auto">
				<table class="w-full text-left text-sm">
					<thead class="border-b border-border text-xs text-muted-foreground"><tr><th class="py-2 pr-3 font-medium">Direction</th><th class="py-2 pr-3 font-medium">Protocol / port</th><th class="py-2 pr-3 font-medium">Status</th><th class="py-2 font-medium">Details</th></tr></thead>
					<tbody>{#each snapshot.checks as check}
						<tr class="border-b border-border/60 last:border-0">
							<td class="py-3 pr-3 capitalize">{check.direction}</td><td class="py-3 pr-3 whitespace-nowrap tabular-nums">{check.protocol.toUpperCase()} / {check.port}</td>
							<td class="py-3 pr-3"><span class={check.state === "blocked" ? "text-red-300" : ["unknown", "missing"].includes(check.state) ? "text-amber-300" : "text-emerald-300"}>{firewallStateLabels[check.state]}</span></td>
							<td class="max-w-lg py-3 text-xs leading-5 text-muted-foreground">{check.detail}</td>
						</tr>
					{/each}</tbody>
				</table>
			</div>
			{#each snapshot.notices as notice}<p class="max-w-4xl text-xs leading-5 text-muted-foreground">{notice}</p>{/each}
			{#if snapshot.reloadRequired}<Notice tone="warn" message="Persistent rules differ from the active firewall. Review the firewall's pending changes before reloading it; this app will not reload unrelated rules automatically." />{/if}
			<div class="flex flex-wrap gap-2">
				{#if snapshot.requiresElevation}<Button size="sm" variant="outline" disabled={!!busy} onclick={() => run("elevated")}>{#if busy === "elevated"}<LoaderCircleIcon class="animate-spin" />{:else}<ShieldCheckIcon />{/if}Check with administrator permission</Button>{/if}
				<Button size="sm" disabled={!!busy || !snapshot.canConfigure} onclick={() => confirmation = "configure"}>{#if busy === "configure"}<LoaderCircleIcon class="animate-spin" />{:else}<ShieldCheckIcon />{/if}Set up FiveM rules</Button>
				<Button size="sm" variant="outline" disabled={!!busy || !snapshot.canRemove} onclick={() => confirmation = "remove"}>{#if busy === "remove"}<LoaderCircleIcon class="animate-spin" />{:else}<Trash2Icon />{/if}Remove app rules</Button>
			</div>
		{:else if !busy}
			<p class="text-sm text-muted-foreground">Not checked. No firewall rules have been changed.</p>
		{/if}
		{#if confirmation}
			<div class="space-y-3 border-t border-border pt-4" role="group" aria-label="Confirm firewall change">
				<p class="text-sm">{confirmation === "configure" ? `Allow FiveM traffic on TCP ${tcpPort} and UDP ${udpPort}? Only missing app-managed rules will be added; the firewall stays enabled.` : `Remove this app's rules for TCP ${tcpPort} and UDP ${udpPort}? Players may lose access if no other rules allow these ports.`}</p>
				<p class="text-xs text-muted-foreground">Approve the operating system's administrator prompt to continue. Database and txAdmin ports are not opened.</p>
				<div class="flex gap-2"><Button size="sm" onclick={() => confirmation && run(confirmation)}>{confirmation === "configure" ? "Apply rules" : "Confirm removal"}</Button><Button size="sm" variant="ghost" onclick={() => confirmation = null}>Cancel</Button></div>
			</div>
		{/if}
	</Card.Content>
</Card.Root>
