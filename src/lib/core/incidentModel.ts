export const INCIDENT_LIMIT = 1000;
export const INCIDENT_STORAGE_KEY = "fxserver-installer.incidents.v1";
export const incidentTypes = ["log", "resource", "config", "restart", "health", "workspace", "database"] as const;
export type IncidentType = typeof incidentTypes[number];
export type IncidentLevel = "debug" | "info" | "success" | "warn" | "error";
export type IncidentPanel = "logs" | "resource-manager" | "server-configure" | "server-manage" | "health" | "workspaces" | "backup-manager";
const panels: IncidentPanel[] = ["logs", "resource-manager", "server-configure", "server-manage", "health", "workspaces", "backup-manager"];

export interface IncidentInput {
	id?: string;
	timestamp?: number | string;
	workspaceId?: string;
	type?: IncidentType;
	level?: IncidentLevel;
	title?: string;
	message?: string;
	detail?: string;
	panel?: IncidentPanel;
}

export interface Incident extends Required<Omit<IncidentInput, "timestamp" | "message">> { timestamp: number }

export function redactIncidentText(value: string, limit = 2000): string {
	return value
		.replace(/-----BEGIN [^-]*PRIVATE KEY-----[\s\S]*?(?:-----END [^-]*PRIVATE KEY-----|$)/gi, "[redacted private key]")
		.replace(/\b(?:mysql|mariadb|postgres(?:ql)?):\/\/[^\s"'<>]+/gi, "[redacted database URL]")
		.replace(/https?:\/\/[^\s"'<>]+/gi, (url) => {
			try { const parsed = new URL(url); return `${parsed.protocol}//${parsed.host}/[redacted URL]`; }
			catch { return "[redacted URL]"; }
		})
		.replace(/\b(?:cfxk_[\w-]+|gh[pousr]_[\w]+|github_pat_[\w]+|sk-[\w-]{12,}|eyJ[\w-]+\.[\w-]+\.[\w-]+)\b/g, "[redacted]")
		.replace(/(?:["']?[\w.-]*(?:password|passwd|pwd|secret|token|license.?key|api.?key|connection.?string|authorization|cookie|webhook|credential)[\w.-]*["']?\s*(?::|=|\s)\s*)(?:"[^"\n]*"|'[^'\n]*'|[^\r\n;,}]+)/gi, "[redacted credential]")
		.replace(/\bBearer\s+[^\s"',;]+/gi, "Bearer [redacted]")
		.replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f]/g, "")
		.slice(0, limit);
}

function safeId(value: unknown, fallback: string): string {
	return typeof value === "string" && /^[a-zA-Z0-9_.:-]{1,100}$/.test(value) && !/cfxk_|gh[pousr]_|secret|password|token/i.test(value) ? value : fallback;
}

export function normalizeIncident(input: IncidentInput, now = Date.now()): Incident {
	const parsedTime = typeof input.timestamp === "string" ? Date.parse(input.timestamp) : input.timestamp;
	const timestamp = typeof parsedTime === "number" && Number.isFinite(parsedTime) && parsedTime > 0 && parsedTime <= now + 60_000 ? parsedTime : now;
	return {
		id: safeId(input.id, crypto.randomUUID()),
		timestamp,
		workspaceId: safeId(input.workspaceId, "default"),
		type: incidentTypes.includes(input.type!) ? input.type! : "log",
		level: ["debug", "info", "success", "warn", "error"].includes(input.level!) ? input.level! : "info",
		title: redactIncidentText(typeof input.title === "string" ? input.title : typeof input.message === "string" ? input.message : "Application event", 240),
		detail: redactIncidentText(typeof input.detail === "string" ? input.detail : ""),
		panel: panels.includes(input.panel!) ? input.panel! : "logs",
	};
}

export function appendBoundedIncident(items: Incident[], input: IncidentInput, now = Date.now()): Incident[] {
	return appendBoundedIncidents(items, [input], now);
}

export function appendBoundedIncidents(items: Incident[], inputs: IncidentInput[], now = Date.now()): Incident[] {
	const seen = new Set(items.map((item) => `${item.workspaceId}:${item.id}`));
	const added: Incident[] = [];
	for (const input of inputs.slice(-100)) {
		const event = normalizeIncident(input, now);
		const key = `${event.workspaceId}:${event.id}`;
		if (!seen.has(key)) { seen.add(key); added.push(event); }
	}
	if (!added.length) return items;
	return [...added, ...items].sort((a, b) => b.timestamp - a.timestamp).slice(0, INCIDENT_LIMIT);
}

export function parseIncidents(raw: string | null): Incident[] {
	try {
		if (!raw || raw.length > 6_000_000) return [];
		const value: unknown = JSON.parse(raw);
		if (!Array.isArray(value)) return [];
		let items: Incident[] = [];
		const candidates = value.slice(0, INCIDENT_LIMIT).filter((item) => item && typeof item === "object" && !Array.isArray(item));
		for (let i = 0; i < candidates.length; i += 100) items = appendBoundedIncidents(items, candidates.slice(i, i + 100));
		return items;
	} catch { return []; }
}

export interface IncidentFilter { workspaceId?: string; type?: string; after?: number; before?: number; search?: string }
export function filterIncidents(items: Incident[], filter: IncidentFilter): Incident[] {
	const query = filter.search?.trim().toLowerCase() ?? "";
	return items.filter((item) => (!filter.workspaceId || item.workspaceId === filter.workspaceId)
		&& (!filter.type || item.type === filter.type)
		&& (!filter.after || item.timestamp >= filter.after)
		&& (!filter.before || item.timestamp <= filter.before)
		&& (!query || `${item.title} ${item.detail}`.toLowerCase().includes(query)));
}
