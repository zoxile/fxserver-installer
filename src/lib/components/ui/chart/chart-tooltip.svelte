<script lang="ts">
	import { Tooltip as LayerTooltip } from "layerchart";

	type TooltipItem = {
		key?: string;
		label?: string;
		name?: string;
		value?: unknown;
		y?: unknown;
		color?: string;
		data?: Record<string, unknown>;
	};

	type Props = {
		indicator?: "dot" | "line" | "dashed";
		dataKey?: string;
		label?: string;
		labelFormatter?: (value: unknown) => string;
		valueFormatter?: (value: unknown, item?: TooltipItem) => string;
	};

	let { indicator = "dot", dataKey, label, labelFormatter, valueFormatter }: Props = $props();

	function getRootData(data: unknown): Record<string, unknown> | undefined {
		if (!data || typeof data !== "object") return undefined;

		const obj = data as Record<string, unknown>;

		if (obj.data && typeof obj.data === "object" && !Array.isArray(obj.data)) {
			return obj.data as Record<string, unknown>;
		}

		return obj;
	}

	function itemsFromData(data: unknown): TooltipItem[] {
		if (!data || typeof data !== "object") return [];

		const obj = data as Record<string, unknown>;

		if (Array.isArray(obj.data)) return obj.data as TooltipItem[];
		if (Array.isArray(obj.items)) return obj.items as TooltipItem[];
		if (Array.isArray(obj.points)) return obj.points as TooltipItem[];
		if (Array.isArray(obj.series)) return obj.series as TooltipItem[];

		const rootData = getRootData(data);

		if (dataKey && rootData && dataKey in rootData) {
			return [
				{
					key: dataKey,
					label,
					value: rootData[dataKey],
					data: rootData,
					color: dataKey === "cpu" ? "var(--color-cpu)" : dataKey === "memory" ? "var(--color-memory)" : undefined,
				},
			];
		}

		return [];
	}

	function labelFromData(data: unknown) {
		const value = data && typeof data === "object" && "x" in data ? (data as { x?: unknown }).x : undefined;

		return labelFormatter?.(value) ?? (value instanceof Date ? value.toLocaleTimeString() : String(value ?? ""));
	}

	function itemLabel(item: TooltipItem) {
		return item.label ?? item.name ?? item.key ?? "";
	}

	function itemValue(item: TooltipItem) {
		const value = item.value ?? item.y ?? (dataKey ? item.data?.[dataKey] : undefined);

		return valueFormatter?.(value, item) ?? String(value ?? "");
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
								class={`block ${indicator === "line" ? "h-0.5 w-3" : indicator === "dashed" ? "h-0.5 w-3 border-t border-dashed" : "size-2 rounded-xs"}`}
								style={`background: ${item.color ?? "currentColor"}; border-color: ${item.color ?? "currentColor"}`}
							></span>
							<span class="font-medium" style={`color: ${item.color ?? "currentColor"}`}>
								{itemLabel(item)}
							</span>
						</div>
						<span class="font-mono text-foreground">{itemValue(item)}</span>
					</div>
				{/each}
			</div>
		</div>
	{/snippet}
</LayerTooltip.Root>
