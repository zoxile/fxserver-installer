<script lang="ts">
    let { before, after, beforeLabel = "Current file", afterLabel = "Selected version" }: {
        before: string; after: string; beforeLabel?: string; afterLabel?: string;
    } = $props();
    const left = $derived(before.split("\n"));
    const right = $derived(after.split("\n"));
    const changed = $derived(Array.from({ length: Math.max(left.length, right.length) }, (_, index) => index).filter((index) => left[index] !== right[index]));
</script>

<div class="space-y-2">
    <p class="text-xs text-muted-foreground">{before === after ? "Identical content" : `${changed.length} differing line positions`} / {before.includes("\r\n") ? "CRLF" : "LF"} to {after.includes("\r\n") ? "CRLF" : "LF"}</p>
    <div class="grid min-w-0 grid-cols-1 gap-3 lg:grid-cols-2">
        {#each [{ label: beforeLabel, lines: left, other: right, content: before, tone: "bg-red-500/10" }, { label: afterLabel, lines: right, other: left, content: after, tone: "bg-emerald-500/10" }] as side}
            <div class="min-w-0 space-y-2">
                <p class="text-xs font-medium">{side.label}</p>
                {#if side.lines.length <= 2000}
                    <!-- svelte-ignore a11y_no_noninteractive_tabindex (Keyboard users need to scroll the read-only comparison.) -->
                    <div class="h-72 overflow-auto rounded-sm border border-border bg-background font-mono text-xs" role="region" aria-label={side.label} tabindex="0">
                        {#each side.lines as line, index}
                            <div class={`flex min-w-full w-max leading-5 ${line !== side.other[index] ? side.tone : ""}`}>
                                <span class="sticky left-0 w-12 shrink-0 border-r border-border bg-muted px-2 text-right text-muted-foreground select-none">{index + 1}</span>
                                <span class="px-2 whitespace-pre">{line || " "}</span>
                            </div>
                        {/each}
                    </div>
                {:else}
                    <textarea class="h-72 w-full rounded-sm border border-border bg-background p-2 font-mono text-xs" readonly wrap="off" aria-label={side.label} value={side.content}></textarea>
                {/if}
            </div>
        {/each}
    </div>
</div>
