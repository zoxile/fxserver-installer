import ArchiveIcon from "@lucide/svelte/icons/archive";
import ActivityIcon from "@lucide/svelte/icons/activity";
import ClipboardCheckIcon from "@lucide/svelte/icons/clipboard-check";
import FolderKanbanIcon from "@lucide/svelte/icons/folder-kanban";
import DatabaseIcon from "@lucide/svelte/icons/database";
import FileJsonIcon from "@lucide/svelte/icons/file-json";
import FolderCogIcon from "@lucide/svelte/icons/folder-cog";
import HashIcon from "@lucide/svelte/icons/hash";
import HomeIcon from "@lucide/svelte/icons/home";
import InfoIcon from "@lucide/svelte/icons/info";
import LogsIcon from "@lucide/svelte/icons/logs";
import PackagePlusIcon from "@lucide/svelte/icons/package-plus";
import RocketIcon from "@lucide/svelte/icons/rocket";
import ScrollTextIcon from "@lucide/svelte/icons/scroll-text";
import SearchIcon from "@lucide/svelte/icons/search";
import ServerCogIcon from "@lucide/svelte/icons/server-cog";
import ShieldIcon from "@lucide/svelte/icons/shield";
import TablePropertiesIcon from "@lucide/svelte/icons/table-properties";
import TerminalIcon from "@lucide/svelte/icons/terminal";
import PlugIcon from "@lucide/svelte/icons/plug";
import WrenchIcon from "@lucide/svelte/icons/wrench";
import type { Component } from "svelte";

export type PageId =
	| "home"
	| "workspaces"
	| "tasks"
	| "backup-manager"
	| "diagnostics"
	| "health"
	| "live-bridge"
	| "database-browser"
	| "incidents"
	| "onboarding"
	| "mariadb"
	| "sql-runner"
	| "artifact-install"
	| "artifact-info"
	| "server-manage"
	| "resource-manager"
	| "server-configure"
	| "server-logs"
	| "command-palette"
	| "configurator"
	| "profiler"
	| "jooat"
	| "json-formatter"
	| "logs"
	| "client-logs";

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
		id: "onboarding",
		label: "First Run Wizard",
		icon: RocketIcon,
	},
	{ id: "workspaces", label: "Workspaces", icon: FolderKanbanIcon },
	{
		id: "database",
		label: "MariaDB",
		icon: DatabaseIcon,
		children: [
			{
				id: "mariadb",
				label: "Manage MariaDB",
				icon: DatabaseIcon,
			},
			{
				id: "sql-runner",
				label: "Queries & Files",
				icon: TablePropertiesIcon,
			},
			{ id: "backup-manager", label: "Backups & Restore", icon: ArchiveIcon },
			{ id: "database-browser", label: "Database Browser", icon: TablePropertiesIcon },
		],
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
				id: "resource-manager",
				label: "Resource Manager",
				icon: ShieldIcon,
			},
			{
				id: "server-configure",
				label: "Configure Server",
				icon: FolderCogIcon,
			},
			{ id: "diagnostics", label: "Diagnostics", icon: ClipboardCheckIcon },
			{ id: "health", label: "Health & Recovery", icon: ActivityIcon },
			{ id: "live-bridge", label: "Live Bridge", icon: PlugIcon },
		],
	},
	{
		id: "log-viewers",
		label: "Logs",
		icon: LogsIcon,
		children: [
			{ id: "incidents", label: "Incident Timeline", icon: ActivityIcon },
			{
				id: "server-logs",
				label: "Server Logs",
				icon: ScrollTextIcon,
			},
			{
				id: "logs",
				label: "Application Logs",
				icon: ScrollTextIcon,
			},
			{
				id: "client-logs",
				label: "Client Logs",
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
				id: "command-palette",
				label: "Command Palette",
				icon: SearchIcon,
			},
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
		],
	},
];

export function getPageLabel(pageId: PageId) {
	if (pageId === "tasks") return "Task Center";
	for (const item of navigation) {
		if (item.id === pageId) return item.label;
		const child = item.children?.find((entry) => entry.id === pageId);
		if (child) return child.label;
	}

	return "Workspace";
}
