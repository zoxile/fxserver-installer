<script lang="ts">
	import AlertCircleIcon from "@lucide/svelte/icons/alert-circle";
	import CheckCircle2Icon from "@lucide/svelte/icons/check-circle-2";
	import XIcon from "@lucide/svelte/icons/x";
	import { Button } from "$lib/components/ui/button/index.js";

	type Tone = "success" | "error" | "warn" | "info";

	let {
		tone = "info",
		title = "",
		message,
		onDismiss,
		class: className = "",
	}: {
		tone?: Tone;
		title?: string;
		message: string;
		onDismiss?: () => void;
		class?: string;
	} = $props();

	const toneClass = $derived(
		({
			success: "border-emerald-400/30 bg-emerald-400/10 text-emerald-100",
			error: "border-red-400/30 bg-red-400/10 text-red-100",
			warn: "border-amber-400/30 bg-amber-400/10 text-amber-100",
			info: "border-border bg-background/70 text-muted-foreground",
		})[tone],
	);
</script>

<div class={`rounded-sm border px-3 py-2 text-xs ${toneClass} ${className}`} role={tone === "error" ? "alert" : "status"}>
	<div class="flex items-start gap-2">
		{#if tone === "success"}
			<CheckCircle2Icon class="mt-0.5 size-3.5 shrink-0" />
		{:else}
			<AlertCircleIcon class="mt-0.5 size-3.5 shrink-0" />
		{/if}
		<div class="min-w-0 flex-1">
			{#if title}
				<p class="font-medium">{title}</p>
			{/if}
			<p class="wrap-break-word">{message}</p>
		</div>
		{#if onDismiss}
			<Button variant="ghost" size="icon-xs" onclick={onDismiss} title="Dismiss notification">
				<XIcon />
			</Button>
		{/if}
	</div>
</div>
