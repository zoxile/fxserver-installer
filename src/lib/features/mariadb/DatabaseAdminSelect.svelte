<script lang="ts">
	import * as Select from "$lib/components/ui/select/index.js";

	type Props = {
		value?: string;
		options: { value: string; label: string }[];
		label: string;
		disabled?: boolean;
		onChange?: (value: string) => void;
	};
	let { value = $bindable(""), options, label, disabled = false, onChange }: Props = $props();
</script>

<Select.Root type="single" bind:value items={options} {disabled} onValueChange={onChange}>
	<Select.Trigger class="w-full min-w-0" aria-label={label}>
		<span class="truncate">{options.find((option) => option.value === value)?.label ?? value}</span>
	</Select.Trigger>
	<Select.Content>
		{#each options as option}
			<Select.Item value={option.value} label={option.label}>{option.label}</Select.Item>
		{/each}
	</Select.Content>
</Select.Root>
