<script lang="ts">
	import { ToggleGroup as ToggleGroupPrimitive } from "bits-ui";
	import { cn } from "$lib/utils.js";

	type Props = ToggleGroupPrimitive.RootProps & {
		class?: string;
	};

	let {
		class: className,
		ref = $bindable(null),
		children,
		...restProps
	}: Props = $props();
</script>

<ToggleGroupPrimitive.Root bind:ref class={cn("flex items-center gap-1", className)} {...restProps}>
	{@render children?.()}
</ToggleGroupPrimitive.Root>
