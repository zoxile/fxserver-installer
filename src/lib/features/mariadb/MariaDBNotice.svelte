<script lang="ts">
	import AlertCircleIcon from "@lucide/svelte/icons/alert-circle";
	import CheckCircle2Icon from "@lucide/svelte/icons/check-circle-2";
	import XIcon from "@lucide/svelte/icons/x";
	import { Button } from "$lib/components/ui/button/index.js";

	type Props = {
		message: string;
		error: string;
		onDismiss: () => void;
	};

	let { message, error, onDismiss }: Props = $props();
	let tone = $derived(error ? "error" : "success");
</script>

{#if message || error}
	<div
		class={["flex items-start gap-3 rounded-md border bg-card px-3 py-3 text-sm shadow-sm", tone === "error" ? "border-destructive/40 text-destructive" : "border-border text-foreground"]}
		role={tone === "error" ? "alert" : "status"}
	>
		{#if tone === "error"}
			<AlertCircleIcon class="mt-0.5 size-4 shrink-0" />
		{:else}
			<CheckCircle2Icon class="mt-0.5 size-4 shrink-0 text-emerald-400" />
		{/if}
		<div class="min-w-0 flex-1">
			<p class="font-medium">{tone === "error" ? "MariaDB error" : "MariaDB notice"}</p>
			<p class={["mt-1 wrap-break-word text-xs", tone === "error" ? "text-destructive" : "text-muted-foreground"]}>
				{error || message}
			</p>
		</div>
		<Button variant="ghost" size="icon-xs" onclick={onDismiss} title="Dismiss notification">
			<XIcon />
		</Button>
	</div>
{/if}
