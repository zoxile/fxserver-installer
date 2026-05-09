<script lang="ts">
	import { onMount } from "svelte";
	import PanelLeftCloseIcon from "@lucide/svelte/icons/panel-left-close";
	import PanelLeftOpenIcon from "@lucide/svelte/icons/panel-left-open";
	import SidebarNavButton from "./SidebarNavButton.svelte";
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
	let isNarrow = $state(false);
	let expandTimer: ReturnType<typeof setTimeout> | null = null;
	let effectiveCollapsed = $derived(collapsed || isNarrow);
	let activeParentId = $derived(
		navigation.find((item) => item.children?.some((child) => child.id === activePage))?.id,
	);

	onMount(() => {
		const media = window.matchMedia("(max-width: 1023px)");
		const update = () => (isNarrow = media.matches);

		update();
		media.addEventListener("change", update);

		return () => media.removeEventListener("change", update);
	});

	$effect(() => {
		if (expandTimer) {
			clearTimeout(expandTimer);
			expandTimer = null;
		}

		if (effectiveCollapsed) {
			showExpandedContent = false;
			openSections = new Set();
			return;
		}

		expandTimer = setTimeout(() => {
			showExpandedContent = true;
		}, 280);

		return () => {
			if (expandTimer) {
				clearTimeout(expandTimer);
				expandTimer = null;
			}
		};
	});

	$effect(() => {
		if (!activeParentId || effectiveCollapsed) return;
		if (openSections.has(activeParentId)) return;

		const next = new Set(openSections);
		next.add(activeParentId);
		openSections = next;
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
		"relative block shrink-0 border-r border-sidebar-border bg-sidebar/85 pt-9 text-sidebar-foreground backdrop-blur-xl transition-[width] duration-300 ease-out will-change-[width]",
		effectiveCollapsed ? "w-16 px-2" : "w-64 px-4",
	]}
>
	{#if !isNarrow}
		<button
			class="absolute top-1/2 -right-3 z-10 flex h-12 w-6 -translate-y-1/2 items-center justify-center rounded-r-md border-0 bg-sidebar text-muted-foreground shadow-none outline-none ring-0 transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground focus-visible:ring-1 focus-visible:ring-ring"
			onclick={onToggle}
			title={effectiveCollapsed ? "Expand sidebar" : "Collapse sidebar"}
			aria-label={effectiveCollapsed ? "Expand sidebar" : "Collapse sidebar"}
		>
			{#if effectiveCollapsed}
				<PanelLeftOpenIcon class="size-3.5" />
			{:else}
				<PanelLeftCloseIcon class="size-3.5" />
			{/if}
		</button>
	{/if}

	<div class="flex h-[calc(100vh-2.25rem)] flex-col overflow-hidden py-5">
		<div class={["relative mb-6 h-14 shrink-0 overflow-hidden", effectiveCollapsed ? "px-0" : "px-2"]}>
			<div
				class={[
					"absolute inset-x-2 top-0 min-w-0 transition-[opacity,transform] duration-200 ease-out",
					showExpandedContent ? "translate-y-0 opacity-100" : "-translate-y-1 opacity-0",
				]}
			>
				<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">FXServer Installer</p>
				<h2 class="mt-1 text-lg font-semibold leading-7 tracking-normal">Setup Workspace</h2>
			</div>
		</div>

		<nav class="flex-1 space-y-1" aria-label="Workspace navigation">
			{#each navigation as item}
				{@const parentIsDirectPage = !item.children}
				{@const sectionActive = isSectionActive(item)}
				{@const sectionOpen = openSections.has(item.id) || activeParentId === item.id}
				<SidebarNavButton
					icon={item.icon}
					label={item.label}
					active={sectionActive}
					collapsed={effectiveCollapsed}
					labelVisible={showExpandedContent}
					expanded={item.children ? sectionOpen : undefined}
					onclick={() => (parentIsDirectPage ? onNavigate(item.id as PageId) : toggleSection(item.id))}
				/>

				{#if item.children && showExpandedContent}
					<div
						class={[
							"grid will-change-[grid-template-rows,opacity] transition-[grid-template-rows,opacity] duration-200 ease-out",
							sectionOpen ? "grid-rows-[1fr] opacity-100" : "grid-rows-[0fr] opacity-0",
						]}
					>
						<div class="overflow-hidden">
							<div class="ml-4 border-l border-sidebar-border py-1 pl-3">
								{#each item.children as child}
									{#if child.icon}
										<SidebarNavButton
											icon={child.icon}
											label={child.label}
											size="child"
											active={activePage === child.id}
											collapsed={false}
											labelVisible={true}
											onclick={() => onNavigate(child.id)}
										/>
									{/if}
								{/each}
							</div>
						</div>
					</div>
				{/if}
			{/each}
		</nav>
	</div>
</aside>
