<script lang="ts">
	import { issueTime, type IssueContext } from "./consoleIssues";
	let { context }: { context: IssueContext } = $props();
</script>

<div class="max-h-80 overflow-auto rounded-sm border border-border bg-black/50 py-1 font-mono text-xs" aria-label="Nearby console lines">
	{#each context.lines as line (line.id)}
		<div class={["grid min-w-0 grid-cols-[4.5rem_2.7rem_minmax(0,1fr)] items-start gap-2 border-l-2 px-2 py-1", line.id === context.anchorId ? "border-primary bg-primary/10" : "border-transparent"]}>
			<time datetime={line.timestamp || undefined} class="text-[10px] leading-5 text-muted-foreground tabular-nums">{issueTime(line.timestamp)}</time>
			<span class={["mt-0.5 truncate rounded-xs border px-1 py-0.5 text-center text-[9px] font-semibold uppercase", line.level === "error" ? "border-red-400/30 text-red-300" : line.level === "warn" ? "border-amber-400/30 text-amber-300" : "border-border text-muted-foreground"]}>
				{line.stream === "stdout" ? "OUT" : line.stream === "stderr" ? "ERR" : line.stream.slice(0, 3)}
			</span>
			<span class="min-w-0 whitespace-pre-wrap break-all leading-5 text-foreground">
				{#each line.segments as segment}<span class={segment.emphasis ? "font-semibold" : undefined} style:color={segment.color ?? undefined}>{segment.text}</span>{/each}
			</span>
		</div>
	{/each}
</div>
