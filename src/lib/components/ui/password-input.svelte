<script lang="ts">
	import EyeIcon from "@lucide/svelte/icons/eye";
	import EyeOffIcon from "@lucide/svelte/icons/eye-off";
	import type { HTMLInputAttributes } from "svelte/elements";
	import { Button } from "$lib/components/ui/button";
	import { Input } from "$lib/components/ui/input";
	import { cn, type WithElementRef } from "$lib/utils.js";

	type Props = WithElementRef<Omit<HTMLInputAttributes, "files" | "type">, HTMLInputElement>;

	let {
		ref = $bindable(null),
		value = $bindable(),
		class: className,
		disabled,
		"aria-label": ariaLabel,
		...restProps
	}: Props = $props();

	let visible = $state(false);
	const toggleLabel = $derived(visible ? "Hide password" : "Show password");
</script>

<div class="relative">
	<Input
		bind:ref
		bind:value
		type={visible ? "text" : "password"}
		class={cn("pr-10", className)}
		{disabled}
		aria-label={ariaLabel}
		{...restProps}
	/>
	<Button
		type="button"
		variant="ghost"
		size="icon-xs"
		class="absolute right-1.5 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
		{disabled}
		aria-label={toggleLabel}
		title={toggleLabel}
		onclick={() => {
			visible = !visible;
		}}
	>
		{#if visible}
			<EyeOffIcon />
		{:else}
			<EyeIcon />
		{/if}
	</Button>
</div>
