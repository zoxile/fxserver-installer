<script lang="ts">
	import type { Snippet } from "svelte";
	import { cn } from "$lib/utils.js";
	import type { HTMLAttributes } from "svelte/elements";
	import type { ChartConfig } from "./index.js";

	type Props = HTMLAttributes<HTMLDivElement> & {
		config: ChartConfig;
		class?: string;
		children?: Snippet;
	};

	let { config, class: className, children, style, ...restProps }: Props = $props();

	const colorStyle = $derived(
		Object.entries(config)
			.filter(([, value]) => value.color)
			.map(([key, value]) => `--color-${key}: ${value.color}`)
			.join("; "),
	);
</script>

<div
	data-chart=""
	class={cn(
		"flex aspect-auto justify-center text-xs text-muted-foreground [&_.layerchart-axis-line]:stroke-border [&_.layerchart-axis-tick-line]:stroke-border [&_.layerchart-axis-tick-text]:fill-muted-foreground [&_.layerchart-grid-line]:stroke-border/60 [&_.layerchart-tooltip]:rounded-sm [&_.layerchart-tooltip]:border-border [&_.layerchart-tooltip]:bg-popover [&_.layerchart-tooltip]:text-popover-foreground",
		className,
	)}
	style={[style, colorStyle].filter(Boolean).join("; ")}
	{...restProps}
>
	{@render children?.()}
</div>
