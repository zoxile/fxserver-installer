import type { BrowserFilter, BrowserRequest } from "$lib/modules/databaseBrowser";
import type { InspectionView } from "$lib/modules/databaseInspection";

export type BrowserView = "rows" | "columns" | "indexes" | "tables" | InspectionView;
export interface BrowserLocation extends BrowserRequest {
	view: BrowserView;
	draftFilters: BrowserFilter[];
}

export const BROWSER_CACHE_TTL = 30_000;
export const BROWSER_CATALOG_TTL = 120_000;
const MAX_ENTRIES = 48;
const MAX_BYTES = 8 * 1024 * 1024;

interface Entry { value: unknown; expires: number; bytes: number }

// Approximate retained size without serializing large row pages on the UI thread.
function retainedBytes(value: unknown, limit: number): number {
	if (typeof value === "string") return 24 + value.length * 2;
	if (!value || typeof value !== "object") return 8;
	let bytes = 32;
	for (const [key, item] of Object.entries(value)) {
		bytes += key.length * 2 + retainedBytes(item, limit - bytes);
		if (bytes > limit) break;
	}
	return bytes;
}

export class DatabaseBrowserCache {
	private entries = new Map<string, Entry>();
	private pending = new Map<string, Promise<unknown>>();
	private locations = new Map<string, BrowserLocation>();
	private bytes = 0;
	private generation = 0;
	lastDatabase = "";

	read<T>(key: readonly unknown[], load: () => Promise<T>, ttl = BROWSER_CACHE_TTL): Promise<T> {
		const id = JSON.stringify(key);
		const cached = this.entries.get(id);
		if (cached && cached.expires > Date.now()) {
			this.entries.delete(id);
			this.entries.set(id, cached);
			return Promise.resolve(cached.value as T);
		}
		if (cached) this.remove(id);
		const existing = this.pending.get(id);
		if (existing) return existing as Promise<T>;
		const generation = this.generation;
		const pending = Promise.resolve().then(load).then((value) => {
			if (generation !== this.generation) return value;
			const bytes = retainedBytes(value, MAX_BYTES) + id.length * 2;
			if (bytes <= MAX_BYTES) {
				while (this.entries.size && (this.entries.size >= MAX_ENTRIES || this.bytes + bytes > MAX_BYTES)) {
					this.remove(this.entries.keys().next().value!);
				}
				this.entries.set(id, { value, bytes, expires: Date.now() + ttl });
				this.bytes += bytes;
			}
			return value;
		}).finally(() => {
			if (this.pending.get(id) === pending) this.pending.delete(id);
		});
		this.pending.set(id, pending);
		return pending;
	}

	saveLocation(location: BrowserLocation) {
		this.locations.delete(location.database);
		this.locations.set(location.database, structuredClone(location));
		if (this.locations.size > 8) this.locations.delete(this.locations.keys().next().value!);
		this.lastDatabase = location.database;
	}

	location(database = this.lastDatabase) {
		const location = this.locations.get(database);
		return location ? structuredClone(location) : undefined;
	}

	clear() {
		this.generation++;
		this.entries.clear();
		this.pending.clear();
		this.bytes = 0;
	}

	private remove(key: string) {
		const entry = this.entries.get(key);
		if (entry) this.bytes -= entry.bytes;
		this.entries.delete(key);
	}
}

let session: { scope: object; cache: DatabaseBrowserCache } | undefined;

export function getDatabaseBrowserCache(scope: object) {
	if (session?.scope !== scope) {
		resetDatabaseBrowserCache();
		session = { scope, cache: new DatabaseBrowserCache() };
	}
	return session.cache;
}

export function resetDatabaseBrowserCache() {
	session?.cache.clear();
	session = undefined;
}

const mutations = new Set([
	"execute_mariadb_query", "restore_backup_snapshot", "test_backup_restore",
	"apply_database_browser_change", "apply_database_admin_action",
	"save_mariadb_user", "update_mariadb_user", "delete_mariadb_user",
	"install_mariadb", "update_mariadb", "uninstall_mariadb",
	"start_mariadb_service", "stop_mariadb_service", "restart_mariadb_service",
]);

export function invalidateDatabaseBrowserCommand(command: string) {
	if (mutations.has(command)) session?.cache.clear();
}
