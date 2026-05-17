<script lang="ts">
	import AlertCircleIcon from "@lucide/svelte/icons/alert-circle";
	import CheckCircle2Icon from "@lucide/svelte/icons/check-circle-2";
	import DownloadIcon from "@lucide/svelte/icons/download";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import HardDriveDownloadIcon from "@lucide/svelte/icons/hard-drive-download";
	import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import ShieldCheckIcon from "@lucide/svelte/icons/shield-check";
	import { onMount } from "svelte";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { Notice } from "$lib/components/ui/notice/index.js";
	import { chooseInstallFolder } from "$lib/core/selectFolder";
	import { getInstallPath, loadInstallPath, setInstallPath } from "$lib/core/paths.svelte";
	import { log } from "$lib/core/logger.svelte";
	import {
		artifactIsFlagged,
		fetchArtifactMetadata,
		getArtifactHealthStatus,
		getInstalledWindowsArtifactInfo,
		installWindowsArtifact,
		type ArtifactHealthStatus,
		type ArtifactInstallResult,
		type ArtifactMetadata,
		type InstalledArtifactInfo,
	} from "$lib/modules/artifact";
	import { artifactUrgencyClass, artifactUrgencyTextClass } from "./artifactUi";

	let metadata = $state<ArtifactMetadata | null>(null);
	let installPath = $state("");
	let busy = $state(false);
	let installing = $state(false);
	let error = $state("");
	let message = $state("");
	let result = $state<ArtifactInstallResult | null>(null);
	let installed = $state<InstalledArtifactInfo | null>(null);

	const recommendedIsFlagged = $derived(metadata ? artifactIsFlagged(metadata.recommendedArtifact, metadata.brokenArtifacts) : false);
	const health = $derived<ArtifactHealthStatus>(getArtifactHealthStatus(metadata, installed));

	onMount(() => {
		loadInstallPath();
		installPath = getInstallPath();
		void refreshMetadata();
	});

	async function refreshMetadata() {
		busy = true;
		error = "";
		message = "";

		try {
			const [nextMetadata, nextInstalled] = await Promise.all([fetchArtifactMetadata(), installPath ? getInstalledWindowsArtifactInfo(installPath) : Promise.resolve(null)]);
			metadata = nextMetadata;
			installed = nextInstalled;
			message = `Artifact ${metadata.recommendedArtifact} is ready to install for Windows.`;
		} catch (caught) {
			error = caught instanceof Error ? caught.message : String(caught);
			log("Artifact install page could not refresh metadata.", { level: "error", scope: "artifacts.install-page", detail: error });
		} finally {
			busy = false;
		}
	}

	async function chooseFolder() {
		error = "";
		message = "";

		try {
			const selectedPath = await chooseInstallFolder();
			installPath = selectedPath ?? getInstallPath();
			if (installPath) {
				message = "Install folder selected.";
				log("Artifact install folder selected.", { scope: "artifacts.install-page", detail: installPath });
				installed = await getInstalledWindowsArtifactInfo(installPath);
			}
		} catch (caught) {
			error = caught instanceof Error ? caught.message : String(caught);
			log("Artifact install folder selection failed.", { level: "error", scope: "artifacts.install-page", detail: error });
		}
	}

	function updateInstallPath(event: Event) {
		installPath = (event.currentTarget as HTMLInputElement).value;
		setInstallPath(installPath);
	}

	async function install() {
		if (!metadata) {
			error = "Refresh artifact metadata before installing.";
			return;
		}

		if (!installPath.trim()) {
			error = "Choose the FXServer folder where the Windows artifact should be extracted.";
			return;
		}

		installing = true;
		error = "";
		message = "Downloading and extracting the Windows artifact. This can take a moment.";
		result = null;

		try {
			result = await installWindowsArtifact(metadata, installPath.trim());
			installed = await getInstalledWindowsArtifactInfo(installPath.trim());
			message = `Installed artifact ${result.version}.`;
		} catch (caught) {
			error = caught instanceof Error ? caught.message : String(caught);
			message = "";
		} finally {
			installing = false;
		}
	}

	const checklist = [
		"Stop FXServer or txAdmin before replacing artifact files.",
		"Keep a backup of your current server folder before extracting.",
		"Do not close the application while it's installing.",
		"Start the server after install and check console output for resource errors.",
	];
</script>

