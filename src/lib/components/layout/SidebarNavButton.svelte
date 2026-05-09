<script lang="ts">
	import type { Component } from "svelte";

	type Props = {
		icon: Component;
		label: string;
		active?: boolean;
		collapsed?: boolean;
		size?: "default" | "child";
		onclick: () => void;
		expanded?: boolean;
	};

	let {
		icon: Icon,
		label,
		active = false,
		collapsed = false,
		size = "default",
		onclick,
		expanded,
	}: Props = $props();
</script>

<button
	class={[
		"flex w-full items-center gap-3 rounded-sm text-left font-medium transition-colors",
		size === "default" ? "h-10 px-3 text-sm" : "h-8 px-2 text-xs",
		collapsed && "justify-center px-0",
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
	<Icon class={size === "default" ? "size-4" : "size-3.5"} />
	{#if !collapsed}
		<span class="truncate">{label}</span>
	{/if}
</button>
