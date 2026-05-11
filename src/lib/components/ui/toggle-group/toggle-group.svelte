<script lang="ts">
	import { setContext, type Snippet } from "svelte";
	import { cn, type WithElementRef } from "$lib/utils.js";
	import type { HTMLAttributes } from "svelte/elements";

	type ToggleGroupContext = {
		type: () => "single" | "multiple";
		value: () => string | string[] | undefined;
		disabled: () => boolean;
		toggle: (value: string) => void;
	};

	type Props = WithElementRef<HTMLAttributes<HTMLDivElement>, HTMLDivElement> & {
		class?: string;
		type?: "single" | "multiple";
		value?: string | string[];
		onValueChange?: (value: string | string[]) => void;
		disabled?: boolean;
		children?: Snippet;
	};

	let {
		class: className,
		ref = $bindable(null),
		type = "single",
		value,
		onValueChange,
		disabled = false,
		children,
		...restProps
	}: Props = $props();

	setContext<ToggleGroupContext>("ui-toggle-group", {
		type: () => type,
		value: () => value,
		disabled: () => disabled,
		toggle: (itemValue: string) => {
			if (disabled) return;
			if (type === "multiple") {
				const current = Array.isArray(value) ? value : [];
				const next = current.includes(itemValue) ? current.filter((entry) => entry !== itemValue) : [...current, itemValue];
				onValueChange?.(next);
				return;
			}

			onValueChange?.(value === itemValue ? "" : itemValue);
		},
	});
</script>

<div bind:this={ref} role="group" aria-disabled={disabled} class={cn("flex items-center gap-1", className)} {...restProps}>
	{@render children?.()}
</div>
