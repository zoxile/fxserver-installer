<script lang="ts">
	import AlertTriangleIcon from "@lucide/svelte/icons/alert-triangle";
	import ArchiveIcon from "@lucide/svelte/icons/archive";
	import CheckCircle2Icon from "@lucide/svelte/icons/check-circle-2";
	import LoaderCircleIcon from "@lucide/svelte/icons/loader-circle";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import { onMount } from "svelte";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { getInstallPath, loadInstallPath } from "$lib/core/paths.svelte";
	import { log } from "$lib/core/logger";
	import {
		fetchArtifactMetadata,
		getArtifactHealthStatus,
		getInstalledWindowsArtifactInfo,
		type ArtifactHealthStatus,
		type ArtifactMetadata,
		type InstalledArtifactInfo,
	} from "$lib/modules/artifact";
	import { artifactUrgencyClass, artifactUrgencyTextClass } from "$lib/features/artifacts/artifactUi";
	import type { PageId } from "$lib/navigation";

	type Props = {
		onNavigate: (page: PageId) => void;
	};

	let { onNavigate }: Props = $props();
	let metadata = $state<ArtifactMetadata | null>(null);
	let installed = $state<InstalledArtifactInfo | null>(null);
	let busy = $state(false);
	let error = $state("");

	const health = $derived<ArtifactHealthStatus>(getArtifactHealthStatus(metadata, installed));
	const installPath = $derived(installed?.destination || getInstallPath() || "No server folder selected");

	onMount(() => {
		loadInstallPath();
		void refresh();
	});

	async function refresh() {
		busy = true;
		error = "";

		try {
			const path = getInstallPath();
			const [nextMetadata, nextInstalled] = await Promise.all([
				fetchArtifactMetadata(),
				path ? getInstalledWindowsArtifactInfo(path) : Promise.resolve(null),
			]);
			metadata = nextMetadata;
			installed = nextInstalled;
		} catch (caught) {
			error = caught instanceof Error ? caught.message : String(caught);
			log("Home artifact status refresh failed.", { level: "error", scope: "home.artifacts", detail: error });
		} finally {
			busy = false;
		}
	}
</script>

<Card.Root class="group relative overflow-hidden rounded-md border-border bg-card shadow-sm">
	<div class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"></div>
	<Card.Header class="border-b border-border pb-4">
		<div class="flex items-start justify-between gap-3">
			<div class="flex items-start gap-3">
				<div class={`flex size-9 shrink-0 items-center justify-center rounded-sm border ${artifactUrgencyClass(health.urgency)}`}>
					{#if busy}
						<LoaderCircleIcon class="size-4 animate-spin" />
					{:else if health.urgency === "needed" || health.urgency === "recommended"}
						<AlertTriangleIcon class="size-4" />
					{:else if health.urgency === "none"}
						<CheckCircle2Icon class="size-4" />
					{:else}
						<ArchiveIcon class="size-4" />
					{/if}
				</div>
				<div class="min-w-0">
					<Card.Title>Installed Artifact</Card.Title>
					<Card.Description class="mt-1">Current Windows artifact health for the selected server folder.</Card.Description>
				</div>
			</div>
			<Button variant="ghost" size="icon-sm" onclick={refresh} disabled={busy} title="Refresh installed artifact status">
				<RefreshCwIcon class={busy ? "animate-spin" : undefined} />
			</Button>
		</div>
	</Card.Header>
	<Card.Content class="space-y-3">
		<div class="grid gap-2">
			<div class="flex items-center justify-between gap-3 rounded-sm border border-border bg-background/70 px-3 py-2">
				<span class="text-xs text-muted-foreground">Status</span>
				<span class={`truncate text-xs font-semibold ${artifactUrgencyTextClass(health.urgency)}`}>{health.label}</span>
			</div>
			<div class="grid gap-2 sm:grid-cols-2">
				<div class="rounded-sm border border-border bg-background/70 px-3 py-2">
					<p class="text-xs text-muted-foreground">Installed</p>
					<p class="mt-1 truncate font-mono text-sm font-semibold text-foreground">{health.currentVersion ?? (installed?.installed ? "Unknown" : "None")}</p>
				</div>
				<div class="rounded-sm border border-border bg-background/70 px-3 py-2">
					<p class="text-xs text-muted-foreground">Recommended</p>
					<p class="mt-1 truncate font-mono text-sm font-semibold text-foreground">{health.recommendedVersion ?? "..."}</p>
				</div>
			</div>
		</div>

		<p class="text-xs leading-5 text-muted-foreground">{error || health.description}</p>
		<p class="truncate font-mono text-[11px] text-muted-foreground">{installPath}</p>

		<div class="flex flex-wrap gap-2">
			<Button variant="outline" size="sm" class="rounded-sm" onclick={() => onNavigate("artifact-install")} title="Open the artifact installer">
				{health.urgency === "needed" || health.urgency === "recommended" ? "Update Artifact" : "Open Installer"}
			</Button>
			<Button variant="outline" size="sm" class="rounded-sm" onclick={() => onNavigate("artifact-info")} title="Open artifact information">
				View Details
			</Button>
		</div>
	</Card.Content>
</Card.Root>
