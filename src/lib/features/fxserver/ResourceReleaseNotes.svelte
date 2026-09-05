<script lang="ts">
	import ExternalLinkIcon from "@lucide/svelte/icons/external-link";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import { Button } from "$lib/components/ui/button/index.js";
	import { openExternalUrl } from "$lib/core/openExternal";
	import { fetchResourceRelease, githubReleaseRepository, type ResourceRelease } from "$lib/modules/resourceRelease";
	let { repository }: { repository: string } = $props();
	let release = $state<ResourceRelease | null>(null);
	let loading = $state(false);
	let loaded = $state(false);
	let error = $state("");
	const slug = $derived(githubReleaseRepository(repository));
	async function load(refresh = false) {
		if (loading || loaded && !refresh) return;
		loading = true; error = "";
		try { release = await fetchResourceRelease(repository, refresh); loaded = true; }
		catch (caught) { error = String(caught); }
		finally { loading = false; }
	}
</script>

{#if slug}
	<details class="min-w-0 border-y border-border py-3" ontoggle={(event) => { if (event.currentTarget.open) void load(); }}>
		<summary class="cursor-pointer text-sm font-medium">Release Notes</summary>
		<div class="mt-3 space-y-3">
			<div class="flex flex-wrap items-center justify-between gap-2"><p class="text-xs text-muted-foreground">Latest published release; the preview uses the selected repository branch.</p><div class="flex gap-2"><Button size="icon" variant="ghost" title="Refresh release notes" aria-label="Refresh release notes" disabled={loading} onclick={() => load(true)}><RefreshCwIcon class={loading ? "animate-spin" : undefined} /></Button><Button size="sm" variant="outline" onclick={() => openExternalUrl(release?.url ?? `https://github.com/${slug}/releases`)}><ExternalLinkIcon />Releases</Button></div></div>
			{#if loading}<p class="text-xs text-muted-foreground">Loading release notes...</p>{:else if error}<p class="break-words text-xs text-amber-300">{error}</p>{:else if release}<p class="break-words text-sm font-medium">{release.title} ({release.tag})</p><pre class="max-h-48 overflow-auto whitespace-pre-wrap break-words font-sans text-xs text-muted-foreground">{release.body || "No release notes supplied."}</pre>{:else if loaded}<p class="text-xs text-muted-foreground">No published release was found.</p>{/if}
		</div>
	</details>
{/if}
