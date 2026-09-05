<script lang="ts">
    import { onDestroy, untrack } from "svelte";
    import HistoryIcon from "@lucide/svelte/icons/history";
    import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
    import RotateCcwIcon from "@lucide/svelte/icons/rotate-ccw";
    import { Button } from "$lib/components/ui/button/index.js";
    import { Checkbox } from "$lib/components/ui/checkbox/index.js";
    import { Notice } from "$lib/components/ui/notice/index.js";
    import * as Select from "$lib/components/ui/select/index.js";
    import { listConfigHistory, readConfigHistoryVersion, restoreConfigHistoryVersion, type ConfigFileRequest, type ConfigHistoryContent, type ConfigHistoryVersion } from "$lib/modules/configHistory";
    import type { ServerConfigFile } from "$lib/modules/fxserver";
    import ConfigDiff from "./ConfigDiff.svelte";

    let { request, currentContent, hasDraft, disabled = false, onRestored, onBusy }: {
        request: ConfigFileRequest; currentContent: string; hasDraft: boolean; disabled?: boolean;
        onRestored: (file: ServerConfigFile) => void; onBusy: (busy: boolean) => void;
    } = $props();
    let versions = $state<ConfigHistoryVersion[]>([]);
    let selectedId = $state("");
    let selected = $state<ConfigHistoryContent | null>(null);
    let busy = $state(false);
    let restoring = $state(false);
    let revealed = $state(false);
    let reviewed = $state(false);
    let error = $state("");
    let generation = 0;
    let active = true;
    onDestroy(() => { active = false; generation += 1; });
    const label = (version: ConfigHistoryVersion) => `${new Date(version.createdAt).toLocaleString()} / ${version.reason.replaceAll("-", " ")} / ${version.size} B`;

    $effect(() => { currentContent; request; untrack(() => { void refresh(); }); });
    $effect(() => { currentContent; selectedId; revealed; reviewed = false; });

    async function refresh() {
        const id = ++generation;
        busy = true;
        selected = null;
        selectedId = "";
        revealed = false;
        reviewed = false;
        error = "";
        try { const result = await listConfigHistory(request); if (id === generation) versions = result; }
        catch (caught) { if (id === generation) error = String(caught); }
        finally { if (id === generation) busy = false; }
    }

    async function choose(id: string) {
        const requestGeneration = ++generation;
        selectedId = id;
        selected = null;
        revealed = false;
        reviewed = false;
        error = "";
        try { const result = await readConfigHistoryVersion(request, id); if (requestGeneration === generation && selectedId === id) selected = result; }
        catch (caught) { if (requestGeneration === generation && selectedId === id) error = String(caught); }
    }

    async function restore() {
        if (!active || !selected || !reviewed || !revealed || hasDraft || disabled || restoring) return;
        const target = JSON.stringify(request);
        restoring = true;
        onBusy(true);
        error = "";
        try {
            const file = await restoreConfigHistoryVersion({ ...request }, selected.version.id, currentContent);
            if (active && target === JSON.stringify(request)) onRestored(file);
        }
        catch (caught) { error = String(caught); }
        finally { restoring = false; if (active) onBusy(false); }
    }
</script>

<section class="min-w-0 space-y-3 border-t border-border pt-4" aria-label="Configuration history">
    <div class="flex flex-wrap items-center justify-between gap-2">
        <h3 class="flex items-center gap-2 text-sm font-semibold"><HistoryIcon class="size-4" />Configuration History</h3>
        <Button variant="ghost" size="icon-sm" title="Refresh history" aria-label="Refresh history" onclick={refresh} disabled={busy || disabled || restoring}><RefreshCwIcon /></Button>
    </div>
    {#if error}<Notice tone="error" message={error} onDismiss={() => error = ""} />{/if}
    <p class="text-xs text-muted-foreground">Encrypted on this Windows account. Up to 20 versions / 4 MiB per file. Restore requires FXServer to be stopped.</p>
    {#if versions.length}
        <Select.Root type="single" value={selectedId} onValueChange={choose} disabled={busy || disabled || restoring}>
            <Select.Trigger class="w-full min-w-0" aria-label="Configuration version"><span class="truncate">{selected ? label(selected.version) : "Select a version"}</span></Select.Trigger>
            <Select.Content>{#each versions as version}<Select.Item value={version.id}>{label(version)}</Select.Item>{/each}</Select.Content>
        </Select.Root>
        {#if selected}
            <label class="flex items-center gap-2 text-xs"><Checkbox bind:checked={revealed} />Reveal config contents, including secrets</label>
            {#if revealed}<ConfigDiff before={currentContent} after={selected.content} />{/if}
            {#if hasDraft}<p class="text-xs text-amber-400">Save or revert your draft before restoring a version.</p>{/if}
            <label class="flex items-center gap-2 text-xs"><Checkbox bind:checked={reviewed} disabled={!revealed || hasDraft || restoring} />I reviewed this file replacement.</label>
            <Button size="sm" variant="outline" onclick={restore} disabled={!reviewed || !revealed || hasDraft || disabled || restoring || selected.content === currentContent}><RotateCcwIcon />{restoring ? "Restoring" : "Restore selected file"}</Button>
        {/if}
    {:else}<p class="text-xs text-muted-foreground">{busy ? "Loading versions..." : "No saved versions for this file yet."}</p>{/if}
</section>
