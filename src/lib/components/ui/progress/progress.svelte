<script lang="ts">
	import { cn, type WithElementRef } from "$lib/utils.js";
	import type { HTMLAttributes } from "svelte/elements";

	let {
		ref = $bindable(null),
		class: className,
		indicatorClass,
		max = 100,
		value,
		...restProps
	}: WithElementRef<HTMLAttributes<HTMLDivElement>, HTMLDivElement> & {
		value?: number | null;
		max?: number;
		indicatorClass?: string;
	} = $props();

	const percentage = $derived(Math.max(0, Math.min(100, (100 * (value ?? 0)) / (max || 1))));
</script>

<div
	bind:this={ref}
	data-slot="progress"
	role="progressbar"
	aria-valuemin="0"
	aria-valuemax={max}
	aria-valuenow={value ?? 0}
	class={cn("bg-muted relative flex h-1.5 w-full items-center overflow-x-hidden rounded-full", className)}
	{...restProps}
>
	<div data-slot="progress-indicator" class={cn("bg-primary size-full flex-1 transition-all", indicatorClass)} style={`transform: translateX(-${100 - percentage}%)`}></div>
</div>
