<script lang="ts">
	import PanelLeftCloseIcon from "@lucide/svelte/icons/panel-left-close";
	import PanelLeftOpenIcon from "@lucide/svelte/icons/panel-left-open";
	import type { PageId } from "$lib/navigation";
	import { navigation } from "$lib/navigation";

	type Props = {
		activePage: PageId;
		collapsed: boolean;
		onNavigate: (page: PageId) => void;
		onToggle: () => void;
	};

	let { activePage, collapsed, onNavigate, onToggle }: Props = $props();

	function isSectionActive(item: (typeof navigation)[number]) {
		return item.id === activePage || item.children?.some((child) => child.id === activePage);
	}
</script>

<aside
	class={[
		"hidden shrink-0 border-r border-sidebar-border bg-sidebar py-5 text-sidebar-foreground transition-[width] duration-200 lg:block",
		collapsed ? "w-16 px-2" : "w-64 px-4",
	]}
>
	<div class={["mb-6 flex items-start justify-between gap-2", collapsed ? "px-0" : "px-2"]}>
		{#if !collapsed}
			<div class="min-w-0">
				<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">FXServer Installer</p>
				<h2 class="mt-2 text-lg font-semibold tracking-normal">Setup Workspace</h2>
			</div>
		{/if}
		<button
			class="flex size-9 shrink-0 items-center justify-center rounded-sm text-muted-foreground transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
			onclick={onToggle}
			title={collapsed ? "Expand sidebar" : "Collapse sidebar"}
			aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
		>
			{#if collapsed}
				<PanelLeftOpenIcon class="size-4" />
			{:else}
				<PanelLeftCloseIcon class="size-4" />
			{/if}
		</button>
	</div>

	<nav class="space-y-1" aria-label="Workspace navigation">
		{#each navigation as item}
			{@const Icon = item.icon}
			{@const parentIsDirectPage = !item.children}
			{@const sectionActive = isSectionActive(item)}
			<button
				class={[
					"flex h-10 w-full items-center gap-3 rounded-sm px-3 text-left text-sm font-medium transition-colors",
					collapsed && "justify-center px-0",
					sectionActive
						? "bg-sidebar-primary text-sidebar-primary-foreground shadow-sm"
						: "text-muted-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
				]}
				disabled={!parentIsDirectPage}
				onclick={() => parentIsDirectPage && onNavigate(item.id as PageId)}
				aria-current={item.id === activePage ? "page" : undefined}
				title={item.label}
			>
				<Icon class="size-4" />
				{#if !collapsed}
					<span>{item.label}</span>
				{/if}
			</button>

			{#if item.children && !collapsed}
				<div class="ml-4 border-l border-sidebar-border py-1 pl-3">
					{#each item.children as child}
						{@const ChildIcon = child.icon}
						<button
							class={[
								"flex h-8 w-full items-center gap-2 rounded-sm px-2 text-left text-xs transition-colors",
								activePage === child.id
									? "bg-sidebar-accent text-sidebar-accent-foreground"
									: "text-muted-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
							]}
							onclick={() => onNavigate(child.id)}
							aria-current={activePage === child.id ? "page" : undefined}
							title={child.label}
						>
							{#if ChildIcon}
								<ChildIcon class="size-3.5" />
							{/if}
							{child.label}
						</button>
					{/each}
				</div>
			{/if}
		{/each}
	</nav>
</aside>
