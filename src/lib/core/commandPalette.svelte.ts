import type { PageId } from "$lib/navigation";

export type CommandCategory = "Navigate" | "Database" | "FXServer" | "Tools" | "Logs" | "Setup";

export type CommandDefinition = {
	id: string;
	title: string;
	description: string;
	category: CommandCategory;
	page: PageId;
	keywords: string[];
};

export type CommandPaletteSettings = {
	enabled: boolean;
	disabledCommandIds: string[];
	shortcuts: Record<string, string>;
};

const storageKey = "fxserver-installer.command-palette";

export const commandPaletteSettings = $state<CommandPaletteSettings>({
	enabled: true,
	disabledCommandIds: [],
	shortcuts: {},
});

export const commandDefinitions: CommandDefinition[] = [
	{ id: "open-workspaces", title: "Open Workspaces", description: "Switch saved server setups.", category: "Setup", page: "workspaces", keywords: ["profile", "staging", "production"] },
	{ id: "open-tasks", title: "Open Task Center", description: "View background operations and session history.", category: "Tools", page: "tasks", keywords: ["progress", "jobs"] },
	{ id: "open-backup-manager", title: "Open Backups & Restore", description: "Schedule database backups and preview restores.", category: "Database", page: "backup-manager", keywords: ["schedule", "retention", "restore"] },
	{ id: "open-diagnostics", title: "Open Diagnostics", description: "Check server readiness and export a redacted report.", category: "FXServer", page: "diagnostics", keywords: ["preflight", "dependencies", "support"] },
	{ id: "open-health", title: "Open Health & Recovery", description: "Configure health alerts and bounded crash recovery.", category: "FXServer", page: "health", keywords: ["cpu", "memory", "disk", "restart"] },
	{ id: "open-home", title: "Open Home", description: "Go to the workspace overview.", category: "Navigate", page: "home", keywords: ["dashboard", "overview"] },
	{ id: "open-onboarding", title: "Open First Run Wizard", description: "Walk through first-time setup checks.", category: "Setup", page: "onboarding", keywords: ["wizard", "setup", "first run"] },
	{ id: "open-mariadb", title: "Manage MariaDB", description: "Install, manage, and inspect MariaDB.", category: "Database", page: "mariadb", keywords: ["database", "mysql", "users"] },
	{ id: "open-sql-runner", title: "Open Queries & Files", description: "Run .sql files, create backups, and use the query console.", category: "Database", page: "sql-runner", keywords: ["import", "migration", "sql", "backup"] },
	{ id: "open-artifact-install", title: "Install FXServer Artifact", description: "Download and extract a recommended artifact.", category: "FXServer", page: "artifact-install", keywords: ["artifact", "download"] },
	{ id: "open-artifact-info", title: "Open Artifact Info", description: "Inspect FXServer artifact metadata.", category: "FXServer", page: "artifact-info", keywords: ["artifact", "version"] },
	{ id: "open-manage-server", title: "Manage FXServer", description: "Start, stop, monitor, and use RCON.", category: "FXServer", page: "server-manage", keywords: ["start", "console", "rcon"] },
	{ id: "open-resource-manager", title: "Open Resource Manager", description: "Start, stop, restart, and ensure resources over RCON.", category: "FXServer", page: "resource-manager", keywords: ["resource", "ensure", "restart"] },
	{ id: "open-configure-server", title: "Configure Server", description: "Edit cfg files and helper values.", category: "FXServer", page: "server-configure", keywords: ["server.cfg", "permissions", "config"] },
	{ id: "open-server-logs", title: "Open Server Logs", description: "Inspect txData FXServer logs.", category: "Logs", page: "server-logs", keywords: ["txadmin", "fxserver.log"] },
	{ id: "open-configurator", title: "Open Configurator", description: "Edit supported Lua config values.", category: "Tools", page: "configurator", keywords: ["lua"] },
	{ id: "open-profiler", title: "Open Profiler", description: "Analyze profiler captures.", category: "Tools", page: "profiler", keywords: ["performance"] },
	{ id: "open-jooat", title: "Open JOOAT Resolver", description: "Hash or resolve JOOAT values.", category: "Tools", page: "jooat", keywords: ["hash"] },
	{ id: "open-json", title: "Open JSON Formatter", description: "Format and validate JSON.", category: "Tools", page: "json-formatter", keywords: ["json"] },
	{ id: "open-app-logs", title: "Open Application Logs", description: "Inspect app logs.", category: "Logs", page: "logs", keywords: ["application"] },
	{ id: "open-client-logs", title: "Open Client Logs", description: "Inspect FiveM client logs.", category: "Logs", page: "client-logs", keywords: ["fivem"] },
	{ id: "open-command-settings", title: "Configure Command Palette", description: "Choose which commands appear in Ctrl+K.", category: "Tools", page: "command-palette", keywords: ["settings", "shortcuts"] },
];

export function loadCommandPaletteSettings() {
	try {
		const saved = JSON.parse(localStorage.getItem(storageKey) || "{}") as Partial<CommandPaletteSettings>;
		commandPaletteSettings.enabled = saved.enabled ?? true;
		commandPaletteSettings.disabledCommandIds = Array.isArray(saved.disabledCommandIds) ? saved.disabledCommandIds : [];
		commandPaletteSettings.shortcuts = saved.shortcuts && typeof saved.shortcuts === "object" ? saved.shortcuts : {};
	} catch {
		commandPaletteSettings.enabled = true;
		commandPaletteSettings.disabledCommandIds = [];
		commandPaletteSettings.shortcuts = {};
	}
}

export function saveCommandPaletteSettings() {
	localStorage.setItem(storageKey, JSON.stringify(commandPaletteSettings));
}

export function commandEnabled(id: string) {
	return !commandPaletteSettings.disabledCommandIds.includes(id);
}

export function setCommandEnabled(id: string, enabled: boolean) {
	const next = new Set(commandPaletteSettings.disabledCommandIds);
	if (enabled) {
		next.delete(id);
	} else {
		next.add(id);
	}
	commandPaletteSettings.disabledCommandIds = [...next];
	saveCommandPaletteSettings();
}

export function getCommandShortcut(id: string) {
	return commandPaletteSettings.shortcuts[id] ?? "";
}

export function setCommandShortcut(id: string, shortcut: string) {
	const next = { ...commandPaletteSettings.shortcuts };
	if (shortcut) {
		next[id] = shortcut;
	} else {
		delete next[id];
	}
	commandPaletteSettings.shortcuts = next;
	saveCommandPaletteSettings();
}

export function shortcutFromKeyboardEvent(event: KeyboardEvent) {
	const key = normalizedKey(event);
	if (!key) return "";

	const parts = [];
	if (event.ctrlKey) parts.push("Ctrl");
	if (event.altKey) parts.push("Alt");
	if (event.shiftKey) parts.push("Shift");
	if (event.metaKey) parts.push("Meta");
	parts.push(key);
	return parts.join("+");
}

export function shortcutMatchesEvent(shortcut: string, event: KeyboardEvent) {
	return shortcutFromKeyboardEvent(event).toLowerCase() === shortcut.toLowerCase();
}

function normalizedKey(event: KeyboardEvent) {
	if (["Control", "Alt", "Shift", "Meta"].includes(event.key)) return "";
	if (event.key === " ") return "Space";
	if (event.key.length === 1) return event.key.toUpperCase();
	return event.key;
}
