<script lang="ts">
	import type { Component } from "svelte";

	type Props = {
		icon: Component;
		label: string;
		active?: boolean;
		collapsed?: boolean;
		labelVisible?: boolean;
		size?: "default" | "child";
		onclick: () => void;
		expanded?: boolean;
	};

	let {
		icon: Icon,
		label,
		active = false,
		collapsed = false,
		labelVisible = !collapsed,
		size = "default",
		onclick,
		expanded,
	}: Props = $props();
</script>

<button
	class={[
		"group flex w-full items-center rounded-sm text-left font-medium leading-none transition-[background-color,color,padding,gap] duration-300 ease-out",
		size === "default" ? "h-10 text-sm" : "h-8 text-xs",
		collapsed ? (size === "default" ? "gap-0 px-4" : "justify-center gap-0 px-0") : size === "default" ? "gap-3 px-2" : "gap-3 px-2",
		active
			? size === "default"
				? "bg-sidebar-primary text-sidebar-primary-foreground shadow-sm"
				: "bg-sidebar-accent text-sidebar-accent-foreground"
			: "text-muted-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
	]}
	{onclick}
	aria-current={active ? "page" : undefined}
	aria-expanded={expanded}
	title={label}
>
	<span class={["flex shrink-0 items-center justify-center leading-none transition-transform duration-300 ease-out", size === "default" ? "size-4" : "size-3.5"]}>
		<Icon class={size === "default" ? "size-4" : "size-3.5"} />
	</span>
	<span
		class={[
			"block h-4 min-w-0 overflow-hidden truncate leading-4 transition-[max-width,opacity,transform] duration-200 ease-out",
			labelVisible ? "max-w-40 translate-x-0 opacity-100" : "max-w-0 -translate-x-1 opacity-0",
		]}
	>
		{label}
	</span>
</button>
