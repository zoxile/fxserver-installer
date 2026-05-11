<script lang="ts">
	import { getContext, type Snippet } from "svelte";
	import { cn, type WithElementRef } from "$lib/utils.js";
	import type { HTMLButtonAttributes } from "svelte/elements";

	type ToggleGroupContext = {
		type: () => "single" | "multiple";
		value: () => string | string[] | undefined;
		disabled: () => boolean;
		toggle: (value: string) => void;
	};

	type Props = WithElementRef<HTMLButtonAttributes, HTMLButtonElement> & {
		class?: string;
		value: string;
		children?: Snippet<[{ pressed: boolean }]>;
	};

	let {
		class: className,
		ref = $bindable(null),
		value,
		disabled = false,
		children,
		onclick,
		...restProps
	}: Props = $props();

	const group = getContext<ToggleGroupContext>("ui-toggle-group");
	const pressed = $derived(Boolean(
		group.type() === "multiple"
			? Array.isArray(group.value()) && group.value()?.includes(value)
			: group.value() === value,
	));
	const itemDisabled = $derived(disabled || group.disabled());
</script>

<button
	bind:this={ref}
	type="button"
	disabled={itemDisabled}
	aria-pressed={pressed}
	data-state={pressed ? "on" : "off"}
	class={cn(
		"inline-flex h-8 items-center justify-center rounded-sm border border-border bg-background px-2 text-xs font-medium text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/40 data-[state=on]:border-primary/40 data-[state=on]:bg-primary/15 data-[state=on]:text-primary disabled:pointer-events-none disabled:opacity-50",
		className,
	)}
	onclick={(event) => {
		onclick?.(event);
		if (!event.defaultPrevented && !itemDisabled) group.toggle(value);
	}}
	{...restProps}
>
	{@render children?.({ pressed })}
</button>
