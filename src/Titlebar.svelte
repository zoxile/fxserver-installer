<script lang="ts">
	import { onMount } from "svelte";
	import { getCurrentWindow } from "@tauri-apps/api/window";
	import icon from "./assets/icon128x128.png";

	const appWindow = getCurrentWindow();
	let appTitle = $state("FXServer Installer");

	let minimizeBtn: HTMLButtonElement;
	let maximizeBtn: HTMLButtonElement;
	let closeBtn: HTMLButtonElement;

	onMount(() => {
		(async () => {
			appTitle = await appWindow.title();
		})();

		minimizeBtn.onclick = () => appWindow.minimize();
		maximizeBtn.onclick = () => appWindow.toggleMaximize();
		closeBtn.onclick = () => appWindow.close();
	});
</script>

<div data-tauri-drag-region class="titlebar">
	<div class="left" data-tauri-drag-region>
		<img src={icon} alt="icon" class="icon" />
		<span class="title">{appTitle}</span>
	</div>

	<div class="controls">
		<button bind:this={minimizeBtn} title="Minimize" class="btn">
			<svg width="14" height="14" viewBox="0 0 24 24">
				<path fill="currentColor" d="M19 13H5v-2h14z" />
			</svg>
		</button>

		<button bind:this={maximizeBtn} title="Maximize" class="btn">
			<svg width="14" height="14" viewBox="0 0 24 24">
				<path fill="currentColor" d="M4 4h16v16H4zm2 4v10h12V8z" />
			</svg>
		</button>

		<button bind:this={closeBtn} title="Close" class="btn close">
			<svg width="14" height="14" viewBox="0 0 24 24">
				<path fill="currentColor" d="M13.46 12L19 17.54V19h-1.46L12 13.46L6.46 19H5v-1.46L10.54 12L5 6.46V5h1.46L12 10.54L17.54 5H19v1.46z" />
			</svg>
		</button>
	</div>
</div>

<style>
	.titlebar {
		height: 36px;
		display: flex;
		justify-content: space-between;
		align-items: center;

		background: rgba(9, 9, 11, 0.5);
		color: hsl(0 0% 98%);
		border-bottom: 1px solid hsl(240 3.7% 15.9%);

		user-select: none;
		position: fixed;
		top: 0;
		left: 0;
		right: 0;
	}

	.left {
		display: flex;
		align-items: center;
		gap: 8px;
		padding-left: 8px;
		app-region: drag;
	}

	.icon {
		width: 16px;
		height: 16px;
	}

	.title {
		font-size: 13px;
		opacity: 0.8;
		font-weight: 500;
	}

	.controls {
		display: flex;
	}

	.btn {
		width: 42px;
		height: 36px;

		display: flex;
		align-items: center;
		justify-content: center;

		background: transparent;
		border: none;
		color: inherit;

		transition: background 0.1s ease-in-out;

		app-region: no-drag;
	}

	.btn:hover {
		background: hsl(240 3.7% 15.9%);
	}

	.btn.close:hover {
		background: #ef4444;
		color: white;
	}
</style>
