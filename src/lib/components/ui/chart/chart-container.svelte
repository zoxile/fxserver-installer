<script lang="ts">
	import "layerchart/core.css";
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
		"flex aspect-auto justify-center text-xs text-muted-foreground [&_.lc-axis-rule]:stroke-border [&_.lc-axis-tick]:stroke-border [&_.lc-axis-tick-label]:fill-muted-foreground",
		className,
	)}
	style={[style, colorStyle].filter(Boolean).join("; ")}
	{...restProps}
>
	{@render children?.()}
</div>
