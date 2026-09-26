<script lang="ts">
    import EyeIcon from "@lucide/svelte/icons/eye";
    import SaveIcon from "@lucide/svelte/icons/save";
    import XIcon from "@lucide/svelte/icons/x";
    import { Button } from "$lib/components/ui/button/index.js";
    import { Checkbox } from "$lib/components/ui/checkbox/index.js";
    import ConfigDiff from "$lib/features/config-history/ConfigDiff.svelte";
    import { canSaveReview, type ConfigChangeReview } from "$lib/modules/configValidation";

    let { review, saving, onSave, onCancel }: {
        review: ConfigChangeReview;
        saving: boolean;
        onSave: (diffReviewed: boolean, warningsReviewed: boolean) => void;
        onCancel: () => void;
    } = $props();
    let revealed = $state(false);
    let diffReviewed = $state(false);
    let warningsReviewed = $state(false);
    const errors = $derived(review.report.issues.filter((issue) => issue.severity === "error"));
    const warnings = $derived(review.report.issues.filter((issue) => issue.severity === "warning"));
    $effect(() => { review; revealed; diffReviewed = false; warningsReviewed = false; });
</script>

<section class="min-w-0 space-y-3 border-y border-border py-4" aria-label="Configuration change review">
    <div class="flex flex-wrap items-center justify-between gap-2">
        <h3 class="text-sm font-semibold">Configuration Change Validation</h3>
        <Button variant="ghost" size="icon-sm" title="Close review" aria-label="Close review" disabled={saving} onclick={onCancel}><XIcon /></Button>
    </div>
    <p class="text-xs text-muted-foreground" role="status">{errors.length} errors, {warnings.length} advisory warnings / {review.report.fileCount} files checked</p>
    {#if review.report.issues.length}
        <ul class="max-h-64 space-y-2 overflow-y-auto text-xs">
            {#each [...errors, ...warnings] as issue}
                <li class="break-words border-l-2 pl-3" class:border-destructive={issue.severity === "error"} class:border-amber-400={issue.severity === "warning"}>
                    <span class="font-semibold">{issue.severity === "error" ? "Error" : "Advisory"}: {issue.file}{issue.line ? `, line ${issue.line}` : ""}</span>
                    <p class="mt-1 text-muted-foreground">{issue.message}</p>
                </li>
            {/each}
        </ul>
    {/if}
    <label class="flex items-center gap-2 text-xs"><Checkbox bind:checked={revealed} disabled={saving} /><EyeIcon class="size-4 shrink-0" />Reveal exact diff, including secrets</label>
    {#if revealed}<ConfigDiff before={review.before} after={review.after} beforeLabel="Loaded disk version" afterLabel="Draft to save" />{/if}
    <label class="flex items-center gap-2 text-xs"><Checkbox bind:checked={diffReviewed} disabled={!revealed || saving || errors.length > 0} />I reviewed this exact file change.</label>
    {#if warnings.length}
        <label class="flex items-start gap-2 text-xs"><Checkbox bind:checked={warningsReviewed} disabled={!revealed || saving || errors.length > 0} />I reviewed the advisory warnings and accept them for this save only.</label>
    {/if}
    {#if errors.length}<p class="text-xs text-destructive">Resolve all errors before saving. Errors cannot be overridden.</p>{/if}
    <div class="flex flex-wrap gap-2">
        <Button size="sm" disabled={saving || !revealed || !canSaveReview(review.report, diffReviewed, warningsReviewed)} onclick={() => onSave(diffReviewed, warningsReviewed)}><SaveIcon />{saving ? "Checking and saving" : "Save reviewed change"}</Button>
        <Button size="sm" variant="outline" disabled={saving} onclick={onCancel}>Back to editing</Button>
    </div>
</section>
