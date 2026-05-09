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
	let showExpandedContent = $state(false);
	let openSections = $state<Set<string>>(new Set());
	let expandTimer: ReturnType<typeof setTimeout> | null = null;

	$effect(() => {
		if (expandTimer) {
			clearTimeout(expandTimer);
			expandTimer = null;
		}

		if (collapsed) {
			showExpandedContent = false;
			openSections = new Set();
			return;
		}

		expandTimer = setTimeout(() => {
			showExpandedContent = true;
		}, 210);
	});

	function isSectionActive(item: (typeof navigation)[number]) {
		return item.id === activePage || item.children?.some((child) => child.id === activePage);
	}

	function toggleSection(sectionId: string) {
		const next = new Set(openSections);

		if (next.has(sectionId)) {
			next.delete(sectionId);
		} else {
			next.add(sectionId);
		}

		openSections = next;
	}
</script>

<aside
	class={[
		"hidden shrink-0 border-r border-sidebar-border bg-sidebar text-sidebar-foreground transition-[width] duration-200 lg:block",
		collapsed ? "w-16 px-2" : "w-64 px-4",
	]}
>
	<div class="flex h-[calc(100vh-2.25rem)] flex-col py-5">
		<div class={["mb-6 min-h-12", collapsed ? "px-0" : "px-2"]}>
			{#if showExpandedContent}
				<div class="min-w-0">
					<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">FXServer Installer</p>
					<h2 class="mt-2 text-lg font-semibold tracking-normal">Setup Workspace</h2>
				</div>
			{/if}
		</div>

	<nav class="flex-1 space-y-1" aria-label="Workspace navigation">
		{#each navigation as item}
			{@const Icon = item.icon}
			{@const parentIsDirectPage = !item.children}
			{@const sectionActive = isSectionActive(item)}
			{@const sectionOpen = openSections.has(item.id)}
			<button
				class={[
					"flex h-10 w-full items-center gap-3 rounded-sm px-3 text-left text-sm font-medium transition-colors",
					collapsed && "justify-center px-0",
					sectionActive
						? "bg-sidebar-primary text-sidebar-primary-foreground shadow-sm"
						: "text-muted-foreground hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
				]}
				onclick={() => (parentIsDirectPage ? onNavigate(item.id as PageId) : toggleSection(item.id))}
				aria-current={item.id === activePage ? "page" : undefined}
				aria-expanded={item.children ? sectionOpen : undefined}
				title={item.label}
			>
				<Icon class="size-4" />
				{#if showExpandedContent}
					<span>{item.label}</span>
				{/if}
			</button>

			{#if item.children && showExpandedContent && sectionOpen}
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

		<div class="mt-4 border-t border-sidebar-border pt-3">
			<button
				class={[
					"flex h-10 w-full items-center gap-3 rounded-sm px-3 text-sm font-medium text-muted-foreground transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground",
					collapsed && "justify-center px-0",
				]}
				onclick={onToggle}
				title={collapsed ? "Expand sidebar" : "Collapse sidebar"}
				aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
			>
				{#if collapsed}
					<PanelLeftOpenIcon class="size-4" />
				{:else}
					<PanelLeftCloseIcon class="size-4" />
				{/if}
				{#if showExpandedContent}
					<span>Collapse</span>
				{/if}
			</button>
		</div>
	</div>
</aside>
