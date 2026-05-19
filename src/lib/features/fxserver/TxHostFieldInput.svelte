<script lang="ts">
	import { Input } from "$lib/components/ui/input/index.js";
	import PasswordInput from "$lib/components/ui/password-input.svelte";
	import * as Select from "$lib/components/ui/select/index.js";
	import type { TxHostField } from "./fxserverEnv";

	let {
		field,
		value = $bindable(""),
		sensitive = false,
	}: {
		field: TxHostField;
		value: string;
		sensitive?: boolean;
	} = $props();
</script>

<label class="grid gap-2 rounded-sm border border-border bg-background/60 p-3">
	<span class="flex items-center justify-between gap-3">
		<span class="text-xs font-semibold text-foreground">{field.label}</span>
		<span class="font-mono text-[10px] text-muted-foreground">{sensitive ? "not saved" : field.key}</span>
	</span>
	<span class="text-xs leading-5 text-muted-foreground">{field.description}</span>

	{#if field.type === "select"}
		<Select.Root bind:value type="single" items={(field.options ?? []).map((option) => ({ value: option, label: option || "Unset" }))}>
			<Select.Trigger title={`Set ${field.key}`} class="w-full rounded-sm font-mono text-xs">
				{value || "Unset"}
			</Select.Trigger>
			<Select.Content class="rounded-sm">
				{#each field.options ?? [] as option}
					<Select.Item value={option} label={option || "Unset"}>
						{option || "Unset"}
					</Select.Item>
				{/each}
			</Select.Content>
		</Select.Root>
	{:else if field.type === "password"}
		<PasswordInput bind:value placeholder={field.placeholder} title={`Set ${field.key}`} class="rounded-sm font-mono text-xs" />
	{:else}
		<Input
			bind:value
			type={field.type === "number" ? "number" : field.type === "url" ? "url" : "text"}
			placeholder={field.placeholder}
			title={`Set ${field.key}`}
			class="rounded-sm font-mono text-xs"
		/>
	{/if}
</label>
