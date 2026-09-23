<script lang="ts">
	import { Checkbox } from "$lib/components/ui/checkbox/index.js";

	type Props = {
		nativePassword?: boolean;
		disabled?: boolean;
	};

	let { nativePassword = $bindable(false), disabled = false }: Props = $props();
	const id = $props.id();
</script>

<div class="min-w-0 space-y-2 border-t border-border pt-4">
	<div class="flex items-start gap-3">
		<Checkbox id={id} bind:checked={nativePassword} {disabled} aria-describedby={`${id}-help`} class="mt-0.5" />
		<label for={id} class="min-w-0 text-sm leading-5 font-medium wrap-anywhere text-foreground">
			Use <span class="font-mono text-xs">mysql_native_password</span> authentication (optional)
		</label>
	</div>
	<div id={`${id}-help`} class="space-y-1 pl-7 text-xs leading-5 text-muted-foreground">
		<p>Leave off to preserve existing authentication. Enable only if your connector requires it.</p>
		<p>Enabling replaces existing methods and requires a password. Uses older SHA-1 authentication; avoid for new high-security setups.</p>
	</div>
</div>
