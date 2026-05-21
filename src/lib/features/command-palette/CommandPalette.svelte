<script lang="ts">
	import SearchIcon from "@lucide/svelte/icons/search";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { commandDefinitions, commandEnabled, commandPaletteSettings, getCommandShortcut } from "$lib/core/commandPalette.svelte";
	import type { PageId } from "$lib/navigation";

	type Props = {
		open: boolean;
		onClose: () => void;
		onNavigate: (page: PageId) => void;
	};

	let { open, onClose, onNavigate }: Props = $props();
	let query = $state("");
	let selectedIndex = $state(0);

	const visibleCommands = $derived(
		commandDefinitions
			.filter((command) => commandPaletteSettings.enabled && commandEnabled(command.id))
			.filter((command) => {
				const needle = query.trim().toLowerCase();
				if (!needle) return true;
				return [command.title, command.description, command.category, command.page, ...command.keywords].join(" ").toLowerCase().includes(needle);
			})
			.slice(0, 12),
	);

	$effect(() => {
		query;
		selectedIndex = 0;
	});

	function runCommand(page: PageId) {
		onNavigate(page);
		onClose();
		query = "";
	}

	function handleKeydown(event: KeyboardEvent) {
		if (!open) return;
		if (event.key === "Escape") {
			event.preventDefault();
			onClose();
			return;
		}
		if (event.key === "ArrowDown") {
			event.preventDefault();
			if (!visibleCommands.length) return;
			selectedIndex = selectedIndex >= visibleCommands.length - 1 ? 0 : selectedIndex + 1;
			return;
		}
		if (event.key === "ArrowUp") {
			event.preventDefault();
			if (!visibleCommands.length) return;
			selectedIndex = selectedIndex <= 0 ? visibleCommands.length - 1 : selectedIndex - 1;
			return;
		}
		if (event.key === "Enter" && visibleCommands[selectedIndex]) {
			event.preventDefault();
			runCommand(visibleCommands[selectedIndex].page);
		}
	}
</script>

<svelte:window onkeydown={handleKeydown} />

{#if open}
	<div class="fixed inset-0 z-[100] p-4 animate-in fade-in-0 duration-150">
		<button class="absolute inset-0 bg-black/60 backdrop-blur-sm" onclick={onClose} aria-label="Close command palette"></button>
		<div
			class="relative mx-auto mt-20 max-w-2xl overflow-hidden rounded-md border border-border bg-card shadow-2xl animate-in fade-in-0 zoom-in-95 slide-in-from-top-2 duration-150"
			role="dialog"
			aria-modal="true"
			tabindex="-1"
		>
			<div class="border-b border-border p-4">
				<div class="relative">
					<SearchIcon class="pointer-events-none absolute top-1/2 left-3.5 size-4 -translate-y-1/2 text-muted-foreground" />
					<Input bind:value={query} autofocus placeholder="Search commands..." class="h-12 rounded-sm bg-background pr-16 pl-10 text-base shadow-xs" />
					<Button variant="outline" size="xs" class="absolute top-1/2 right-2 h-6 -translate-y-1/2 rounded-sm px-2 font-mono text-[10px] uppercase text-muted-foreground" onclick={onClose}>ESC</Button>
				</div>
			</div>
			<div class="max-h-[28rem] overflow-auto p-2">
				{#if visibleCommands.length}
					{#each visibleCommands as command, index}
						<button
							class={[
								"flex w-full items-start justify-between gap-4 rounded-sm px-3 py-2 text-left transition-colors",
								index === selectedIndex ? "bg-primary/15 text-foreground" : "text-muted-foreground hover:bg-muted/60 hover:text-foreground",
							]}
							onclick={() => runCommand(command.page)}
						>
							<span class="min-w-0">
								<span class="block text-sm font-medium text-foreground">{command.title}</span>
								<span class="mt-1 block truncate text-xs">{command.description}</span>
							</span>
							<span class="flex shrink-0 items-center gap-2">
								{#if getCommandShortcut(command.id)}
									<kbd class="rounded-sm border border-border bg-background px-2 py-1 text-[10px] font-semibold text-muted-foreground">{getCommandShortcut(command.id)}</kbd>
								{/if}
								<span class="rounded-sm border border-border bg-background px-2 py-1 text-[10px] font-semibold uppercase text-muted-foreground">{command.category}</span>
							</span>
						</button>
					{/each}
				{:else}
					<div class="p-8 text-center text-sm text-muted-foreground">No enabled commands match your search.</div>
				{/if}
			</div>
		</div>
	</div>
{/if}
