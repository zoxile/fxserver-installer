<script lang="ts">
	import { platform, platformLabel } from "$lib/core/platform.svelte";
	import ArchiveIcon from "@lucide/svelte/icons/archive";
	import CheckCircle2Icon from "@lucide/svelte/icons/check-circle-2";
	import GaugeIcon from "@lucide/svelte/icons/gauge";
	import DatabaseIcon from "@lucide/svelte/icons/database";
	import LogsIcon from "@lucide/svelte/icons/logs";
	import RocketIcon from "@lucide/svelte/icons/rocket";
	import ServerCogIcon from "@lucide/svelte/icons/server-cog";
	import TerminalIcon from "@lucide/svelte/icons/terminal";
	import WrenchIcon from "@lucide/svelte/icons/wrench";
	import * as Separator from "$lib/components/ui/separator/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import releasePolicy from "../../../../release-policy.json";
	import appPackage from "../../../../package.json";
	import ProjectCard from "./ProjectCard.svelte";
	import HomeArtifactStatusCard from "./HomeArtifactStatusCard.svelte";
	import HomeBentoCard from "./HomeBentoCard.svelte";
	import { workspaceSession } from "$lib/core/workspaces.svelte";
	import type { PageId } from "$lib/navigation";

	let { onNavigate }: { onNavigate: (page: PageId) => void } = $props();
	const betaBuild = releasePolicy.betaVersions.includes(appPackage.version);
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

		<ProjectCard {betaBuild} />

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
			description={platform.os === "linux" ? "Manage MariaDB distribution packages, inspect the service, manage database users, run SQL, and back up databases." : "Install, update, or uninstall MariaDB while preserving data, with visible installer stages, service controls, user grants, SQL tools, and database backups."}
			icon={DatabaseIcon}
			size="hero"
			className="md:[grid-area:db]"
			kicker="Database layer"
			highlights={[
				platform.os === "linux" ? "System package management" : "Install and update details",
				"Preserve databases",
				"Backup warning",
				"Service controls",
				"User grants",
				"Query helpers",
				"Saved queries and table bookmarks",
				"Slow query analysis",
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
			description={`Browse official ${platformLabel()} builds with JG Scripts recommendations and reported issues.`}
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
			description={platform.secureStorageSupported ? "Start FXServer with TXHOST variables, send RCON commands, save the RCON password securely, and track CPU, RAM, uptime, threads, and handles." : "Start FXServer with TXHOST variables, send RCON commands with session credentials, and track server performance."}
			icon={ServerCogIcon}
			size="wide"
			className="md:[grid-area:server]"
			kicker="Server core"
			highlights={["Live console", "Resource dependencies", platform.secureStorageSupported ? "Secure RCON password" : "Session RCON password", "Reviewed maintenance"]}
			actions={[
				{ label: "Manage", onclick: () => onNavigate("server-manage") },
				{ label: "Resources", onclick: () => onNavigate("resource-manager") },
				{ label: "Live Bridge", onclick: () => onNavigate("live-bridge") },
				{ label: "Maintenance", onclick: () => onNavigate("maintenance") },
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
			highlights={["Colored cfg editor", "Change validation", platform.secureStorageSupported ? "Encrypted version history" : "Configuration review", "Permission helpers"]}
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
			highlights={["Grouped console issues", "Application logs", "Server logs", "Client logs"]}
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
			highlights={["Command palette", "JSON formatter", "JOOAT resolver", "Guided profiler and comparison", "Lua configurator"]}
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
