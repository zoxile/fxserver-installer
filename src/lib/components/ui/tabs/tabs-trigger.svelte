<script lang="ts">
	import { Tabs as TabsPrimitive } from "bits-ui";
	import { cn } from "$lib/utils.js";
	let { ref = $bindable(null), class: className, ...restProps }: TabsPrimitive.TriggerProps = $props();
</script>

<TabsPrimitive.Trigger bind:ref data-slot="tabs-trigger" class={cn("border-b-2 border-transparent px-1 py-3 text-sm font-medium capitalize text-muted-foreground outline-none data-[state=active]:border-primary data-[state=active]:text-foreground focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50", className)} {...restProps} />
