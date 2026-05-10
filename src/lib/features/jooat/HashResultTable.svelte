<script lang="ts">
	import CopyIcon from "@lucide/svelte/icons/copy";
	import { Button } from "$lib/components/ui/button/index.js";
	import type { HashResult } from "./joaat";

	type Props = {
		rows: HashResult[];
		onCopy: (value: string, label: string) => void;
	};

	let { rows, onCopy }: Props = $props();
</script>

<div class="overflow-hidden rounded-sm border border-border bg-background/70">
	<div class="overflow-x-auto">
		<table class="w-full min-w-[720px] text-left text-sm">
			<thead class="border-b border-border bg-muted/40 text-xs text-muted-foreground">
				<tr>
					<th class="px-3 py-2 font-medium">Input</th>
					<th class="px-3 py-2 font-medium">Hex</th>
					<th class="px-3 py-2 font-medium">Unsigned</th>
					<th class="px-3 py-2 font-medium">Signed</th>
					<th class="w-10 px-3 py-2 font-medium">Copy</th>
				</tr>
			</thead>
			<tbody>
				{#each rows as row}
					<tr class="border-b border-border/70 last:border-0">
						<td class="max-w-72 px-3 py-2">
							<p class="truncate font-medium text-foreground">{row.input}</p>
							<p class="truncate text-xs text-muted-foreground">{row.normalized}</p>
						</td>
						<td class="px-3 py-2 font-mono text-xs text-primary">{row.hex}</td>
						<td class="px-3 py-2 font-mono text-xs">{row.unsigned}</td>
						<td class="px-3 py-2 font-mono text-xs">{row.signed}</td>
						<td class="px-3 py-2">
							<Button variant="ghost" size="icon-sm" onclick={() => onCopy(row.hex, `${row.input} hash`)} title={`Copy ${row.input} hex hash`}>
								<CopyIcon class="size-3.5" />
							</Button>
						</td>
					</tr>
				{:else}
					<tr>
						<td colspan="5" class="px-3 py-8 text-center text-sm text-muted-foreground">Add names above to generate JOOAT hashes.</td>
					</tr>
				{/each}
			</tbody>
		</table>
	</div>
</div>
