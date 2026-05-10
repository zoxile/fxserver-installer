import { invoke } from "@tauri-apps/api/core";

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
		scope: options.scope ?? "app",
		message,
		detail: options.detail,
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
		const nextLogs = [...readBrowserLogs(), line].slice(-maxVisibleLogs);
		localStorage.setItem(browserLogKey, JSON.stringify(nextLogs));
		return;
	}

	persistQueue = persistQueue
		.then(() => invoke<void>("append_app_log", { entry: line }))
		.catch((error) => {
			console.error("Could not persist app log entry.", error);
		});
}

function readBrowserLogs() {
	try {
		return JSON.parse(localStorage.getItem(browserLogKey) || "[]") as string[];
	} catch {
		return [];
	}
}

function parseLogLine(line: string): AppLogEntry | null {
	try {
		const parsed = JSON.parse(line) as Partial<AppLogEntry>;
		if (!parsed.timestamp || !parsed.level || !parsed.scope || !parsed.message) return null;

		return {
			id: parsed.id ?? `${parsed.timestamp}-${parsed.scope}`,
			timestamp: parsed.timestamp,
			level: parsed.level,
			scope: parsed.scope,
			message: parsed.message,
			detail: parsed.detail,
		};
	} catch {
		return null;
	}
}

export function log(message: string, options: LogOptions = {}) {
	const entry = createLogEntry(message, options);
	logs.push(entry);
	trimVisibleLogs();
	persistEntry(entry);

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
		const parsedLogs = stored.entries.map(parseLogLine).filter((entry): entry is AppLogEntry => Boolean(entry));
		logs.splice(0, logs.length, ...parsedLogs.slice(-maxVisibleLogs));
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
	const parsedLogs = stored.entries.map(parseLogLine).filter((entry): entry is AppLogEntry => Boolean(entry));
	logs.splice(0, logs.length, ...parsedLogs.slice(-maxVisibleLogs));
}

export async function clearLogs() {
	if (hasTauriRuntime()) {
		await invoke<void>("clear_app_logs");
	} else {
		localStorage.removeItem(browserLogKey);
	}

	logs.splice(0, logs.length);
	log("Log history cleared.", { level: "warn", scope: "core.logger" });
}
