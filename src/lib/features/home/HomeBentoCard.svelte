<script lang="ts">
	import type { Component } from "svelte";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";

	type Props = {
		title: string;
		description: string;
		icon: Component;
		size?: "hero" | "feature" | "wide" | "compact";
		kicker?: string;
		highlights?: string[];
		className?: string;
		actionLabel?: string;
		actions?: { label: string; onclick: () => void }[];
		onclick: () => void;
	};

	let {
		title,
		description,
		icon: Icon,
		size = "compact",
		kicker,
		highlights = [],
		className = "",
		actionLabel = "Open",
		actions = [],
		onclick,
	}: Props = $props();
</script>

<Card.Root
	class={[
		"group flex h-full min-h-40 flex-col rounded-md border-border bg-card shadow-sm",
		size === "hero" && "min-h-80",
		size === "feature" && "min-h-64",
		size === "wide" && "min-h-40",
		size === "compact" && "min-h-36",
		className,
	]}
>
	<Card.Header class={["shrink-0 border-b border-border pb-3", size === "compact" ? "space-y-1" : "space-y-2"]}>
		<div class="flex items-start gap-3">
			<div class="flex size-9 shrink-0 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border transition-transform duration-200 group-hover:scale-105">
				<Icon class="size-4" />
			</div>
			<div class="min-w-0">
				{#if kicker}
					<p class="mb-1 text-xs font-medium text-muted-foreground">{kicker}</p>
				{/if}
				<Card.Title class={size === "hero" ? "text-xl" : undefined}>{title}</Card.Title>
				<Card.Description
					class={[
						"overflow-hidden [display:-webkit-box] [-webkit-box-orient:vertical]",
						size === "compact" ? "[-webkit-line-clamp:2]" : "[-webkit-line-clamp:3]",
					]}
				>
					{description}
				</Card.Description>
			</div>
		</div>
	</Card.Header>
	<Card.Content class="flex min-h-0 flex-1 flex-col justify-between gap-3">
		{#if highlights.length}
			<div class={["grid min-h-0 gap-2 text-sm text-muted-foreground", size === "hero" ? "sm:grid-cols-3" : "grid-cols-1"]}>
				{#each highlights as highlight}
					<div class="rounded-sm border border-border bg-background/70 p-2.5">
						<p class="font-medium text-foreground">{highlight}</p>
					</div>
				{/each}
			</div>
		{/if}

		<div class="mt-auto flex flex-wrap gap-2">
			{#if actions.length}
				{#each actions as action}
					<Button variant="outline" onclick={action.onclick} title={`${action.label} ${title}`} class="h-8 w-fit shrink-0 rounded-sm px-3 text-xs">
						{action.label}
					</Button>
				{/each}
			{:else}
				<Button variant="outline" {onclick} title={`${actionLabel} ${title}`} class="h-8 w-fit shrink-0 rounded-sm px-3 text-xs">
					{actionLabel}
				</Button>
			{/if}
		</div>
	</Card.Content>
</Card.Root>
