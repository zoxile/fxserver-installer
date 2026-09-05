import { invoke } from "@tauri-apps/api/core";
import { databaseSession, rememberDatabaseCredentials } from "./databaseSession.svelte";
import type { MariaDBCredentials } from "$lib/modules/mariadb";
import { getInstallPath, setInstallPath } from "./paths.svelte";
import { hasRunningTasks, taskSession, trackTask } from "./tasks.svelte";
import { fxserverSettings, loadFxserverSettings, readSavedEnvironment, setServerProfile, setTxDataPath, writeSavedEnvironment } from "$lib/features/fxserver/fxserverSettings.svelte";
import { emptyWorkspace, parseWorkspaces, publicEnvironment, type Workspace } from "./workspaceSettings";

const storageKey = "fxserver-installer.workspaces.v1";
const databaseCredentials = new Map<string, MariaDBCredentials>();
let loaded = false;
let applying = false;

export const workspaceSession = $state({ activeId: "default", items: [] as Workspace[], revision: 0 });

export function getWorkspaceId() {
	return workspaceSession.activeId;
}

export function initializeWorkspaces() {
	if (loaded) return;
	loadFxserverSettings();
	const saved = parseWorkspaces(localStorage.getItem(storageKey));
	loaded = true;
	if (saved) {
		workspaceSession.items = saved.items;
		workspaceSession.activeId = saved.activeId;
		applySettings(saved.items.find((item) => item.id === saved.activeId)!);
	} else {
		workspaceSession.items = [emptyWorkspace("default", "Default")];
		captureActiveWorkspace();
	}
	taskSession.workspaceId = workspaceSession.activeId;
	if ("__TAURI_INTERNALS__" in window) {
		void invoke("initialize_health_workspace", { workspaceId: workspaceSession.activeId }).catch((error) => console.error("Could not initialize the health workspace.", error));
	}
	window.addEventListener("workspace-settings-changed", captureActiveWorkspace);
	window.addEventListener("beforeunload", captureActiveWorkspace);
}

function persist() {
	localStorage.setItem(storageKey, JSON.stringify({ activeId: workspaceSession.activeId, items: workspaceSession.items }));
}

export function captureActiveWorkspace() {
	if (!loaded || applying) return;
	const current = workspaceSession.items.find((item) => item.id === workspaceSession.activeId);
	if (!current) return;
	current.artifactPath = getInstallPath();
	current.txDataPath = fxserverSettings.txDataPath;
	current.profile = fxserverSettings.profile;
	current.environment = publicEnvironment(readSavedEnvironment());
	current.database = { ...databaseSession.defaults };
	persist();
}

function applySettings(workspace: Workspace) {
	applying = true;
	try {
		setInstallPath(workspace.artifactPath);
		writeSavedEnvironment(publicEnvironment(workspace.environment));
		setTxDataPath(workspace.txDataPath);
		setServerProfile(workspace.profile);
		fxserverSettings.profiles = [];
		fxserverSettings.hasRootLogs = false;
		fxserverSettings.profileError = "";
		databaseSession.defaults = { ...workspace.database };
		databaseSession.credentials = null;
		databaseSession.connectionString = "";
		const credentials = databaseCredentials.get(workspace.id);
		if (credentials) rememberDatabaseCredentials(credentials);
	} finally {
		applying = false;
	}
}

export async function saveWorkspace(workspace: Workspace) {
	const name = workspace.name.trim();
	if (!name) throw new Error("Enter a workspace name.");
	if (workspaceSession.items.some((item) => item.id !== workspace.id && item.name.toLowerCase() === name.toLowerCase())) throw new Error("A workspace with that name already exists.");
	if (!Number.isInteger(workspace.database.port) || workspace.database.port < 1 || workspace.database.port > 65535) throw new Error("Enter a database port between 1 and 65535.");
	const saved = { ...workspace, name: name.slice(0, 80), environment: publicEnvironment(workspace.environment) };
	const index = workspaceSession.items.findIndex((item) => item.id === workspace.id);
	if (index < 0 && workspaceSession.items.length >= 50) throw new Error("Up to 50 workspaces can be saved.");
	const active = workspace.id === workspaceSession.activeId;
	if (active && (hasRunningTasks() || taskSession.switching)) throw new Error("Wait for background tasks to finish before editing the active workspace.");
	if (active) taskSession.switching = true;
	try {
		if (active && "__TAURI_INTERNALS__" in window) await invoke("prepare_workspace_switch", { workspaceId: workspace.id });
		if (index >= 0 && JSON.stringify(workspaceSession.items[index].database) !== JSON.stringify(saved.database)) databaseCredentials.delete(workspace.id);
		if (active && databaseSession.credentials && JSON.stringify(databaseSession.defaults) === JSON.stringify(saved.database)) {
			databaseCredentials.set(workspace.id, { ...databaseSession.credentials });
		} else if (active) databaseCredentials.delete(workspace.id);
		if (index >= 0) workspaceSession.items[index] = saved;
		else workspaceSession.items.push(saved);
		if (active) { applySettings(saved); workspaceSession.revision += 1; }
		persist();
	} finally {
		if (active) taskSession.switching = false;
	}
}

export async function removeWorkspace(id: string) {
	if (id === workspaceSession.activeId) throw new Error("Switch to another workspace before removing this one.");
	if (taskSession.switching || hasRunningTasks()) throw new Error("Wait for background tasks to finish before removing a workspace.");
	await trackTask("remove_workspace", "Remove workspace", async () => {
		if ("__TAURI_INTERNALS__" in window) {
			const overview = await invoke<{ busy: boolean; schedules: { config: { id: string } }[] }>("get_backup_manager", { workspaceId: id });
			if (overview.busy) throw new Error("Wait for the backup or restore to finish before removing a workspace.");
			for (const schedule of overview.schedules) {
				await invoke("remove_backup_schedule", { workspaceId: id, scheduleId: schedule.config.id });
			}
			await invoke("clear_fxserver_rcon_password", { workspaceId: id });
		}
		workspaceSession.items = workspaceSession.items.filter((item) => item.id !== id);
		databaseCredentials.delete(id);
		persist();
	});
}

export async function switchWorkspace(id: string) {
	if (id === workspaceSession.activeId) return;
	if (taskSession.switching || hasRunningTasks()) throw new Error("Wait for background tasks to finish before switching workspaces.");
	const target = workspaceSession.items.find((item) => item.id === id);
	if (!target) throw new Error("Workspace not found.");
	taskSession.switching = true;
	try {
		if ("__TAURI_INTERNALS__" in window) await invoke("prepare_workspace_switch", { workspaceId: id });
		captureActiveWorkspace();
		if (databaseSession.credentials) databaseCredentials.set(workspaceSession.activeId, { ...databaseSession.credentials });
		workspaceSession.activeId = id;
		taskSession.workspaceId = id;
		applySettings(target);
		persist();
		workspaceSession.revision += 1;
	} finally {
		taskSession.switching = false;
	}
}
