<script lang="ts">
	import DatabaseIcon from "@lucide/svelte/icons/database";
	import Titlebar from "./Titlebar.svelte";
	import Sidebar from "$lib/components/layout/Sidebar.svelte";
	import HomePage from "$lib/features/home/HomePage.svelte";
	import MariaDBPanel from "$lib/features/mariadb/MariaDBPanel.svelte";
	import PlaceholderPage from "$lib/features/placeholder/PlaceholderPage.svelte";
	import type { PageId } from "$lib/navigation";
	import { getPageLabel } from "$lib/navigation";

	let sidebarCollapsed = $state(false);
	let activePage = $state<PageId>("home");

	const placeholders: Record<Exclude<PageId, "home" | "mariadb">, string> = {
		"artifact-install": "Download and prepare the selected FXServer artifact.",
		"artifact-info": "Inspect artifact metadata, recommended builds, and known broken versions.",
		server: "Configure and launch the FXServer setup flow.",
		configurator: "Build and edit server configuration files.",
		profiler: "View captured profiler output and spot server performance issues.",
		"json-formatter": "Format, validate, and inspect JSON resources.",
	};

	function navigate(page: PageId) {
		activePage = page;
	}
</script>

<Titlebar />

<main class="dark min-h-screen bg-background text-foreground">
	<div class="flex min-h-[calc(100vh-2.25rem)] pt-9">
		<Sidebar
			activePage={activePage}
			collapsed={sidebarCollapsed}
			onNavigate={navigate}
			onToggle={() => (sidebarCollapsed = !sidebarCollapsed)}
		/>

		<section class="h-screen min-w-0 flex-1 overflow-y-auto pt-9 scrollbar-hidden">
			<div class="border-b border-border bg-card px-4 py-3 lg:hidden">
				<div class="flex items-center gap-2 text-sm font-semibold">
					<DatabaseIcon class="size-4 text-muted-foreground" />
					{getPageLabel(activePage)}
				</div>
			</div>

			<div class="mx-auto max-w-7xl px-4 py-6 sm:px-6 lg:px-8">
				{#if activePage === "home"}
					<HomePage onNavigate={navigate} />
				{:else if activePage === "mariadb"}
					<MariaDBPanel />
				{:else}
					<PlaceholderPage title={getPageLabel(activePage)} description={placeholders[activePage]} />
				{/if}
			</div>
		</section>
	</div>
</main>
