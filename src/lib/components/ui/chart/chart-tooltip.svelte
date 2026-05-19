<script lang="ts">
	import { Tooltip as LayerTooltip } from "layerchart";

	type TooltipItem = {
		key?: string;
		label?: string;
		value?: unknown;
		color?: string;
	};

	type Props = {
		indicator?: "dot" | "line" | "dashed";
		labelFormatter?: (value: unknown) => string;
		valueFormatter?: (value: unknown, item?: TooltipItem) => string;
	};

	let { indicator = "dot", labelFormatter, valueFormatter }: Props = $props();

	function itemsFromData(data: unknown): TooltipItem[] {
		if (Array.isArray(data)) return data as TooltipItem[];
		if (data && typeof data === "object" && "data" in data && Array.isArray((data as { data?: unknown }).data)) {
			return (data as { data: TooltipItem[] }).data;
		}
		return [];
	}

	function labelFromData(data: unknown) {
		const value = data && typeof data === "object" && "x" in data ? (data as { x?: unknown }).x : undefined;
		return labelFormatter?.(value) ?? (value instanceof Date ? value.toLocaleTimeString() : String(value ?? ""));
	}

	function itemLabel(item: TooltipItem) {
		return item.label ?? item.key ?? "";
	}

	function itemValue(item: TooltipItem) {
		return valueFormatter?.(item.value, item) ?? String(item.value ?? "");
	}
</script>

<LayerTooltip.Root
	classes={{
		root: "z-50",
		container: "rounded-sm border border-border bg-popover/95 px-3 py-2 text-xs text-popover-foreground shadow-md backdrop-blur",
		content: "grid gap-2",
	}}
>
	{#snippet children({ data }: { data: unknown })}
		<div class="grid gap-2">
			<div class="font-medium text-foreground">{labelFromData(data)}</div>
			<div class="grid gap-1.5">
				{#each itemsFromData(data) as item}
					<div class="flex min-w-36 items-center justify-between gap-4">
						<div class="flex items-center gap-2">
							<span
								class={`block ${indicator === "line" ? "h-0.5 w-3" : indicator === "dashed" ? "h-0.5 w-3 border-t border-dashed" : "size-2 rounded-full"}`}
								style={`background: ${item.color ?? "currentColor"}; border-color: ${item.color ?? "currentColor"}`}
							></span>
							<span class="text-muted-foreground">{itemLabel(item)}</span>
						</div>
						<span class="font-mono text-foreground">{itemValue(item)}</span>
					</div>
				{/each}
			</div>
		</div>
	{/snippet}
</LayerTooltip.Root>
