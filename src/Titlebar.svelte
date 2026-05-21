<script lang="ts">
	import { onMount } from "svelte";
	import { getCurrentWindow } from "@tauri-apps/api/window";

	import icon from "./assets/icon128x128.png";

	let appTitle = $state("FXServer Installer");

	let minimizeBtn: HTMLButtonElement;
	let maximizeBtn: HTMLButtonElement;
	let closeBtn: HTMLButtonElement;

	onMount(() => {
		let appWindow: ReturnType<typeof getCurrentWindow> | null = null;

		try {
			appWindow = getCurrentWindow();
		} catch {
			return;
		}

		(async () => {
			appTitle = (await appWindow?.title()) ?? appTitle;
		})();

		minimizeBtn.onclick = () => appWindow?.minimize();
		maximizeBtn.onclick = () => appWindow?.toggleMaximize();
		closeBtn.onclick = () => appWindow?.close();
	});
</script>

<div data-tauri-drag-region class="fixed top-0 right-0 left-0 z-50 flex h-9 items-center justify-between border-b border-border bg-background/50 text-foreground backdrop-blur-md select-none">
	<div data-tauri-drag-region class="flex items-center gap-2 pl-2">
		<img src={icon} alt="icon" class="size-4" />

		<span class="text-[13px] font-medium opacity-80">
			{appTitle}
		</span>
	</div>

	<div class="flex">
		<button bind:this={minimizeBtn} title="Minimize" class="flex h-9 w-[42px] items-center justify-center transition-colors hover:bg-muted">
			<svg width="14" height="14" viewBox="0 0 24 24">
				<path fill="currentColor" d="M19 13H5v-2h14z" />
			</svg>
		</button>

		<button bind:this={maximizeBtn} title="Maximize" class="flex h-9 w-[42px] items-center justify-center transition-colors hover:bg-muted">
			<svg width="14" height="14" viewBox="0 0 24 24">
				<path fill="currentColor" d="M4 4h16v16H4zm2 4v10h12V8z" />
			</svg>
		</button>

		<button bind:this={closeBtn} title="Close to tray" class="flex h-9 w-[42px] items-center justify-center transition-colors hover:bg-red-500 hover:text-white">
			<svg width="14" height="14" viewBox="0 0 24 24">
				<path fill="currentColor" d="M13.46 12L19 17.54V19h-1.46L12 13.46L6.46 19H5v-1.46L10.54 12L5 6.46V5h1.46L12 10.54L17.54 5H19v1.46z" />
			</svg>
		</button>
	</div>
</div>
