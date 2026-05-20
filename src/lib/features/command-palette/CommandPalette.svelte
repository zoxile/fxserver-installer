<script lang="ts">
	import SearchIcon from "@lucide/svelte/icons/search";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { commandDefinitions, commandEnabled, commandPaletteSettings } from "$lib/core/commandPalette.svelte";
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
			selectedIndex = Math.min(visibleCommands.length - 1, selectedIndex + 1);
			return;
		}
		if (event.key === "ArrowUp") {
			event.preventDefault();
			selectedIndex = Math.max(0, selectedIndex - 1);
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
	<div class="fixed inset-0 z-100 p-4">
		<button class="absolute inset-0 bg-black/60 backdrop-blur-sm" onclick={onClose} aria-label="Close command palette"></button>
		<div class="relative mx-auto mt-20 max-w-2xl overflow-hidden rounded-md border border-border bg-card shadow-2xl" role="dialog" aria-modal="true" tabindex="-1">
			<div class="flex items-center gap-3 border-b border-border px-4 py-3">
				<SearchIcon class="size-4 text-muted-foreground" />
				<Input bind:value={query} autofocus placeholder="Search commands..." class="border-0 bg-transparent px-0 shadow-none focus-visible:ring-0" />
				<Button variant="outline" size="sm" onclick={onClose}>Esc</Button>
			</div>
			<div class="max-h-112 overflow-auto p-2">
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
							<span class="shrink-0 rounded-sm border border-border bg-background px-2 py-1 text-[10px] font-semibold uppercase text-muted-foreground">{command.category}</span>
						</button>
					{/each}
				{:else}
					<div class="p-8 text-center text-sm text-muted-foreground">No enabled commands match your search.</div>
				{/if}
			</div>
		</div>
	</div>
{/if}
