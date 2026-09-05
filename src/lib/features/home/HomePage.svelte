<script lang="ts">
	import ArchiveIcon from "@lucide/svelte/icons/archive";
	import CheckCircle2Icon from "@lucide/svelte/icons/check-circle-2";
	import DatabaseIcon from "@lucide/svelte/icons/database";
	import DownloadIcon from "@lucide/svelte/icons/download";
	import ExternalLinkIcon from "@lucide/svelte/icons/external-link";
	import GaugeIcon from "@lucide/svelte/icons/gauge";
	import GitBranchIcon from "@lucide/svelte/icons/git-branch";
	import LogsIcon from "@lucide/svelte/icons/logs";
	import PackageIcon from "@lucide/svelte/icons/package";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import RocketIcon from "@lucide/svelte/icons/rocket";
	import ServerCogIcon from "@lucide/svelte/icons/server-cog";
	import TerminalIcon from "@lucide/svelte/icons/terminal";
	import WrenchIcon from "@lucide/svelte/icons/wrench";
	import { onMount } from "svelte";
	import * as Card from "$lib/components/ui/card/index.js";
	import * as Separator from "$lib/components/ui/separator/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import releasePolicy from "../../../../release-policy.json";
	import appPackage from "../../../../package.json";
	import { openExternalUrl } from "$lib/core/openExternal";
	import { compareVersions, fetchLatestAppRelease, getCurrentAppVersion, type AppReleaseInfo } from "$lib/modules/appRelease";
	import HomeArtifactStatusCard from "./HomeArtifactStatusCard.svelte";
	import HomeBentoCard from "./HomeBentoCard.svelte";
	import { workspaceSession } from "$lib/core/workspaces.svelte";
	import type { PageId } from "$lib/navigation";

	type Props = {
		onNavigate: (page: PageId) => void;
	};

	let { onNavigate }: Props = $props();
	let currentVersion = $state("Checking...");
	let latestRelease = $state<AppReleaseInfo | null>(null);
	let releaseError = $state("");
	let checkingRelease = $state(false);
	const betaBuild = releasePolicy.betaVersions.includes(appPackage.version);

	const latestVersion = $derived(latestRelease?.version ?? "");
	const updateAvailable = $derived(Boolean(latestRelease && compareVersions(latestRelease.version, currentVersion) > 0));
	const projectStatus = $derived(
		releaseError ? "Check failed" : checkingRelease ? "Checking..." : !latestRelease ? "Unknown" : updateAvailable ? `Update ${latestVersion} available` : "Up to date",
	);
	const projectStatusTone = $derived(releaseError ? "text-red-300" : checkingRelease ? "text-muted-foreground" : updateAvailable ? "text-amber-400" : "text-emerald-400");
	const projectDetails = $derived([
		{ label: "App", value: "FXServer Installer", icon: PackageIcon },
		{ label: "Version", value: currentVersion, icon: GaugeIcon },
		{ label: "Git", value: "zoxile/fxserver-installer", icon: GitBranchIcon },
	]);

	onMount(() => {
		void refreshProjectVersion();
	});

	async function refreshProjectVersion(force = false) {
		checkingRelease = true;
		releaseError = "";

		try {
			const [version, release] = await Promise.all([getCurrentAppVersion(), fetchLatestAppRelease(force)]);
			currentVersion = version;
			latestRelease = release;
		} catch (error) {
			currentVersion = await getCurrentAppVersion().catch(() => "Unknown");
			releaseError = error instanceof Error ? error.message : String(error);
		} finally {
			checkingRelease = false;
		}
	}

	function openLatestInstaller() {
		const url = latestRelease?.installerUrl || latestRelease?.htmlUrl || "https://github.com/zoxile/fxserver-installer/releases/latest";
		void openExternalUrl(url);
	}
</script>

