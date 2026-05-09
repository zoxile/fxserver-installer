<script lang="ts">
	import ArchiveIcon from "@lucide/svelte/icons/archive";
	import DatabaseIcon from "@lucide/svelte/icons/database";
	import FolderCogIcon from "@lucide/svelte/icons/folder-cog";
	import PanelLeftCloseIcon from "@lucide/svelte/icons/panel-left-close";
	import PanelLeftOpenIcon from "@lucide/svelte/icons/panel-left-open";
	import ServerCogIcon from "@lucide/svelte/icons/server-cog";
	import Titlebar from "./Titlebar.svelte";
	import MariaDBPanel from "$lib/features/mariadb/MariaDBPanel.svelte";

	let sidebarCollapsed = $state(false);

	const navigation = [
		{
			id: "mariadb",
			label: "MariaDB",
			icon: DatabaseIcon,
			active: true,
		},

		{
			id: "artifact",
			label: "Artifacts",
			icon: ArchiveIcon,
			active: false,
			children: [
				{
					id: "artifact-install",
					label: "Install Artifact",
					active: false,
				},

				{
					id: "artifact-info",
					label: "Artifact Information",
					active: false,
				},
			],
		},

		{
			id: "server",
			label: "FXServer",
			icon: ServerCogIcon,
			active: false,
		},

		{
			id: "tools",
			label: "Tools & Utils",
			icon: FolderCogIcon,
			active: false,
			children: [
				{
					id: "configurator",
					label: "Configurator",
					active: true,
				},

				{
					id: "profiler",
					label: "Display Profiler",
					active: false,
				},

				{
					id: "json-formatter",
					label: "JSON Formatter",
					active: false,
				},
			],
		},
	];
</script>

<Titlebar />

<main class="dark min-h-screen bg-background text-foreground">
	<div class="flex min-h-[calc(100vh-2.25rem)] pt-9">
		<aside
			class={[
				"hidden shrink-0 border-r border-sidebar-border bg-sidebar py-5 text-sidebar-foreground transition-[width] duration-200 lg:block",
				sidebarCollapsed ? "w-16 px-2" : "w-64 px-4",
			]}
		>
			<div class={["mb-6 flex items-start justify-between gap-2", sidebarCollapsed ? "px-0" : "px-2"]}>
				{#if !sidebarCollapsed}
					<div class="min-w-0">
						<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">FXServer Installer</p>
						<h2 class="mt-2 text-lg font-semibold tracking-normal">Setup Workspace</h2>
					</div>
				{/if}
				<button
					class="flex size-9 shrink-0 items-center justify-center rounded-sm text-muted-foreground transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
					onclick={() => (sidebarCollapsed = !sidebarCollapsed)}
					title={sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar"}
				>
					{#if sidebarCollapsed}
						<PanelLeftOpenIcon class="size-4" />
					{:else}
						<PanelLeftCloseIcon class="size-4" />
					{/if}
				</button>
			</div>

			<nav class="space-y-1">
				{#each navigation as item}
					{@const Icon = item.icon}
					<button
						class={[
							"flex h-10 w-full items-center gap-3 rounded-md px-3 text-left text-sm font-medium transition-colors",
							sidebarCollapsed && "justify-center px-0",
							item.active ? "bg-sidebar-primary text-sidebar-primary-foreground shadow-sm" : "text-muted-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
						]}
						disabled={!item.active}
						aria-current={item.active ? "page" : undefined}
						title={item.active ? item.label : `${item.label} is not available yet`}
					>
						<Icon class="size-4" />
						{#if !sidebarCollapsed}
							<span>{item.label}</span>
						{/if}
					</button>

					{#if item.children && !sidebarCollapsed}
						<div class="ml-4 border-l border-sidebar-border py-1 pl-3">
							{#each item.children as child}
								<button
									class={[
										"flex h-8 w-full items-center rounded-sm px-2 text-left text-xs transition-colors",
										child.active ? "text-foreground" : "text-muted-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
									]}
									disabled={!child.active}
									title={child.active ? child.label : `${child.label} is not available yet`}
								>
									{child.label}
								</button>
							{/each}
						</div>
					{/if}
				{/each}
			</nav>
		</aside>

		<section class="min-w-0 h-screen overflow-y-auto flex-1 scrollbar-hidden pt-9">
			<div class="border-b border-border bg-card px-4 py-3 lg:hidden">
				<div class="flex items-center gap-2 text-sm font-semibold">
					<DatabaseIcon class="size-4 text-muted-foreground" />
					MariaDB
				</div>
			</div>

			<div class="mx-auto max-w-7xl px-4 py-6 sm:px-6 lg:px-8">
				<MariaDBPanel />
			</div>
		</section>
	</div>
</main>
