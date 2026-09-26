import { redactIncidentText } from "$lib/core/incidentModel";
import type { BrowserFilter, BrowserRequest } from "$lib/modules/databaseBrowser";
import { MAX_TEMPLATE_LENGTH, queryParameters } from "./savedQueryParameters";

export const DATABASE_LIBRARY_KEY = "fxserver-installer.database-library.v1";
export const LIBRARY_LIMITS = { folders: 32, queries: 100, bookmarks: 100, bytes: 512 * 1024 } as const;
export interface QueryFolder { id: string; scope: string; name: string }
export interface SavedQuery extends QueryFolder { folderId: string | null; database: string; sql: string }
export interface DatabaseBookmark extends QueryFolder, BrowserRequest { view: "rows" | "columns" | "indexes" }
export interface DatabaseLibrary { version: 1; folders: QueryFolder[]; queries: SavedQuery[]; bookmarks: DatabaseBookmark[] }
type StoragePort = Pick<Storage, "getItem" | "setItem" | "removeItem">;
export const emptyDatabaseLibrary = (): DatabaseLibrary => ({ version: 1, folders: [], queries: [], bookmarks: [] });
const sensitiveName = /password|passwd|dbpass|pwd|secret|token|license.?key|api.?key|connection.?string|authorization|cookie|webhook|credential|private.?key/i;
const identifier = (value: unknown): value is string => typeof value === "string" && value.length > 0 && new TextEncoder().encode(value).length <= 256 && !/[\u0000-\u001f\u007f]/.test(value);
const record = (value: unknown): value is Record<string, unknown> => Boolean(value && typeof value === "object" && !Array.isArray(value));
const exactKeys = (value: Record<string, unknown>, keys: string[]) => Object.keys(value).length === keys.length && keys.every((key) => Object.hasOwn(value, key));
const integer = (value: unknown, min: number, max: number): value is number => typeof value === "number" && Number.isInteger(value) && value >= min && value <= max;

export function containsLibrarySecret(text: string, knownSecrets: readonly string[] = []): boolean {
	// Inspect hex literals as well, including the draft encoder's output.
	let encodedSecret = false;
	const decoded = text.replace(/(?:\b0x([a-f\d]+)\b|\bX'([a-f\d]*)')/gi, (_, bare: string, quoted: string) => {
		const hex = bare ?? quoted;
		if (hex.length % 2) return "";
		const literal = new TextDecoder().decode(Uint8Array.from(hex.match(/../g) ?? [], (byte) => Number.parseInt(byte, 16)));
		if (literal && knownSecrets.includes(literal)) encodedSecret = true;
		return literal;
	});
	if (encodedSecret) return true;
	for (const candidate of [text, decoded]) {
		// The shared redactor has unbounded credential-key patterns. Overlapping,
		// bounded windows keep long ordinary SQL literals from causing quadratic work.
		for (let start = 0; start < candidate.length; start += 256) {
			const window = candidate.slice(start, start + 512);
			if (!sensitiveName.test(window) && !/\/\/|cfxk_|gh[pousr]_|github_pat_|sk-|eyJ|IDENTIFIED|Bearer|PRIVATE KEY|default_account|[\u0000-\u001f]/i.test(window)) continue;
			if (redactIncidentText(window, window.length + 1) !== window) return true;
		}
		if (/\bIDENTIFIED\s+(?:BY|VIA|WITH)|\bSET\s+PASSWORD\b|-----BEGIN .*PRIVATE KEY/i.test(candidate)) return true;
		if (/\b(?:INSERT|UPDATE|REPLACE|CREATE|ALTER)\b/i.test(candidate) && sensitiveName.test(candidate)) return true;
		for (const secret of knownSecrets.filter(Boolean)) {
			const variants = [secret, encodeURIComponent(secret), secret.replaceAll("'", "''"), secret.replaceAll("\\", "\\\\").replaceAll("'", "\\'")];
			// Short passwords such as "a" must not invalidate every identifier or SQL
			// keyword containing that character. Exact values/quoted literals still match.
			if (variants.some((variant) => secret.length >= 8
				? candidate.includes(variant)
				: candidate === variant || candidate.includes(`'${variant}'`) || candidate.includes(`"${variant}"`))) return true;
		}
	}
	return false;
}

function safeStrings(value: unknown, secrets: readonly string[]): boolean {
	if (typeof value === "string") return !containsLibrarySecret(value, secrets);
	if (Array.isArray(value)) return value.every((item) => safeStrings(item, secrets));
	return !record(value) || Object.values(value).every((item) => safeStrings(item, secrets));
}

function base(value: Record<string, unknown>) {
	return typeof value.id === "string" && /^[\da-f-]{36}$/.test(value.id)
		&& typeof value.scope === "string" && /^[\da-f]{64}$/.test(value.scope)
		&& typeof value.name === "string" && value.name.trim() === value.name && value.name.length > 0 && value.name.length <= 80 && !/[\u0000-\u001f\u007f]/.test(value.name);
}

function validFilter(value: unknown): value is BrowserFilter {
	if (!record(value) || !exactKeys(value, ["column", "operator", "value"]) || !identifier(value.column) || sensitiveName.test(value.column)) return false;
	if (!["eq", "ne", "contains", "lt", "lte", "gt", "gte", "isNull", "isNotNull"].includes(String(value.operator))) return false;
	if (value.operator === "isNull" || value.operator === "isNotNull") return value.value === null;
	return typeof value.value === "string" && value.value.length <= 512;
}

