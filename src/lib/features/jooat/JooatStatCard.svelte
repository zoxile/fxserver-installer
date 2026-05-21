<script lang="ts">
	import type { Component } from "svelte";
	import * as Card from "$lib/components/ui/card/index.js";

	type Props = {
		label: string;
		value: string;
		description: string;
		icon: Component;
	};

	let { label, value, description, icon: Icon }: Props = $props();
</script>

<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-[box-shadow,transform] duration-500 ease-[cubic-bezier(0.22,1,0.36,1)] hover:-translate-y-0.5">
	<div class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"></div>
	<Card.Content class="flex items-start justify-between gap-4 p-4">
		<div class="min-w-0">
			<p class="text-xs text-muted-foreground">{label}</p>
			<p class="mt-2 truncate text-xl font-semibold text-foreground">{value}</p>
			<p class="mt-1 truncate text-xs text-muted-foreground">{description}</p>
		</div>
		<div class="flex size-9 shrink-0 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
			<Icon class="size-4" />
		</div>
	</Card.Content>
</Card.Root>
