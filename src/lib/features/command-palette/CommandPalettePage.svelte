<script lang="ts">
	import SearchIcon from "@lucide/svelte/icons/search";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Checkbox } from "$lib/components/ui/checkbox/index.js";
	import { commandDefinitions, commandEnabled, commandPaletteSettings, saveCommandPaletteSettings, setCommandEnabled } from "$lib/core/commandPalette.svelte";
</script>

<section class="space-y-6">
	<div>
		<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">Tools</p>
		<h1 class="mt-2 text-3xl font-semibold tracking-normal text-foreground">Command Palette</h1>
		<p class="mt-2 max-w-2xl text-sm leading-6 text-muted-foreground">Configure the commands shown in the global `Ctrl + K` launcher.</p>
	</div>

	<Card.Root class="rounded-md border-border bg-card shadow-sm">
		<Card.Header class="border-b border-border pb-4">
			<div class="flex items-center gap-3">
				<div class="flex size-9 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
					<SearchIcon class="size-4" />
				</div>
				<div>
					<Card.Title>Palette Behavior</Card.Title>
					<Card.Description>Turn the launcher on or off and choose which commands appear.</Card.Description>
				</div>
			</div>
		</Card.Header>
		<Card.Content class="space-y-4">
			<label class="flex cursor-pointer items-center justify-between gap-4 rounded-sm border border-border bg-background/70 p-3">
				<span>
					<span class="block text-sm font-medium text-foreground">Enable global command palette</span>
					<span class="mt-1 block text-xs text-muted-foreground">When enabled, `Ctrl + K` opens the launcher.</span>
				</span>
				<Checkbox
					checked={commandPaletteSettings.enabled}
					onCheckedChange={(checked) => {
						commandPaletteSettings.enabled = Boolean(checked);
						saveCommandPaletteSettings();
					}}
				/>
			</label>

			<div class="grid gap-2">
				{#each commandDefinitions as command}
					<label class="flex cursor-pointer items-center justify-between gap-4 rounded-sm border border-border bg-background/60 px-3 py-2">
						<span class="min-w-0">
							<span class="block truncate text-sm font-medium text-foreground">{command.title}</span>
							<span class="mt-1 block truncate text-xs text-muted-foreground">{command.category} - {command.description}</span>
						</span>
						<Checkbox checked={commandEnabled(command.id)} onCheckedChange={(checked) => setCommandEnabled(command.id, Boolean(checked))} />
					</label>
				{/each}
			</div>
		</Card.Content>
	</Card.Root>
</section>
