import ArchiveIcon from "@lucide/svelte/icons/archive";
import DatabaseIcon from "@lucide/svelte/icons/database";
import FileJsonIcon from "@lucide/svelte/icons/file-json";
import FolderCogIcon from "@lucide/svelte/icons/folder-cog";
import HashIcon from "@lucide/svelte/icons/hash";
import HomeIcon from "@lucide/svelte/icons/home";
import InfoIcon from "@lucide/svelte/icons/info";
import PackagePlusIcon from "@lucide/svelte/icons/package-plus";
import ScrollTextIcon from "@lucide/svelte/icons/scroll-text";
import ServerCogIcon from "@lucide/svelte/icons/server-cog";
import TerminalIcon from "@lucide/svelte/icons/terminal";
import WrenchIcon from "@lucide/svelte/icons/wrench";
import type { Component } from "svelte";

export type PageId =
	| "home"
	| "mariadb"
	| "artifact-install"
	| "artifact-info"
	| "server-manage"
	| "server-logs"
	| "configurator"
	| "profiler"
	| "jooat"
	| "json-formatter"
	| "logs";

export interface NavigationChild {
	id: PageId;
	label: string;
	icon?: Component;
}

export interface NavigationItem {
	id: PageId | string;
	label: string;
	icon: Component;
	children?: NavigationChild[];
}

export const navigation: NavigationItem[] = [
	{
		id: "home",
		label: "Home",
		icon: HomeIcon,
	},
	{
		id: "mariadb",
		label: "MariaDB",
		icon: DatabaseIcon,
	},
	{
		id: "artifacts",
		label: "Artifacts",
		icon: ArchiveIcon,
		children: [
			{
				id: "artifact-install",
				label: "Install Artifact",
				icon: PackagePlusIcon,
			},
			{
				id: "artifact-info",
				label: "Artifact Information",
				icon: InfoIcon,
			},
		],
	},
	{
		id: "fxserver",
		label: "FXServer",
		icon: ServerCogIcon,
		children: [
			{
				id: "server-manage",
				label: "Manage Server",
				icon: TerminalIcon,
			},
			{
				id: "server-logs",
				label: "Server Logs",
				icon: ScrollTextIcon,
			},
		],
	},
	{
		id: "tools",
		label: "Tools & Utils",
		icon: FolderCogIcon,
		children: [
			{
				id: "configurator",
				label: "Configurator",
				icon: WrenchIcon,
			},
			{
				id: "profiler",
				label: "Profiler",
				icon: InfoIcon,
			},
			{
				id: "jooat",
				label: "JOOAT Resolver & Hasher",
				icon: HashIcon,
			},
			{
				id: "json-formatter",
				label: "JSON Formatter",
				icon: FileJsonIcon,
			},
			{
				id: "logs",
				label: "Application Logs",
				icon: ScrollTextIcon,
			},
		],
	},
];

export function getPageLabel(pageId: PageId) {
	for (const item of navigation) {
		if (item.id === pageId) return item.label;
		const child = item.children?.find((entry) => entry.id === pageId);
		if (child) return child.label;
	}

	return "Workspace";
}
