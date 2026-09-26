<script lang="ts">
	import Trash2Icon from "@lucide/svelte/icons/trash-2";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	let { busy, onReset }: { busy: boolean; onReset: (confirmation: string) => boolean } = $props();
	let confirmation = $state("");
</script>

<section class="space-y-2 border-y border-destructive/40 py-3" aria-label="Reset saved database library">
	<p class="text-xs text-destructive">Reset permanently deletes all saved queries, folders, and bookmarks for every workspace and connection on this device. Existing data has not been changed.</p>
	<div class="flex flex-wrap items-end gap-2">
		<label class="grid min-w-0 flex-1 basis-full gap-1 text-xs sm:basis-0">Type RESET<Input bind:value={confirmation} maxlength={5} autocomplete="off" disabled={busy} /></label>
		<Button variant="destructive" size="sm" disabled={busy || confirmation !== "RESET"} onclick={() => { if (onReset(confirmation)) confirmation = ""; }}><Trash2Icon />Reset Library</Button>
	</div>
</section>