<section class="space-y-6">
	<div class="flex flex-col justify-between gap-4 lg:flex-row lg:items-end">
		<div>
			<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">Artifacts</p>
			<h1 class="mt-2 text-3xl font-semibold tracking-normal text-foreground">Install Artifact</h1>
			<p class="mt-2 max-w-2xl text-sm leading-6 text-muted-foreground">Download and extract the healthy Windows FXServer artifact recommended by the JG Scripts artifacts database.</p>
		</div>
		<Button variant="outline" onclick={refreshMetadata} disabled={busy || installing} title="Refresh the recommended Windows artifact from JG Scripts">
			<RefreshCwIcon class={busy ? "animate-spin" : undefined} />
			Refresh Metadata
		</Button>
	</div>

	{#if error}
		<Notice tone="error" message={error} onDismiss={() => (error = "")} class="px-4 py-3 text-sm" />
	{:else if message}
		<Notice tone="success" message={message} onDismiss={() => (message = "")} class="px-4 py-3 text-sm" />
	{/if}

	<div class="grid gap-4 xl:grid-cols-12">
		<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-transform duration-300 hover:-translate-y-0.5 xl:col-span-7">
			<div
				class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"
			></div>
			<Card.Header class="border-b border-border pb-4">
				<div class="flex items-center gap-3">
					<div class="flex size-9 shrink-0 items-center justify-center rounded-sm border border-sky-400/30 bg-sky-400/10 text-sky-200">
						<HardDriveDownloadIcon class="size-4" />
					</div>
					<div>
						<Card.Title>Windows Artifact Install</Card.Title>
						<Card.Description>Choose the server folder and extract the latest healthy Windows build into it.</Card.Description>
					</div>
				</div>
			</Card.Header>
			<Card.Content class="space-y-4">
				<div class="grid gap-3 md:grid-cols-[1fr_auto]">
					<label class="grid gap-2">
						<span class="text-xs font-medium text-muted-foreground">Install Folder</span>
						<Input value={installPath} oninput={updateInstallPath} placeholder="C:\FXServer\server" title="Folder where FXServer artifact files will be extracted." class="rounded-sm font-mono" />
					</label>
					<div class="flex items-end">
						<Button variant="outline" onclick={chooseFolder} disabled={installing} title="Pick the FXServer install folder">
							<FolderOpenIcon />
							Browse
						</Button>
					</div>
				</div>

				<div class="grid gap-3 md:grid-cols-3">
					<div class="rounded-sm border border-border bg-background/70 p-3">
						<p class="text-xs text-muted-foreground">Installed Build</p>
						<p class="mt-1 font-mono text-xl font-semibold text-foreground">{health.currentVersion ?? (installed?.installed ? "Unknown" : "None")}</p>
					</div>
					<div class="rounded-sm border border-border bg-background/70 p-3">
						<p class="text-xs text-muted-foreground">Recommended Build</p>
						<p class="mt-1 font-mono text-xl font-semibold text-foreground">{metadata?.recommendedArtifact ?? "..."}</p>
					</div>
					<div class="rounded-sm border border-border bg-background/70 p-3">
						<p class="text-xs text-muted-foreground">Update Status</p>
						<p class={`mt-1 text-xl font-semibold ${artifactUrgencyTextClass(health.urgency)}`}>{health.label}</p>
					</div>
				</div>

				<div class={`rounded-sm border px-3 py-2 text-sm ${artifactUrgencyClass(health.urgency)}`}>
					{health.description}
				</div>

				{#if metadata}
					<div class="rounded-sm border border-border bg-background/60 p-3">
						<p class="text-xs font-medium text-muted-foreground">Windows Download URL</p>
						<p class="mt-2 break-all font-mono text-xs text-muted-foreground">{metadata.windowsDownloadLink}</p>
					</div>
				{/if}

				<Button class="w-full rounded-sm" onclick={install} disabled={installing || busy || !metadata || !installPath.trim()} title="Download and extract the recommended Windows artifact">
					{#if installing}
						<LoaderCircleIcon class="animate-spin" />
						Installing...
					{:else}
						<DownloadIcon />
						{installed?.installed ? "Update Windows Artifact" : "Install Windows Artifact"}
					{/if}
				</Button>
			</Card.Content>
		</Card.Root>

		<div class="grid gap-4 xl:col-span-5">
			<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-transform duration-300 hover:-translate-y-0.5">
				<div
					class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"
				></div>
				<Card.Header class="border-b border-border pb-4">
					<Card.Title>Before Installing</Card.Title>
					<Card.Description>Quick checks to avoid a bad update.</Card.Description>
				</Card.Header>
				<Card.Content class="space-y-3">
					{#each checklist as item}
						<div class="flex items-start gap-2 rounded-sm border border-border bg-background/70 px-3 py-2 text-sm text-muted-foreground">
							<ShieldCheckIcon class="mt-0.5 size-4 shrink-0 text-emerald-300" />
							<p>{item}</p>
						</div>
					{/each}
				</Card.Content>
			</Card.Root>

			<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-transform duration-300 hover:-translate-y-0.5">
				<div
					class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"
				></div>
				<Card.Header class="border-b border-border pb-4">
					<Card.Title>Install Result</Card.Title>
					<Card.Description>Written after a successful local extraction.</Card.Description>
				</Card.Header>
				<Card.Content>
					{#if result}
						<div class="space-y-2 text-sm">
							<p class="rounded-sm border border-emerald-400/30 bg-emerald-400/10 px-3 py-2 font-medium text-emerald-200">Artifact {result.version} installed.</p>
							<p class="break-all font-mono text-xs text-muted-foreground">{result.destination}</p>
							<p class="break-all font-mono text-xs text-muted-foreground">Marker: {result.markerPath}</p>
						</div>
					{:else}
						<div class="flex min-h-40 items-center justify-center px-4 text-center text-sm text-muted-foreground">No artifact has been installed from this page yet.</div>
					{/if}
				</Card.Content>
			</Card.Root>
		</div>
	</div>
</section>
