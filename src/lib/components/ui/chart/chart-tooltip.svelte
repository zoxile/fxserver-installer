<script lang="ts">
	import { getChartContext, Tooltip as LayerTooltip, type TooltipSeries } from "layerchart";

	type Props = {
		indicator?: "dot" | "line" | "dashed";
		dataKey?: string;
		label?: string;
		labelFormatter?: (value: unknown) => string;
		valueFormatter?: (value: unknown, item: TooltipSeries) => string;
	};

	let { indicator = "dot", dataKey, label, labelFormatter, valueFormatter }: Props = $props();
	const context = getChartContext();
	const items = $derived(context.tooltip.series.filter((item) => item.visible && (!dataKey || item.key === dataKey)));
	const xValue = $derived(context.tooltip.data ? context.x(context.tooltip.data) : undefined);
	const heading = $derived(labelFormatter?.(xValue) ?? (xValue instanceof Date ? xValue.toLocaleTimeString() : String(xValue ?? "")));
</script>

<LayerTooltip.Root
	portal={false}
	motion="none"
	classes={{
		root: "z-50",
		container: "rounded-sm border border-border bg-popover/95 px-3 py-2 text-xs text-popover-foreground shadow-md backdrop-blur",
		content: "grid gap-2",
	}}
>
	<div class="grid gap-2">
		<div class="font-medium text-foreground">{heading}</div>
		<div class="grid gap-1.5">
			{#each items as item (item.key)}
				<div class="flex min-w-36 items-center justify-between gap-4">
					<div class="flex items-center gap-2">
						<span
							class={`block ${indicator === "line" ? "h-0.5 w-3" : indicator === "dashed" ? "h-0.5 w-3 border-t border-dashed" : "size-2 rounded-xs"}`}
							style={`background: ${item.color ?? "currentColor"}; border-color: ${item.color ?? "currentColor"}`}
						></span>
						<span class="font-medium" style={`color: ${item.color ?? "currentColor"}`}>
							{label ?? item.label}
						</span>
					</div>
					<span class="font-mono text-foreground">{valueFormatter?.(item.value, item) ?? String(item.value ?? "")}</span>
				</div>
			{/each}
		</div>
	</div>
</LayerTooltip.Root>
