<script lang="ts">
	import PlayIcon from "@lucide/svelte/icons/play";
	import TerminalIcon from "@lucide/svelte/icons/terminal";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import * as Select from "$lib/components/ui/select/index.js";
	import type { MariaDBQueryResult } from "$lib/modules/mariadb";

	type Props = {
		busy: boolean;
		canExecute: boolean;
		databases: string[];
		selectedDatabase: string;
		query: string;
		result: MariaDBQueryResult | null;
		onExecute: () => void;
	};

	let { busy, canExecute, databases, selectedDatabase = $bindable(), query = $bindable(), result, onExecute }: Props = $props();
	const globalQueryValue = "__global__";
	const databaseOptions = $derived([
		{ value: globalQueryValue, label: "Global query" },
		...databases.map((database) => ({ value: database, label: database })),
	]);
</script>

<Card.Root class="h-full rounded-md border-border bg-card shadow-sm">
	<Card.Header class="border-b border-border pb-4">
		<div class="flex items-center justify-between gap-4">
			<div class="flex min-w-0 items-center gap-3">
				<div class="flex size-9 shrink-0 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
					<TerminalIcon class="size-5" />
				</div>
				<div>
					<Card.Title>Query Console</Card.Title>
					<Card.Description>Run SQL against MariaDB using the connection above.</Card.Description>
				</div>
			</div>
			<Button onclick={onExecute} disabled={busy || !canExecute} title={canExecute ? "Execute the SQL query and show returned rows" : "Apply valid admin credentials before running queries"}>
				<PlayIcon />
				Execute
			</Button>
		</div>
	</Card.Header>

	<Card.Content class="space-y-4">
		<div class="grid gap-2">
			<span class="text-xs font-medium text-muted-foreground">Query Scope</span>
			<Select.Root bind:value={selectedDatabase} type="single" items={databaseOptions} disabled={!canExecute}>
				<Select.Trigger title="Choose whether the query runs globally or inside one database" class="w-full rounded-sm font-mono text-xs">
					{selectedDatabase === globalQueryValue ? "Global query" : selectedDatabase || "Choose database"}
				</Select.Trigger>
				<Select.Content class="rounded-sm">
					{#each databaseOptions as option}
						<Select.Item value={option.value} label={option.label}>
							{option.label}
						</Select.Item>
					{/each}
				</Select.Content>
			</Select.Root>
		</div>

		<textarea
			bind:value={query}
			spellcheck="false"
			placeholder="SELECT * FROM users LIMIT 25;"
			title="SQL query to execute against MariaDB."
			disabled={!canExecute}
			class="min-h-40 w-full resize-y rounded-sm border border-input bg-background px-3 py-3 font-mono text-sm shadow-xs outline-none transition-[color,box-shadow] placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
		></textarea>

		{#if result}
			{#if result.success && result.columns.length}
				<div class="max-h-72 overflow-auto rounded-sm border border-border bg-background">
					<table class="w-full border-collapse text-left text-xs">
						<thead class="sticky top-0 bg-muted text-muted-foreground">
							<tr>
								{#each result.columns as column}
									<th class="border-b border-border px-3 py-2 font-medium whitespace-nowrap">{column}</th>
								{/each}
							</tr>
						</thead>
						<tbody>
							{#each result.rows as row}
								<tr class="border-b border-border/70 last:border-0">
									{#each result.columns as _, index}
										<td class="px-3 py-2 align-top font-mono whitespace-nowrap">{row[index] ?? ""}</td>
									{/each}
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			{:else}
				<pre
					class={[
						"max-h-72 overflow-auto rounded-sm border p-4 font-mono text-xs leading-6 whitespace-pre-wrap",
						result.success
							? "border-border bg-muted/40 text-foreground"
							: "border-destructive/30 bg-destructive/10 text-destructive",
					]}
				>{result.stdout || result.stderr || "No output."}</pre>
			{/if}
		{/if}
	</Card.Content>
</Card.Root>
