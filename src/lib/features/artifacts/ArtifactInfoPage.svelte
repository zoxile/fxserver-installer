<script lang="ts">
	import AlertCircleIcon from "@lucide/svelte/icons/alert-circle";
	import ArchiveIcon from "@lucide/svelte/icons/archive";
	import CheckCircle2Icon from "@lucide/svelte/icons/check-circle-2";
	import ExternalLinkIcon from "@lucide/svelte/icons/external-link";
	import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
	import MonitorDownIcon from "@lucide/svelte/icons/monitor-down";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import ShieldCheckIcon from "@lucide/svelte/icons/shield-check";
	import { onMount } from "svelte";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { log } from "$lib/core/logger";
	import { openExternalUrl } from "$lib/core/openExternal";
	import { getInstallPath, loadInstallPath } from "$lib/core/paths.svelte";
	import {
		artifactIsFlagged,
		fetchArtifactMetadata,
		getArtifactHealthStatus,
		getInstalledWindowsArtifactInfo,
		type ArtifactHealthStatus,
		type ArtifactMetadata,
		type InstalledArtifactInfo,
	} from "$lib/modules/artifact";
	import { artifactUrgencyClass, artifactUrgencyTextClass } from "./artifactUi";
	import ArtifactIssueList from "./ArtifactIssueList.svelte";
	import ArtifactStatCard from "./ArtifactStatCard.svelte";

	let metadata = $state<ArtifactMetadata | null>(null);
	let installed = $state<InstalledArtifactInfo | null>(null);
	let busy = $state(false);
	let error = $state("");
	let selectedInstallPath = $state("");

	const recommendedIsFlagged = $derived(metadata ? artifactIsFlagged(metadata.recommendedArtifact, metadata.brokenArtifacts) : false);
	const health = $derived<ArtifactHealthStatus>(getArtifactHealthStatus(metadata, installed));
	const installFolderLabel = $derived(installed?.destination ?? (selectedInstallPath || "No folder selected"));

	onMount(() => {
		loadInstallPath();
		selectedInstallPath = getInstallPath();
		void refresh();
	});

	async function refresh() {
		busy = true;
		error = "";

		try {
			const path = selectedInstallPath || getInstallPath();
			selectedInstallPath = path;
			const [nextMetadata, nextInstalled] = await Promise.all([
				fetchArtifactMetadata(),
				path ? getInstalledWindowsArtifactInfo(path) : Promise.resolve(null),
			]);
			metadata = nextMetadata;
			installed = nextInstalled;
		} catch (caught) {
			error = caught instanceof Error ? caught.message : String(caught);
			log("Artifact information page failed to refresh.", { level: "error", scope: "artifacts.info", detail: error });
		} finally {
			busy = false;
		}
	}

	function formatFetchedAt(value: string) {
		return new Date(value).toLocaleString(undefined, {
			month: "short",
			day: "2-digit",
			hour: "2-digit",
			minute: "2-digit",
		});
	}
</script>

