<script lang="ts">
	import type { Component } from "svelte";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";

	type Props = {
		title: string;
		description: string;
		icon: Component;
		size?: "feature" | "default";
		actionLabel?: string;
		onclick: () => void;
	};

	let {
		title,
		description,
		icon: Icon,
		size = "default",
		actionLabel = "Open",
		onclick,
	}: Props = $props();
</script>

<Card.Root
	class={[
		"flex h-full min-h-44 flex-col rounded-md border-border bg-card shadow-sm",
		size === "feature" && "md:col-span-2",
	]}
>
	<Card.Header class="border-b border-border pb-4">
		<div class="flex items-start gap-3">
			<div class="flex size-9 shrink-0 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
				<Icon class="size-4" />
			</div>
			<div class="min-w-0">
				<Card.Title>{title}</Card.Title>
				<Card.Description>{description}</Card.Description>
			</div>
		</div>
	</Card.Header>
	<Card.Content class="flex flex-1 flex-col justify-between gap-5">
		{#if title === "MariaDB"}
			<div class="grid gap-2 text-sm text-muted-foreground sm:grid-cols-3">
				<div class="rounded-sm border border-border bg-background p-3">
					<p class="font-medium text-foreground">Detect</p>
					<p class="mt-1 text-xs">Find the local service.</p>
				</div>
				<div class="rounded-sm border border-border bg-background p-3">
					<p class="font-medium text-foreground">Configure</p>
					<p class="mt-1 text-xs">Prepare users and grants.</p>
				</div>
				<div class="rounded-sm border border-border bg-background p-3">
					<p class="font-medium text-foreground">Query</p>
					<p class="mt-1 text-xs">Inspect live data.</p>
				</div>
			</div>
		{/if}

		<Button variant="outline" {onclick} title={`${actionLabel} ${title}`} class="w-fit">
			{actionLabel}
		</Button>
	</Card.Content>
</Card.Root>
