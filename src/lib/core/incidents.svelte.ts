import { appendBoundedIncidents, normalizeIncident, INCIDENT_STORAGE_KEY, parseIncidents, type Incident, type IncidentInput, type IncidentPanel, type IncidentType } from "./incidentModel";

export type { Incident, IncidentInput } from "./incidentModel";
export const incidentSession = $state({ items: [] as Incident[], persistenceError: "" });
let initialized = false;
let pendingSave: ReturnType<typeof setTimeout> | undefined;
let pendingEvents: Incident[] = [];
let pendingBatch = false;

export function initializeIncidents() {
	if (initialized || typeof localStorage === "undefined") return;
	initialized = true;
	try { incidentSession.items = parseIncidents(localStorage.getItem(INCIDENT_STORAGE_KEY)); persist(); }
	catch { incidentSession.persistenceError = "Incident history is unavailable. New events remain in memory."; }
	if (typeof window !== "undefined") window.addEventListener("beforeunload", flushIncidents);
}

export function flushIncidents() {
	if (pendingSave) clearTimeout(pendingSave);
	pendingSave = undefined;
	flushBatch();
	persist();
}

function flushBatch() {
	pendingBatch = false;
	if (!pendingEvents.length) return;
	incidentSession.items = appendBoundedIncidents(incidentSession.items, pendingEvents);
	pendingEvents = [];
}

function persist() {
	try {
		localStorage.setItem(INCIDENT_STORAGE_KEY, JSON.stringify(incidentSession.items));
		incidentSession.persistenceError = "";
	} catch { incidentSession.persistenceError = "Incident history could not be saved. New events remain in memory."; }
}

export function appendIncident(input: IncidentInput) {
	appendIncidents([input]);
}

export function appendIncidents(inputs: IncidentInput[]) {
	initializeIncidents();
	for (const input of inputs.slice(-100)) pendingEvents.push(normalizeIncident(input));
	if (pendingEvents.length > 100) pendingEvents.splice(0, pendingEvents.length - 100);
	if (!pendingBatch) { pendingBatch = true; queueMicrotask(flushBatch); }
	if (!pendingSave) pendingSave = setTimeout(flushIncidents, 1000);
}

export function clearIncidents(workspaceId?: string) {
	initializeIncidents();
	flushBatch();
	incidentSession.items = workspaceId ? incidentSession.items.filter((item) => item.workspaceId !== workspaceId) : [];
	flushIncidents();
}

export function appendTaskIncident(task: { id: string; command: string; label: string; workspaceId: string; status: string; finishedAt?: number }) {
	if (task.status === "running") return;
	if (task.status === "completed" && /^(get_|read_|list_|preview_|validate_|run_fxserver_preflight)/.test(task.command)) return;
	let type: IncidentType = "log";
	let panel: IncidentPanel = "logs";
	if (/resource/.test(task.command)) { type = "resource"; panel = "resource-manager"; }
	else if (/server_config|config_history|diagnostic_config_patch/.test(task.command)) { type = "config"; panel = "server-configure"; }
	else if (/^(start|stop|restart)_fxserver/.test(task.command)) { type = "restart"; panel = "server-manage"; }
	else if (/clone|workspace/.test(task.command)) { type = "workspace"; panel = "workspaces"; }
	else if (/backup|restore|mariadb/.test(task.command)) { type = "database"; panel = "backup-manager"; }
	appendIncident({ id: `task:${task.id}`, timestamp: task.finishedAt, workspaceId: task.workspaceId, type, panel,
		level: task.status === "failed" ? "error" : task.status === "cancelled" ? "warn" : "success", title: `${task.label}: ${task.status}` });
}

export function appendHealthIncident(event: { id: number; timestamp: number; workspaceId: string; level: "info" | "warn" | "error"; kind: string; message: string }) {
	appendIncident({ id: `health:${event.timestamp}:${event.id}`, timestamp: event.timestamp < 1e12 ? event.timestamp * 1000 : event.timestamp,
		workspaceId: event.workspaceId, level: event.level, type: "health", panel: "health", title: event.message, detail: event.kind });
}
