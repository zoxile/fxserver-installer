<script lang="ts">
	import { onMount } from "svelte";
	import CheckIcon from "@lucide/svelte/icons/check";
	import XIcon from "@lucide/svelte/icons/x";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import type { AdminPreview } from "$lib/modules/databaseAdmin";

	type Props = {
		preview: AdminPreview;
		busy: boolean;
		onConfirm: (confirmation: string) => void;
		onCancel: () => void;
	};
	let { preview, busy, onConfirm, onCancel }: Props = $props();
	let confirmation = $state("");
	let now = $state(Date.now());
	const expired = $derived(now >= preview.expiresAt);
	onMount(() => {
		const timer = setInterval(() => now = Date.now(), 1000);
		return () => clearInterval(timer);
	});
</script>

<section class="space-y-3 border-y border-amber-500/50 py-4" aria-label="Administrative action confirmation">
	<h3 class="text-base font-semibold">Pending Administration</h3>
	<p class="wrap-anywhere text-sm font-mono">{preview.host}:{preview.port} / {preview.confirmation}</p>
	<p class="text-xs text-amber-500">{preview.warning}</p>
	<pre class="max-h-56 overflow-auto border border-border bg-muted/30 p-3 text-xs whitespace-pre-wrap wrap-anywhere">{preview.sql};</pre>
	<p class="text-xs text-muted-foreground" role="status">
		{expired ? "Preview expired. Cancel and review again." : `Expires ${new Date(preview.expiresAt).toLocaleTimeString()}`}
	</p>
	<div class="flex flex-wrap items-end gap-3">
		<label class="grid min-w-0 flex-1 gap-2 text-xs">
			Confirm {preview.confirmation}
			<Input bind:value={confirmation} autocomplete="off" maxlength={520} disabled={busy || expired} />
		</label>
		<Button
			variant="destructive" disabled={busy || expired || confirmation !== preview.confirmation}
			onclick={() => onConfirm(confirmation)}
		>
			<CheckIcon />Confirm Administration
		</Button>
		<Button variant="outline" disabled={busy} onclick={onCancel}>
			<XIcon />Cancel Preview
		</Button>
	</div>
</section>
