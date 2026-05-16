<script lang="ts">
	import DatabaseIcon from "@lucide/svelte/icons/database";
	import Titlebar from "./Titlebar.svelte";
	import Sidebar from "$lib/components/layout/Sidebar.svelte";
	import ArtifactInfoPage from "$lib/features/artifacts/ArtifactInfoPage.svelte";
	import ArtifactInstallPage from "$lib/features/artifacts/ArtifactInstallPage.svelte";
	import ClientLogsPage from "$lib/features/client-logs/ClientLogsPage.svelte";
	import ConfigureServerPage from "$lib/features/fxserver/ConfigureServerPage.svelte";
	import ConfiguratorPage from "$lib/features/configurator/ConfiguratorPage.svelte";
	import FxserverLogsPage from "$lib/features/fxserver/FxserverLogsPage.svelte";
	import HomePage from "$lib/features/home/HomePage.svelte";
	import ManageServerPage from "$lib/features/fxserver/ManageServerPage.svelte";
	import JooatResolverPage from "$lib/features/jooat/JooatResolverPage.svelte";
	import JsonFormatterPage from "$lib/features/json/JsonFormatterPage.svelte";
	import LogViewerPage from "$lib/features/app-logs/LogViewerPage.svelte";
	import MariaDBPanel from "$lib/features/mariadb/MariaDBPanel.svelte";
	import PlaceholderPage from "$lib/features/placeholder/PlaceholderPage.svelte";
	import ProfilerPage from "$lib/features/profiler/ProfilerPage.svelte";
	import ArrowDownIcon from "@lucide/svelte/icons/arrow-down";
	import ArrowUpIcon from "@lucide/svelte/icons/arrow-up";
	import type { PageId } from "$lib/navigation";
	import { getPageLabel } from "$lib/navigation";
	import { initializeLogger, log } from "$lib/core/logger.svelte";
	import { onMount } from "svelte";

	let scrollContainer: HTMLElement;
	let isNearBottom = $state(false);
	let sidebarCollapsed = $state(false);
	let activePage = $state<PageId>("home");
	let navigationFrame = 0;

	const placeholders: Record<
		Exclude<PageId, "home" | "mariadb" | "artifact-install" | "artifact-info" | "server-manage" | "server-configure" | "server-logs" | "json-formatter" | "profiler" | "jooat" | "logs" | "client-logs" | "configurator">,
		string
	> = {};

	onMount(() => {
		void (async () => {
			await initializeLogger();
			log("Application shell mounted.", { level: "debug", scope: "app" });
		})();
	});

	function navigate(page: PageId) {
		if (page === activePage) return;
		log(`Navigating to ${getPageLabel(page)}.`, { scope: "navigation", detail: `${activePage} -> ${page}` });

		if (navigationFrame) {
			cancelAnimationFrame(navigationFrame);
		}

		navigationFrame = requestAnimationFrame(() => {
			activePage = page;
			navigationFrame = 0;
		});
	}

	function handleScroll() {
		if (!scrollContainer) return;

		const threshold = 120;

		isNearBottom = scrollContainer.scrollHeight - scrollContainer.scrollTop - scrollContainer.clientHeight < threshold;
	}

	function toggleScroll() {
		if (!scrollContainer) return;

		scrollContainer.scrollTo({
			top: isNearBottom ? 0 : scrollContainer.scrollHeight,
			behavior: "smooth",
		});
	}
</script>

<Titlebar />

<main class="dark h-screen overflow-hidden bg-background text-foreground">
	<div class="flex h-screen">
		<Sidebar {activePage} collapsed={sidebarCollapsed} onNavigate={navigate} onToggle={() => (sidebarCollapsed = !sidebarCollapsed)} />

		<section bind:this={scrollContainer} onscroll={handleScroll} class="min-w-0 flex-1 overflow-y-auto pt-9 scrollbar-hidden">
			<div class="border-b border-border bg-card px-4 py-3 lg:hidden">
				<div class="flex items-center gap-2 text-sm font-semibold">
					<DatabaseIcon class="size-4 text-muted-foreground" />
					{getPageLabel(activePage)}
				</div>
			</div>

			<div class="mx-auto max-w-7xl px-4 pt-6 pb-12 sm:px-6 lg:px-8">
				{#if activePage === "home"}
					<HomePage onNavigate={navigate} />
				{:else if activePage === "mariadb"}
					<MariaDBPanel />
				{:else if activePage === "artifact-install"}
					<ArtifactInstallPage />
				{:else if activePage === "artifact-info"}
					<ArtifactInfoPage />
				{:else if activePage === "server-manage"}
					<ManageServerPage />
				{:else if activePage === "server-configure"}
					<ConfigureServerPage />
				{:else if activePage === "server-logs"}
					<FxserverLogsPage />
				{:else if activePage === "json-formatter"}
					<JsonFormatterPage />
				{:else if activePage === "profiler"}
					<ProfilerPage />
				{:else if activePage === "jooat"}
					<JooatResolverPage />
				{:else if activePage === "logs"}
					<LogViewerPage />
				{:else if activePage === "client-logs"}
					<ClientLogsPage />
				{:else if activePage === "configurator"}
					<ConfiguratorPage />
				{:else}
					<PlaceholderPage title={getPageLabel(activePage)} description={placeholders[activePage]} />
				{/if}
			</div>
		</section>
	</div>
	<button
		onclick={toggleScroll}
		class="fixed bottom-12 right-12 z-50 flex size-10 items-center justify-center rounded-sm border border-border backdrop-blur-sm text-foreground shadow-sm transition-all hover:bg-accent"
		title={isNearBottom ? "Scroll to top" : "Scroll to bottom"}
	>
		{#if isNearBottom}
			<ArrowUpIcon class="size-4" />
		{:else}
			<ArrowDownIcon class="size-4" />
		{/if}
	</button>
</main>
