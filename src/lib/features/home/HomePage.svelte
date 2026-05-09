<script lang="ts">
	import ArchiveIcon from "@lucide/svelte/icons/archive";
	import DatabaseIcon from "@lucide/svelte/icons/database";
	import FileJsonIcon from "@lucide/svelte/icons/file-json";
	import GaugeIcon from "@lucide/svelte/icons/gauge";
	import ServerCogIcon from "@lucide/svelte/icons/server-cog";
	import WrenchIcon from "@lucide/svelte/icons/wrench";
	import * as Card from "$lib/components/ui/card/index.js";
	import HomeBentoCard from "./HomeBentoCard.svelte";
	import type { PageId } from "$lib/navigation";

	type Props = {
		onNavigate: (page: PageId) => void;
	};

	let { onNavigate }: Props = $props();
</script>

<section class="space-y-6">
	<div class="grid gap-4 lg:grid-cols-[1.2fr_0.8fr]">
		<div>
			<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">Home</p>
			<h1 class="mt-2 max-w-3xl text-3xl font-semibold tracking-normal text-foreground">FXServer setup, arranged into one quiet workspace.</h1>
			<p class="mt-3 max-w-2xl text-sm leading-6 text-muted-foreground">
				Move through database setup, artifact preparation, server configuration, and utility tools without losing the thread.
			</p>
		</div>

		<Card.Root class="rounded-md border-border bg-card shadow-sm">
			<Card.Header class="border-b border-border pb-4">
				<Card.Title>Setup Flow</Card.Title>
				<Card.Description>Start with dependencies, then prepare the runtime and config.</Card.Description>
			</Card.Header>
			<Card.Content>
				<div class="grid grid-cols-3 gap-2 text-center text-xs text-muted-foreground">
					<div class="rounded-sm border border-border bg-background p-3">
						<p class="text-sm font-semibold text-foreground">1</p>
						<p class="mt-1">Database</p>
					</div>
					<div class="rounded-sm border border-border bg-background p-3">
						<p class="text-sm font-semibold text-foreground">2</p>
						<p class="mt-1">Artifact</p>
					</div>
					<div class="rounded-sm border border-border bg-background p-3">
						<p class="text-sm font-semibold text-foreground">3</p>
						<p class="mt-1">Server</p>
					</div>
				</div>
			</Card.Content>
		</Card.Root>
	</div>

	<div class="grid auto-rows-[minmax(176px,auto)] gap-4 md:grid-cols-2 xl:grid-cols-4">
		<HomeBentoCard
			title="MariaDB"
			description="Install, detect, control services, manage users, and run SQL."
			icon={DatabaseIcon}
			size="feature"
			actionLabel="Open MariaDB"
			onclick={() => onNavigate("mariadb")}
		/>
		<HomeBentoCard
			title="Artifacts"
			description="Download recommended FXServer artifacts and inspect build metadata."
			icon={ArchiveIcon}
			size="feature"
			actionLabel="Install Artifact"
			onclick={() => onNavigate("artifact-install")}
		/>
		<HomeBentoCard
			title="FXServer"
			description="Prepare server setup and launch workflows."
			icon={ServerCogIcon}
			actionLabel="Open Server"
			onclick={() => onNavigate("server")}
		/>
		<HomeBentoCard
			title="Configurator"
			description="Build and tune server configuration files."
			icon={WrenchIcon}
			actionLabel="Configure"
			onclick={() => onNavigate("configurator")}
		/>
		<HomeBentoCard
			title="Profiler"
			description="Review profiler captures and inspect runtime health."
			icon={GaugeIcon}
			actionLabel="View Profiler"
			onclick={() => onNavigate("profiler")}
		/>
		<HomeBentoCard
			title="JSON Formatter"
			description="Format and validate resource data."
			icon={FileJsonIcon}
			actionLabel="Open Formatter"
			onclick={() => onNavigate("json-formatter")}
		/>
	</div>
</section>
