<script lang="ts">
	import type { Component } from "svelte";
	import * as Card from "$lib/components/ui/card/index.js";

	type Tone = "default" | "success" | "warn" | "error" | "info";

	type Props = {
		label: string;
		value: string;
		description: string;
		icon: Component;
		tone?: Tone;
	};

	let { label, value, description, icon: Icon, tone = "default" }: Props = $props();

	function toneClass(currentTone: Tone) {
		return {
			default: "border-border bg-muted text-muted-foreground",
			success: "border-emerald-400/30 bg-emerald-400/10 text-emerald-200",
			warn: "border-amber-400/30 bg-amber-400/10 text-amber-200",
			error: "border-red-400/30 bg-red-400/10 text-red-200",
			info: "border-sky-400/30 bg-sky-400/10 text-sky-200",
		}[currentTone];
	}
</script>

<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-transform duration-300 hover:-translate-y-0.5">
	<div class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"></div>
	<Card.Content class="flex items-start gap-3 p-4">
		<div class={`flex size-9 shrink-0 items-center justify-center rounded-sm border ${toneClass(tone)}`}>
			<Icon class="size-4" />
		</div>
		<div class="min-w-0">
			<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">{label}</p>
			<p class="mt-1 truncate text-2xl font-semibold text-foreground">{value}</p>
			<p class="mt-1 text-xs leading-5 text-muted-foreground">{description}</p>
		</div>
	</Card.Content>
</Card.Root>
