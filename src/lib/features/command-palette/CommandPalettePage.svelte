<script lang="ts">
	import KeyboardIcon from "@lucide/svelte/icons/keyboard";
	import SearchIcon from "@lucide/svelte/icons/search";
	import XIcon from "@lucide/svelte/icons/x";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Checkbox } from "$lib/components/ui/checkbox/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import {
		commandDefinitions,
		commandEnabled,
		commandPaletteSettings,
		getCommandShortcut,
		saveCommandPaletteSettings,
		setCommandEnabled,
		setCommandShortcut,
		shortcutFromKeyboardEvent,
	} from "$lib/core/commandPalette.svelte";

	let listeningFor = $state("");

	function captureShortcut(commandId: string, event: KeyboardEvent) {
		event.preventDefault();
		event.stopPropagation();

		if (event.key === "Escape") {
			listeningFor = "";
			return;
		}

		if (event.key === "Backspace" || event.key === "Delete") {
			setCommandShortcut(commandId, "");
			listeningFor = "";
			return;
		}

		const shortcut = shortcutFromKeyboardEvent(event);
		if (!shortcut) return;

		setCommandShortcut(commandId, shortcut);
		listeningFor = "";
	}
</script>

<section class="space-y-6">
	<div>
		<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">Tools</p>
		<h1 class="mt-2 text-3xl font-semibold tracking-normal text-foreground">Command Palette</h1>
		<p class="mt-2 max-w-2xl text-sm leading-6 text-muted-foreground">Configure the commands shown in the global Ctrl + K launcher and assign direct shortcuts.</p>
	</div>

	<Card.Root class="rounded-md border-border bg-card shadow-sm">
		<Card.Header class="border-b border-border pb-4">
			<div class="flex items-center gap-3">
				<div class="flex size-9 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
					<SearchIcon class="size-4" />
				</div>
				<div>
					<Card.Title>Palette Behavior</Card.Title>
					<Card.Description>Turn the launcher on or off, choose which commands appear, and assign keybinds.</Card.Description>
				</div>
			</div>
		</Card.Header>
		<Card.Content class="space-y-4">
			<label class="flex cursor-pointer items-center justify-between gap-4 rounded-sm border border-border bg-background/70 p-3">
				<span>
					<span class="block text-sm font-medium text-foreground">Enable global command palette</span>
					<span class="mt-1 block text-xs text-muted-foreground">When enabled, Ctrl + K opens the launcher.</span>
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
					<div class="grid gap-3 rounded-sm border border-border bg-background/60 px-3 py-3 lg:grid-cols-[minmax(0,1fr)_15rem_auto] lg:items-center">
						<label class="flex min-w-0 cursor-pointer items-center gap-3">
							<Checkbox checked={commandEnabled(command.id)} onCheckedChange={(checked) => setCommandEnabled(command.id, Boolean(checked))} />
							<span class="min-w-0">
								<span class="block truncate text-sm font-medium text-foreground">{command.title}</span>
								<span class="mt-1 block truncate text-xs text-muted-foreground">{command.category} - {command.description}</span>
							</span>
						</label>
						<div class="relative">
							<KeyboardIcon class="pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />
							<Input
								readonly
								value={listeningFor === command.id ? "Press keys..." : getCommandShortcut(command.id) || "No shortcut"}
								class={[
									"h-9 rounded-sm pl-8 pr-2 font-mono text-xs",
									listeningFor === command.id ? "border-primary/60 bg-primary/10 text-primary" : getCommandShortcut(command.id) ? "text-foreground" : "text-muted-foreground",
								]}
								onfocus={() => (listeningFor = command.id)}
								onclick={() => (listeningFor = command.id)}
								onkeydown={(event) => captureShortcut(command.id, event)}
								title="Click and press a shortcut"
							/>
						</div>
						<Button
							variant="ghost"
							size="icon-sm"
							class="text-muted-foreground transition-colors hover:bg-destructive/10 hover:text-destructive disabled:hover:bg-transparent disabled:hover:text-muted-foreground"
							onclick={() => setCommandShortcut(command.id, "")}
							disabled={!getCommandShortcut(command.id)}
							title="Clear shortcut"
						>
							<XIcon />
						</Button>
					</div>
				{/each}
			</div>
		</Card.Content>
	</Card.Root>
</section>
