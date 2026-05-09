<script lang="ts">
	import AlertCircleIcon from "@lucide/svelte/icons/alert-circle";
	import CheckCircle2Icon from "@lucide/svelte/icons/check-circle-2";
	import LightbulbIcon from "@lucide/svelte/icons/lightbulb";

	type Props = {
		type: "success" | "error" | "repair";
		title: string;
		description: string;
	};

	let { type, title, description }: Props = $props();
</script>

<div
	class={[
		"flex items-start gap-3 rounded-md border bg-card px-3 py-3 text-sm shadow-sm",
		type === "error" && "border-destructive/40 text-destructive",
		type === "repair" && "border-amber-500/35 text-amber-300",
		type === "success" && "border-border text-foreground",
	]}
	role={type === "error" ? "alert" : "status"}
>
	{#if type === "error"}
		<AlertCircleIcon class="mt-0.5 size-4 shrink-0" />
	{:else if type === "repair"}
		<LightbulbIcon class="mt-0.5 size-4 shrink-0" />
	{:else}
		<CheckCircle2Icon class="mt-0.5 size-4 shrink-0 text-emerald-400" />
	{/if}
	<div class="min-w-0">
		<p class="font-medium">{title}</p>
		<p class={["mt-1 break-words text-xs", type === "error" ? "text-destructive" : "text-muted-foreground"]}>
			{description}
		</p>
	</div>
</div>
