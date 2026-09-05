import { sensitiveTxHostKeys } from "$lib/features/fxserver/fxserverEnv";

export interface Workspace {
	id: string;
	name: string;
	artifactPath: string;
	txDataPath: string;
	profile: string;
	environment: Record<string, string>;
	database: { host: string; port: number; username: string; database: string };
}

export function publicEnvironment(values: Record<string, string>) {
	return Object.fromEntries(Object.entries(values).filter(([key, value]) =>
		typeof value === "string" && !sensitiveTxHostKeys.has(key) && !/password|secret|token|license.?key/i.test(key),
	));
}

export function emptyWorkspace(id: string, name: string): Workspace {
	return {
		id, name, artifactPath: "", txDataPath: "", profile: "", environment: {},
		database: { host: "localhost", port: 3306, username: "root", database: "" },
	};
}

export function parseWorkspaces(raw: string | null): { activeId: string; items: Workspace[] } | null {
	try {
		const value = JSON.parse(raw ?? "null");
		if (!value || !Array.isArray(value.items) || value.items.length > 50) return null;
		const ids = new Set<string>();
		const items: Workspace[] = value.items.map((item: Workspace) => {
			if (!item || !/^(default|[a-f0-9-]{36})$/.test(item.id) || ids.has(item.id)) throw new Error("Invalid workspace ID");
			ids.add(item.id);
			for (const key of ["name", "artifactPath", "txDataPath", "profile"] as const) {
				if (typeof item[key] !== "string") throw new Error("Invalid workspace settings");
			}
			const defaults = emptyWorkspace(item.id, item.name);
			return {
				...defaults, name: item.name.trim().slice(0, 80) || "Workspace",
				artifactPath: item.artifactPath, txDataPath: item.txDataPath, profile: item.profile,
				environment: publicEnvironment(item.environment && typeof item.environment === "object" ? item.environment : {}),
				database: {
					host: typeof item.database?.host === "string" ? item.database.host : "localhost",
					port: Number.isInteger(item.database?.port) && item.database.port > 0 && item.database.port <= 65535 ? item.database.port : 3306,
					username: typeof item.database?.username === "string" ? item.database.username : "root",
					database: typeof item.database?.database === "string" ? item.database.database : "",
				},
			};
		});
		return items.length && ids.has(value.activeId) ? { activeId: value.activeId, items } : null;
	} catch {
		return null;
	}
}