export function validateDatabaseLibrary(value: unknown, secrets: readonly string[] = []): value is DatabaseLibrary {
	if (!record(value) || !exactKeys(value, ["version", "folders", "queries", "bookmarks"]) || value.version !== 1) return false;
	if (!Array.isArray(value.folders) || value.folders.length > LIBRARY_LIMITS.folders || !Array.isArray(value.queries) || value.queries.length > LIBRARY_LIMITS.queries || !Array.isArray(value.bookmarks) || value.bookmarks.length > LIBRARY_LIMITS.bookmarks) return false;
	const ids = new Set<string>();
	const names = new Set<string>();
	for (const [kind, entries] of [["folders", value.folders], ["queries", value.queries], ["bookmarks", value.bookmarks]] as const) {
		for (const item of entries) {
			if (!record(item) || !base(item) || !safeStrings(item, secrets)) return false;
			if (ids.has(item.id as string)) return false;
			ids.add(item.id as string);
			const nameKey = JSON.stringify([kind, item.scope, kind === "queries" ? item.folderId : null, (item.name as string).toLowerCase()]);
			if (names.has(nameKey)) return false;
			names.add(nameKey);
			if (kind === "folders") {
				if (!exactKeys(item, ["id", "scope", "name"])) return false;
			} else if (kind === "queries") {
				if (!exactKeys(item, ["id", "scope", "name", "folderId", "database", "sql"]) || !identifier(item.database) || typeof item.sql !== "string" || !item.sql.trim() || item.sql.length > MAX_TEMPLATE_LENGTH) return false;
				if (item.folderId !== null && !value.folders.some((folder) => record(folder) && folder.id === item.folderId && folder.scope === item.scope)) return false;
				try { queryParameters(item.sql); } catch { return false; }
			} else {
				if (!exactKeys(item, ["id", "scope", "name", "database", "table", "view", "filters", "sortColumn", "descending", "offset", "pageSize"])) return false;
				if (!identifier(item.database) || !identifier(item.table) || !["rows", "columns", "indexes"].includes(String(item.view))) return false;
				if (!Array.isArray(item.filters) || item.filters.length > 8 || !item.filters.every(validFilter)) return false;
				if (item.sortColumn !== null && !identifier(item.sortColumn)) return false;
				if (typeof item.descending !== "boolean" || !integer(item.offset, 0, 1_000_000) || !integer(item.pageSize, 1, 200)) return false;
			}
		}
	}
	return true;
}

function serialize(library: DatabaseLibrary, secrets: readonly string[]) {
	if (!validateDatabaseLibrary(library, secrets)) throw new Error("Cannot save: check names, duplicates, limits, and sensitive SQL or filter values. Credentials and secret fields are not allowed.");
	const json = JSON.stringify(library);
	if (new TextEncoder().encode(json).length > LIBRARY_LIMITS.bytes) throw new Error("Saved database library exceeds its 512 KiB limit. Remove unused items.");
	return json;
}

export function readDatabaseLibrary(storage: StoragePort, secrets: readonly string[] = []): { library: DatabaseLibrary; warning: string; readOnly: boolean } {
	const raw = storage.getItem(DATABASE_LIBRARY_KEY);
	if (raw === null) return { library: emptyDatabaseLibrary(), warning: "", readOnly: false };
	try {
		if (raw.length > LIBRARY_LIMITS.bytes || new TextEncoder().encode(raw).length > LIBRARY_LIMITS.bytes) throw new Error();
		const library: unknown = JSON.parse(raw);
		if (!validateDatabaseLibrary(library, secrets)) throw new Error();
		return { library, warning: "", readOnly: false };
	} catch {
		return { library: emptyDatabaseLibrary(), warning: "Saved database data failed validation or a sensitive-data check. It remains unchanged in local storage. The library is read-only until the data is repaired or you explicitly reset it.", readOnly: true };
	}
}

export function writeDatabaseLibrary(storage: StoragePort, library: DatabaseLibrary, secrets: readonly string[] = []) {
	// Never allow an empty read fallback (or a concurrent invalid write) to replace
	// data we could not safely interpret. Reset is a separate, confirmed operation.
	if (readDatabaseLibrary(storage, secrets).readOnly) throw new Error("The existing library is read-only. Repair the saved data or explicitly confirm a reset before saving.");
	const json = serialize(library, secrets);
	try { storage.setItem(DATABASE_LIBRARY_KEY, json); }
	catch { throw new Error("Local database library could not be saved. Storage may be full or disabled."); }
}

export function resetDatabaseLibrary(storage: StoragePort, confirmation: string) {
	if (confirmation !== "RESET") throw new Error("Type RESET to delete the entire saved database library.");
	try { storage.removeItem(DATABASE_LIBRARY_KEY); }
	catch { throw new Error("The saved database library could not be reset. Storage may be disabled."); }
}

// An opaque partition, not an authentication token. Passwords never enter the digest
// or storage. Scope changes on workspace/server/user changes, but not password rotation.
export async function databaseLibraryScope(workspace: string, connection: { host: string; port: number; username: string }) {
	const bytes = new TextEncoder().encode(JSON.stringify([workspace, connection.host.trim().toLowerCase(), Number(connection.port), connection.username]));
	return [...new Uint8Array(await crypto.subtle.digest("SHA-256", bytes))].map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

export function bookmarkRequest(bookmark: DatabaseBookmark): BrowserRequest {
	return { database: bookmark.database, table: bookmark.table, filters: bookmark.filters.map((filter) => ({ ...filter })), sortColumn: bookmark.sortColumn, descending: bookmark.descending, offset: bookmark.offset, pageSize: bookmark.pageSize };
}
