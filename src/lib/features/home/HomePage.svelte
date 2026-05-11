<script lang="ts">
	import ArchiveIcon from "@lucide/svelte/icons/archive";
	import CheckCircle2Icon from "@lucide/svelte/icons/check-circle-2";
	import DatabaseIcon from "@lucide/svelte/icons/database";
	import ExternalLinkIcon from "@lucide/svelte/icons/external-link";
	import FileJsonIcon from "@lucide/svelte/icons/file-json";
	import GaugeIcon from "@lucide/svelte/icons/gauge";
	import GitBranchIcon from "@lucide/svelte/icons/git-branch";
	import PackageIcon from "@lucide/svelte/icons/package";
	import ServerCogIcon from "@lucide/svelte/icons/server-cog";
	import WrenchIcon from "@lucide/svelte/icons/wrench";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { openExternalUrl } from "$lib/core/openExternal";
	import HomeArtifactStatusCard from "./HomeArtifactStatusCard.svelte";
	import HomeBentoCard from "./HomeBentoCard.svelte";
	import type { PageId } from "$lib/navigation";

	type Props = {
		onNavigate: (page: PageId) => void;
	};

	let { onNavigate }: Props = $props();
	const currentVersion = "0.1.0";
	const latestVersion = "0.1.0";
	const isUpToDate = currentVersion === latestVersion;

	const projectDetails = [
		{ label: "App", value: "FXServer Installer", icon: PackageIcon },
		{ label: "Version", value: currentVersion, icon: GaugeIcon },
		{ label: "Git", value: "zoxile/fxserver-installer", icon: GitBranchIcon },
	];
</script>

<section class="relative space-y-6 overflow-hidden">
	<div class="grid gap-4 xl:grid-cols-[1.15fr_0.85fr_0.85fr]">
		<div>
			<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">Home</p>
			<h1 class="mt-2 max-w-3xl text-3xl font-semibold tracking-normal text-foreground">FXServer setup, arranged into one quiet workspace.</h1>
			<p class="mt-3 max-w-2xl text-sm leading-6 text-muted-foreground">Move through database setup, artifact preparation, server configuration, and utility tools without losing the thread.</p>
		</div>

		<Card.Root class="rounded-md border-border bg-card shadow-sm">
			<Card.Header class="border-b border-border pb-4">
				<Card.Title>Project</Card.Title>
				<Card.Description>Workspace details and repository context.</Card.Description>
			</Card.Header>
			<Card.Content class="space-y-3">
				<div class="grid gap-2">
					{#each projectDetails as detail}
						{@const Icon = detail.icon}
						<div class="flex items-center justify-between gap-3 rounded-sm border border-border bg-background/70 px-3 py-2">
							<div class="flex items-center gap-2 text-xs text-muted-foreground">
								<Icon class="size-3.5" />
								{detail.label}
							</div>
							<p class="truncate text-xs font-medium text-foreground">{detail.value}</p>
						</div>
					{/each}
				</div>
				<div class="flex items-center justify-between gap-3 rounded-sm border border-border bg-background/70 px-3 py-2">
					<div class="flex items-center gap-2 text-xs text-muted-foreground">
						<CheckCircle2Icon class={["size-3.5", isUpToDate ? "text-emerald-400" : "text-amber-400"]} />
						Update status
					</div>
					<p class={["truncate text-xs font-medium", isUpToDate ? "text-emerald-400" : "text-amber-400"]}>
						{isUpToDate ? "Up to date" : `Update ${latestVersion} available`}
					</p>
				</div>
				<Button
					variant="outline"
					size="sm"
					class="w-fit rounded-sm"
					onclick={() => openExternalUrl("https://github.com/zoxile/fxserver-installer")}
					title="Open GitHub repository"
				>
					GitHub
					<ExternalLinkIcon class="size-3.5" />
				</Button>
			</Card.Content>
		</Card.Root>

		<HomeArtifactStatusCard {onNavigate} />
	</div>

	<div class="bento-grid grid gap-4">
		<HomeBentoCard
			title="MariaDB"
			description="Install MariaDB, detect the local service, control status, manage users, assign grants, and run SQL from one place."
			icon={DatabaseIcon}
			size="hero"
			className="md:[grid-area:db]"
			kicker="Database layer"
			highlights={["Installation config", "Service controls", "User grants", "Query console", "Connection profile", "Version detection"]}
			actionLabel="Open MariaDB"
			onclick={() => onNavigate("mariadb")}
		/>
		<HomeBentoCard
			title="Artifacts"
			description="Download recommended FXServer artifacts and inspect build metadata."
			icon={ArchiveIcon}
			size="feature"
			className="md:[grid-area:artifacts]"
			kicker="Runtime builds"
			highlights={["Installer", "Version metadata"]}
			actionLabel="Install Artifact"
			onclick={() => onNavigate("artifact-install")}
		/>
		<HomeBentoCard
			title="FXServer"
			description="Start FXServer with TXHOST variables and watch its process usage."
			icon={ServerCogIcon}
			size="wide"
			className="md:[grid-area:server]"
			kicker="Server core"
			actionLabel="Open Server"
			onclick={() => onNavigate("server-manage")}
		/>
		<HomeBentoCard
			title="Configurator"
			description="Build and tune server configuration files."
			icon={WrenchIcon}
			size="compact"
			className="md:[grid-area:config]"
			kicker="Configuration"
			actionLabel="Configure"
			onclick={() => onNavigate("configurator")}
		/>
		<HomeBentoCard
			title="Profiler"
			description="Review profiler captures and inspect runtime health."
			icon={GaugeIcon}
			size="compact"
			className="md:[grid-area:profiler]"
			kicker="Runtime"
			actionLabel="View Profiler"
			onclick={() => onNavigate("profiler")}
		/>
		<HomeBentoCard
			title="JSON Formatter"
			description="Format and validate resource data."
			icon={FileJsonIcon}
			size="compact"
			className="md:[grid-area:json]"
			kicker="Utility"
			actionLabel="Open Formatter"
			onclick={() => onNavigate("json-formatter")}
		/>
	</div>
</section>

<style>
	@media (min-width: 768px) {
		.bento-grid {
			grid-template-columns: repeat(6, minmax(0, 1fr));
			grid-template-rows: repeat(4, minmax(118px, auto));
			grid-template-areas:
				"db db db artifacts artifacts artifacts"
				"db db db artifacts artifacts artifacts"
				"db db db server server server"
				"profiler profiler config config json json";
		}
	}
</style>