<section class="space-y-6">
	<div class="flex flex-col justify-between gap-4 lg:flex-row lg:items-end">
		<div>
			<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">Artifacts</p>
			<h1 class="mt-2 text-3xl font-semibold tracking-normal text-foreground">Artifact Information</h1>
			<p class="mt-2 max-w-2xl text-sm leading-6 text-muted-foreground">Inspect the Windows FXServer artifact JG Scripts currently considers healthy, plus known versions and ranges with reported issues.</p>
		</div>
		<div class="flex flex-wrap gap-2">
			<Button variant="outline" onclick={refresh} disabled={busy} title="Refresh artifact metadata from JG Scripts">
				<RefreshCwIcon class={busy ? "animate-spin" : undefined} />
				Refresh
			</Button>
			<Button variant="outline" onclick={() => openExternalUrl("https://artifacts.jgscripts.com/")} title="Open the JG Scripts artifacts database">
				<ExternalLinkIcon />
				JG Artifacts DB
			</Button>
		</div>
	</div>

	{#if error}
		<div class="rounded-sm border border-red-400/30 bg-red-400/10 px-4 py-3 text-sm text-red-100">
			<div class="flex items-start gap-2">
				<AlertCircleIcon class="mt-0.5 size-4 shrink-0" />
				<p>{error}</p>
			</div>
		</div>
	{/if}

	{#if metadata}
		<div class="grid gap-4 md:grid-cols-2 xl:grid-cols-4">
			<ArtifactStatCard label="Installed Build" value={health.currentVersion ?? (installed?.installed ? "Unknown" : "None")} description={health.description} icon={ArchiveIcon} tone={health.urgency === "needed" ? "error" : health.urgency === "recommended" ? "warn" : health.urgency === "none" ? "success" : "info"} />
			<ArtifactStatCard label="Healthy Windows Build" value={metadata.recommendedArtifact} description={recommendedIsFlagged ? "This build appears in the reported issue list." : "Latest artifact with no reported issues according to JG Scripts."} icon={ShieldCheckIcon} tone={recommendedIsFlagged ? "warn" : "success"} />
			<ArtifactStatCard label="Platform" value="Windows" description="Linux artifacts are intentionally hidden until the app supports Linux installs." icon={MonitorDownIcon} tone="info" />
			<ArtifactStatCard label="Reported Issues" value={String(metadata.brokenArtifacts.length)} description="Known broken builds and ranges loaded from the JG Scripts database." icon={ArchiveIcon} tone={metadata.brokenArtifacts.length ? "warn" : "success"} />
		</div>

		<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-transform duration-300 hover:-translate-y-0.5">
			<div class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"></div>
			<Card.Header class="border-b border-border pb-4">
				<Card.Title>Installed Artifact Status</Card.Title>
				<Card.Description>Compared against the current healthy Windows recommendation from JG Scripts.</Card.Description>
			</Card.Header>
			<Card.Content class="grid gap-3 md:grid-cols-3">
				<div class={`rounded-sm border px-3 py-2 ${artifactUrgencyClass(health.urgency)}`}>
					<p class="text-xs opacity-80">Status</p>
					<p class="mt-1 text-lg font-semibold">{health.label}</p>
				</div>
				<div class="rounded-sm border border-border bg-background/70 px-3 py-2">
					<p class="text-xs text-muted-foreground">Version Source</p>
					<p class="mt-1 truncate text-sm font-medium text-foreground">{installed?.detectionSource ?? "none"}</p>
				</div>
				<div class="rounded-sm border border-border bg-background/70 px-3 py-2">
					<p class="text-xs text-muted-foreground">Product Version</p>
					<p class="mt-1 truncate font-mono text-sm font-medium text-foreground">{installed?.productVersion ?? installed?.fileVersion ?? "Unknown"}</p>
				</div>
				<div class="rounded-sm border border-border bg-background/70 px-3 py-2">
					<p class="text-xs text-muted-foreground">Install Folder</p>
					<p class="mt-1 truncate font-mono text-xs text-foreground">{installFolderLabel}</p>
				</div>
				<div class="rounded-sm border border-border bg-background/70 px-3 py-2">
					<p class="text-xs text-muted-foreground">Version File</p>
					<p class="mt-1 truncate font-mono text-xs text-foreground">{installed?.citizenServerImplPath ?? "Not found"}</p>
				</div>
				<div class="rounded-sm border border-border bg-background/70 px-3 py-2">
					<p class="text-xs text-muted-foreground">Metadata Fetched</p>
					<p class="mt-1 text-sm font-medium text-foreground">{formatFetchedAt(metadata.fetchedAt)}</p>
				</div>
				<p class={`text-sm leading-5 md:col-span-3 ${artifactUrgencyTextClass(health.urgency)}`}>{health.description}</p>
			</Card.Content>
		</Card.Root>

		<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-transform duration-300 hover:-translate-y-0.5">
			<div class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"></div>
			<Card.Header class="border-b border-border pb-4">
				<Card.Title>Windows Download</Card.Title>
				<Card.Description>The install page uses this Windows artifact link. Linux output from the API is ignored for now.</Card.Description>
			</Card.Header>
			<Card.Content class="space-y-3">
				<div class="rounded-sm border border-border bg-background/70 p-3 font-mono text-xs text-muted-foreground break-all">
					{metadata.windowsDownloadLink}
				</div>
				<div class="flex items-center gap-2 text-xs text-muted-foreground">
					<CheckCircle2Icon class={["size-3.5", recommendedIsFlagged ? "text-amber-400" : "text-emerald-400"]} />
					{recommendedIsFlagged ? "This recommended build is also reported as problematic. Review before installing." : "This recommended build is not present in the reported issue list."}
				</div>
			</Card.Content>
		</Card.Root>

		<ArtifactIssueList issues={metadata.brokenArtifacts} />
	{:else if busy}
		<Card.Root class="rounded-sm border-border bg-card shadow-sm">
			<Card.Content class="flex min-h-72 items-center justify-center gap-2 text-sm text-muted-foreground">
				<LoaderCircleIcon class="size-4 animate-spin" />
				Loading JG Scripts artifact metadata...
			</Card.Content>
		</Card.Root>
	{/if}
</section>