<section class="relative space-y-6 overflow-hidden">
	{#if betaBuild}
		<Notice tone="warn" title={`${appPackage.version} Beta`} message="This version is not fully tested on live servers or databases. Issues and breaking changes may occur. Keep independent backups and test changes on a disposable server first." />
	{/if}
	<div class="grid gap-4 xl:grid-cols-[1.15fr_0.85fr_0.85fr]">
		<div>
			<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">Home</p>
			<h1 class="mt-2 max-w-3xl text-3xl font-semibold tracking-normal text-foreground">FXServer setup, arranged into one quiet workspace.</h1>
			<p class="mt-3 max-w-2xl text-sm leading-6 text-muted-foreground">Move through database setup, artifact preparation, server configuration, and utility tools without losing the thread.</p>
		</div>

		<Card.Root class="flex flex-col rounded-md border-border bg-card shadow-sm">
			<Card.Header class="border-b border-border pb-4">
				<div class="flex items-center justify-between gap-2"><Card.Title>Project</Card.Title>{#if betaBuild}<span class="rounded-sm border border-amber-400/30 bg-amber-400/10 px-2 py-0.5 text-xs font-medium text-amber-300">Beta</span>{/if}</div>
				<Card.Description>Workspace details and repository context.</Card.Description>
			</Card.Header>
			<Card.Content class="flex flex-1 flex-col gap-3">
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
						<CheckCircle2Icon class={["size-3.5", projectStatusTone]} />
						Update status
					</div>
					<div class="flex min-w-0 items-center gap-2">
						<p class={["truncate text-xs font-medium", projectStatusTone]}>{projectStatus}</p>
						<Button variant="ghost" size="icon" class="size-7 rounded-sm" onclick={() => refreshProjectVersion(true)} disabled={checkingRelease} title="Refresh latest version status">
							<RefreshCwIcon class={["size-3.5", checkingRelease && "animate-spin"]} />
						</Button>
					</div>
				</div>
				{#if releaseError}
					<p class="rounded-sm border border-red-400/20 bg-red-400/10 px-3 py-2 text-xs text-red-100">{releaseError}</p>
				{/if}
				<div class="mt-auto flex flex-wrap gap-2">
					<Button variant={updateAvailable ? "default" : "outline"} size="sm" class="rounded-sm" onclick={openLatestInstaller} title="Open the latest installer download">
						<DownloadIcon class="size-3.5" />
						{updateAvailable ? "Update" : "Installer"}
					</Button>
					<Button
						variant="outline"
						size="sm"
						class="rounded-sm"
						onclick={() => openExternalUrl("https://github.com/zoxile/fxserver-installer")}
						title="Open GitHub repository"
					>
						GitHub
						<ExternalLinkIcon class="size-3.5" />
					</Button>
				</div>
			</Card.Content>
		</Card.Root>

		<HomeArtifactStatusCard {onNavigate} />
	</div>

	<Separator.Root class="bg-border/80" />
	<div class="flex flex-wrap items-center justify-between gap-3 border-b border-border pb-5">
		<h2 class="text-sm font-semibold">{workspaceSession.items.find((item) => item.id === workspaceSession.activeId)?.name ?? "Default"}</h2>
		<div class="flex flex-wrap gap-2">
			<Button variant="outline" size="sm" onclick={() => onNavigate("workspaces")}><ServerCogIcon class="size-4" />Workspaces</Button>
			<Button variant="outline" size="sm" onclick={() => onNavigate("diagnostics")}><CheckCircle2Icon class="size-4" />Diagnostics</Button>
			<Button variant="outline" size="sm" onclick={() => onNavigate("health")}><GaugeIcon class="size-4" />Health</Button>
			<Button variant="outline" size="sm" onclick={() => onNavigate("tasks")}><TerminalIcon class="size-4" />Tasks</Button>
		</div>
	</div>

	<div class="bento-grid grid gap-4">
		<HomeBentoCard
			title="MariaDB"
			description="Install, update, or uninstall MariaDB while preserving data, with visible installer stages, service controls, user grants, SQL tools, and database backups."
			icon={DatabaseIcon}
			size="hero"
			className="md:[grid-area:db]"
			kicker="Database layer"
			highlights={[
				"Install and update details",
				"Preserve databases",
				"Backup warning",
				"Service controls",
				"User grants",
				"Query helpers",
				"SQL console",
				"Scheduled backups and restore tests",
				"Table browser and CSV export",
				"Version detection",
			]}
			actions={[
				{ label: "Manage", onclick: () => onNavigate("mariadb") },
				{ label: "Queries & Files", onclick: () => onNavigate("sql-runner") },
				{ label: "Backups & Restore", onclick: () => onNavigate("backup-manager") },
				{ label: "Browse Data", onclick: () => onNavigate("database-browser") },
			]}
			onclick={() => onNavigate("mariadb")}
		/>
		<HomeBentoCard
			title="First Run"
			description="Walk through the setup flow from MariaDB to artifact install, profile selection, database string, RCON, and first server start."
			icon={RocketIcon}
			size="compact"
			className="md:[grid-area:onboarding]"
			kicker="Setup"
			highlights={["Guided checklist", "Direct actions"]}
			actionLabel="Open Wizard"
			onclick={() => onNavigate("onboarding")}
		/>
		<HomeBentoCard
			title="Artifacts"
			description="Browse official Windows builds with JG Scripts recommendations and reported issues."
			icon={ArchiveIcon}
			size="feature"
			className="md:[grid-area:artifacts]"
			kicker="Runtime builds"
			highlights={["Official build catalog", "Recommended builds", "Reported issue warnings"]}
			actionLabel="Install Artifact"
			onclick={() => onNavigate("artifact-install")}
		/>
		<HomeBentoCard
			title="FXServer"
			description="Start FXServer with TXHOST variables, send RCON commands, save the RCON password securely, and track CPU, RAM, uptime, threads, and handles."
			icon={ServerCogIcon}
			size="wide"
			className="md:[grid-area:server]"
			kicker="Server core"
			highlights={["Live console", "Resource controls", "Secure RCON password", "Performance charts"]}
			actions={[
				{ label: "Manage", onclick: () => onNavigate("server-manage") },
				{ label: "Resources", onclick: () => onNavigate("resource-manager") },
				{ label: "Live Bridge", onclick: () => onNavigate("live-bridge") },
			]}
			onclick={() => onNavigate("server-manage")}
		/>
		<HomeBentoCard
			title="Configure Server"
			description="Choose a txAdmin profile, edit colored .cfg files, validate database connection strings, and use focused helpers for RCON and permissions."
			icon={WrenchIcon}
			size="compact"
			className="md:[grid-area:config]"
			kicker="Configuration"
			highlights={["Colored cfg editor", "Encrypted version history", "Diff and restore", "Permission helpers"]}
			actionLabel="Configure"
			onclick={() => onNavigate("server-configure")}
		/>
		<HomeBentoCard
			title="Logs"
			description="Use the dedicated Logs section to inspect application logs, txData server logs, and FiveM client logs with filters, level colors, and live refresh."
			icon={LogsIcon}
			size="wide"
			className="md:[grid-area:logs]"
			kicker="Diagnostics"
			highlights={["Dedicated nav section", "Application logs", "Server logs", "Client logs"]}
			actions={[
				{ label: "App Logs", onclick: () => onNavigate("logs") },
				{ label: "Server Logs", onclick: () => onNavigate("server-logs") },
				{ label: "Client Logs", onclick: () => onNavigate("client-logs") },
				{ label: "Timeline", onclick: () => onNavigate("incidents") },
			]}
			onclick={() => onNavigate("client-logs")}
		/>
		<HomeBentoCard
			title="Tools"
			description="Format JSON, resolve JOOAT hashes, review profiler captures, and keep resource data tidy without leaving the workspace."
			icon={TerminalIcon}
			size="feature"
			className="md:[grid-area:tools]"
			kicker="Utility"
			highlights={["Command palette", "JSON formatter", "JOOAT resolver", "Profiler", "Lua configurator"]}
			actions={[
				{ label: "Palette", onclick: () => onNavigate("command-palette") },
				{ label: "JSON", onclick: () => onNavigate("json-formatter") },
				{ label: "JOOAT", onclick: () => onNavigate("jooat") },
				{ label: "Profiler", onclick: () => onNavigate("profiler") },
				{ label: "Configurator", onclick: () => onNavigate("configurator") },
			]}
			onclick={() => onNavigate("json-formatter")}
		/>
	</div>
</section>

<style>
	@media (min-width: 768px) {
		.bento-grid {
			grid-template-columns: repeat(6, minmax(0, 1fr));
			grid-template-rows: repeat(5, minmax(118px, auto));
			grid-template-areas:
				"db db db artifacts artifacts artifacts"
				"db db db artifacts artifacts artifacts"
				"db db db server server server"
				"onboarding onboarding config config tools tools"
				"logs logs logs logs tools tools";
		}
	}
</style>
