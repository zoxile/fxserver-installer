<script lang="ts">
	import { ToggleGroup as ToggleGroupPrimitive } from "bits-ui";
	import { cn } from "$lib/utils.js";

	type Props = ToggleGroupPrimitive.ItemProps & {
		class?: string;
	};

	let {
		class: className,
		ref = $bindable(null),
		children,
		...restProps
	}: Props = $props();
</script>

<ToggleGroupPrimitive.Item
	bind:ref
	class={cn(
		"inline-flex h-8 items-center justify-center rounded-sm border border-border bg-background px-2 text-xs font-medium text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/40 data-[state=on]:border-primary/40 data-[state=on]:bg-primary/15 data-[state=on]:text-primary disabled:pointer-events-none disabled:opacity-50",
		className,
	)}
	{...restProps}
>
	{@render children?.({ pressed: false })}
</ToggleGroupPrimitive.Item>
