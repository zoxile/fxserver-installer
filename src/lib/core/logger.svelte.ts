import { invoke } from "@tauri-apps/api/core";
import { redactIncidentText } from "./incidentModel";

export type LogLevel = "debug" | "info" | "success" | "warn" | "error";

export interface AppLogEntry {
	id: string;
	timestamp: string;
	level: LogLevel;
	scope: string;
	message: string;
	detail?: string;
}

interface AppLogFile {
	path: string;
	entries: string[];
}

type LogOptions = {
	level?: LogLevel;
	scope?: string;
	detail?: string;
};

const browserLogKey = "fxserver-installer.logs";
const maxVisibleLogs = 700;
let initialized = false;
let persistQueue = Promise.resolve();

export const logs = $state<AppLogEntry[]>([]);
export const logFilePath = $state({ value: "Browser preview local storage" });

function hasTauriRuntime() {
	return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function createLogEntry(message: string, options: LogOptions = {}): AppLogEntry {
	const timestamp = new Date().toISOString();

	return {
		id: `${timestamp}-${Math.random().toString(16).slice(2)}`,
		timestamp,
		level: options.level ?? "info",
		scope: redactIncidentText(options.scope ?? "app", 120),
		message: redactIncidentText(message),
		detail: options.detail === undefined ? undefined : redactIncidentText(options.detail, 8000),
	};
}

function trimVisibleLogs() {
	if (logs.length > maxVisibleLogs) {
		logs.splice(0, logs.length - maxVisibleLogs);
	}
}

function persistEntry(entry: AppLogEntry) {
	const line = JSON.stringify(entry);

	if (!hasTauriRuntime()) {
		try {
			const history = readBrowserLogs().map(parseLogLine).filter((item): item is AppLogEntry => item !== null).map((item) => JSON.stringify(item));
			localStorage.setItem(browserLogKey, JSON.stringify([...history, line].slice(-maxVisibleLogs)));
		} catch { console.error("Could not persist app log entry."); }
		return;
	}

	persistQueue = persistQueue
		.then(() => invoke<void>("append_app_log", { entry: line }))
		.catch(() => {
			console.error("Could not persist app log entry.");
		});
}

function readBrowserLogs() {
	try {
		const parsed: unknown = JSON.parse(localStorage.getItem(browserLogKey) || "[]");
		return Array.isArray(parsed) ? parsed.slice(-maxVisibleLogs).filter((line): line is string => typeof line === "string") : [];
	} catch {
		return [];
	}
}

function parseLogLine(line: string, index = 0): AppLogEntry | null {
	try {
		const parsed = JSON.parse(line) as Partial<AppLogEntry>;
		if (!parsed || typeof parsed.scope !== "string" || typeof parsed.message !== "string" || !["debug", "info", "success", "warn", "error"].includes(parsed.level ?? "")) return null;
		const timestamp = new Date(parsed.timestamp ?? "");
		if (!Number.isFinite(timestamp.getTime())) return null;

		return {
			id: typeof parsed.id === "string" ? redactIncidentText(parsed.id, 240) : `${timestamp.toISOString()}-${index}`,
			timestamp: timestamp.toISOString(),
			level: parsed.level!,
			scope: redactIncidentText(parsed.scope, 120),
			message: redactIncidentText(parsed.message),
			detail: typeof parsed.detail === "string" ? redactIncidentText(parsed.detail, 8000) : undefined,
		};
	} catch {
		return null;
	}
}

export function acceptBackgroundLog(value: unknown) {
	const entry = parseLogLine(JSON.stringify(value));
	if (entry && !logs.some((item) => item.id === entry.id)) {
		logs.push(entry);
		trimVisibleLogs();
		window.dispatchEvent(new CustomEvent("app-log-entry", { detail: entry }));
	}
}

function normalizeLogEntries(entries: AppLogEntry[]) {
	const usedIds = new Set<string>();

	return entries.map((entry, index) => {
		const baseId = entry.id || `${entry.timestamp}-${entry.scope}-${index}`;
		let nextId = baseId;
		let suffix = 1;

		while (usedIds.has(nextId)) {
			nextId = `${baseId}-${suffix}`;
			suffix += 1;
		}

		usedIds.add(nextId);
		return nextId === entry.id ? entry : { ...entry, id: nextId };
	});
}

export function log(message: string, options: LogOptions = {}) {
	const entry = createLogEntry(message, options);
	logs.push(entry);
	trimVisibleLogs();
	persistEntry(entry);
	window.dispatchEvent(new CustomEvent("app-log-entry", { detail: entry }));

	const consoleMethod = entry.level === "error" ? "error" : entry.level === "warn" ? "warn" : "info";
	console[consoleMethod](`[${entry.scope}] ${entry.message}`, entry.detail ?? "");
}

export async function initializeLogger() {
	if (initialized) return;
	initialized = true;

	try {
		const stored = hasTauriRuntime()
			? await invoke<AppLogFile>("read_app_logs")
			: {
					path: "Browser preview local storage",
					entries: readBrowserLogs(),
				};

		logFilePath.value = stored.path;
		const parsedLogs = stored.entries.slice(-maxVisibleLogs).map(parseLogLine).filter((entry): entry is AppLogEntry => Boolean(entry));
		logs.splice(0, logs.length, ...normalizeLogEntries(parsedLogs.slice(-maxVisibleLogs)));
		log("Logger initialized.", { level: "debug", scope: "core.logger", detail: logFilePath.value });
	} catch (error) {
		log("Logger started without persisted history.", {
			level: "warn",
			scope: "core.logger",
			detail: error instanceof Error ? error.message : String(error),
		});
	}
}

export async function refreshLogs() {
	const stored = hasTauriRuntime()
		? await invoke<AppLogFile>("read_app_logs")
		: {
				path: "Browser preview local storage",
				entries: readBrowserLogs(),
			};

	logFilePath.value = stored.path;
	const parsedLogs = stored.entries.slice(-maxVisibleLogs).map(parseLogLine).filter((entry): entry is AppLogEntry => Boolean(entry));
	logs.splice(0, logs.length, ...normalizeLogEntries(parsedLogs.slice(-maxVisibleLogs)));
}

export async function clearLogs() {
	if (hasTauriRuntime()) {
		await persistQueue;
		await invoke<void>("clear_app_logs");
	} else {
		localStorage.removeItem(browserLogKey);
	}

	logs.splice(0, logs.length);
	log("Log history cleared.", { level: "warn", scope: "core.logger" });
}
