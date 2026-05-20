<script lang="ts">
	import ArrowRightIcon from "@lucide/svelte/icons/arrow-right";
	import CheckCircle2Icon from "@lucide/svelte/icons/check-circle-2";
	import CircleIcon from "@lucide/svelte/icons/circle";
	import RocketIcon from "@lucide/svelte/icons/rocket";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import type { PageId } from "$lib/navigation";

	type Props = {
		onNavigate: (page: PageId) => void;
	};

	let { onNavigate }: Props = $props();
	const completionKey = "fxserver-installer.onboarding.completed";
	let completed = $state<string[]>([]);

	const steps: { id: string; title: string; description: string; page: PageId; action: string }[] = [
		{ id: "mariadb", title: "Prepare MariaDB", description: "Install MariaDB, start the service, and create or validate a database user.", page: "mariadb", action: "Open MariaDB" },
		{ id: "artifact", title: "Install FXServer artifact", description: "Download a recommended Windows artifact and choose the server folder.", page: "artifact-install", action: "Install Artifact" },
		{ id: "profile", title: "Choose txData profile", description: "Load your txAdmin profile and resolve the dataPath from config.json.", page: "server-configure", action: "Configure Server" },
		{ id: "database-string", title: "Set database string", description: "Validate credentials, choose a database, and write the connection string into cfg.", page: "server-configure", action: "Open Connection Tools" },
		{ id: "rcon", title: "Enable RCON", description: "Ensure rconlog and rcon_password are present before using console and resource tools.", page: "server-configure", action: "Configure RCON" },
		{ id: "start", title: "Start and verify server", description: "Start FXServer, watch performance, send RCON, and inspect logs.", page: "server-manage", action: "Manage Server" },
	];

	$effect(() => {
		try {
			completed = JSON.parse(localStorage.getItem(completionKey) || "[]");
		} catch {
			completed = [];
		}
	});

	function toggleStep(id: string) {
		const next = new Set(completed);
		if (next.has(id)) {
			next.delete(id);
		} else {
			next.add(id);
		}
		completed = [...next];
		localStorage.setItem(completionKey, JSON.stringify(completed));
	}
</script>

<section class="space-y-6">
	<div class="flex flex-col justify-between gap-4 lg:flex-row lg:items-end">
		<div>
			<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">Setup</p>
			<h1 class="mt-2 text-3xl font-semibold tracking-normal text-foreground">First Run Wizard</h1>
			<p class="mt-2 max-w-2xl text-sm leading-6 text-muted-foreground">A guided checklist for getting a fresh FXServer workspace into a usable state.</p>
		</div>
		<div class="rounded-sm border border-border bg-card px-3 py-2 text-xs font-semibold text-muted-foreground">{completed.length} / {steps.length} complete</div>
	</div>

	<Card.Root class="rounded-md border-border bg-card shadow-sm">
		<Card.Header class="border-b border-border pb-4">
			<div class="flex items-center gap-3">
				<div class="flex size-9 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
					<RocketIcon class="size-4" />
				</div>
				<div>
					<Card.Title>Setup Flow</Card.Title>
					<Card.Description>Mark steps complete as you go. Each action opens the matching workspace.</Card.Description>
				</div>
			</div>
		</Card.Header>
		<Card.Content class="grid gap-3">
			{#each steps as step, index}
				{@const done = completed.includes(step.id)}
				<div class="grid gap-3 rounded-sm border border-border bg-background/70 p-3 md:grid-cols-[auto_minmax(0,1fr)_auto] md:items-center">
					<button class="flex size-9 items-center justify-center rounded-sm border border-border bg-card text-muted-foreground" onclick={() => toggleStep(step.id)} title={done ? "Mark incomplete" : "Mark complete"}>
						{#if done}
							<CheckCircle2Icon class="size-5 text-emerald-400" />
						{:else}
							<CircleIcon class="size-5" />
						{/if}
					</button>
					<div class="min-w-0">
						<p class="text-sm font-semibold text-foreground">{index + 1}. {step.title}</p>
						<p class="mt-1 text-xs leading-5 text-muted-foreground">{step.description}</p>
					</div>
					<Button variant="outline" onclick={() => onNavigate(step.page)} title={step.action}>
						{step.action}
						<ArrowRightIcon class="size-4" />
					</Button>
				</div>
			{/each}
		</Card.Content>
	</Card.Root>
</section>
