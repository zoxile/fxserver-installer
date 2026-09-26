import { BROWSER_CATALOG_TTL, type DatabaseBrowserCache } from "$lib/core/databaseBrowserCache";
import type { BrowserMetadata, BrowserPage, BrowserRequest } from "$lib/modules/databaseBrowser";
import { bookmarkRequest, emptyDatabaseLibrary, validateDatabaseLibrary, type DatabaseBookmark } from "./databaseLibrary";

interface BookmarkLoaders {
	scope: string;
	current: () => boolean;
	cache: DatabaseBrowserCache;
	databases: () => Promise<string[]>;
	tables: (database: string) => Promise<string[]>;
	metadata: (database: string, table: string) => Promise<BrowserMetadata>;
	rows: (request: BrowserRequest) => Promise<BrowserPage>;
}

// Stage the whole view first. The caller commits it only while its session is current.
export async function loadDatabaseBookmark(bookmark: DatabaseBookmark, loaders: BookmarkLoaders, secrets: readonly string[] = []) {
	if (!loaders.current()) return null;
	if (bookmark.scope !== loaders.scope || !validateDatabaseLibrary({ ...emptyDatabaseLibrary(), bookmarks: [bookmark] }, secrets)) throw new Error("This bookmark is invalid or belongs to another workspace or connection.");
	const request = bookmarkRequest(bookmark);
	const databases = await loaders.cache.read(["databases"], loaders.databases, BROWSER_CATALOG_TTL);
	if (!loaders.current()) return null;
	if (!databases.includes(request.database)) throw new Error("The bookmarked database is no longer available. The current view was kept.");
	const tables = await loaders.cache.read(["tables", request.database], () => loaders.tables(request.database), BROWSER_CATALOG_TTL);
	if (!loaders.current()) return null;
	if (!tables.includes(request.table)) throw new Error("The bookmarked table is no longer available. The current view was kept.");
	const metadata = await loaders.cache.read(["metadata", request.database, request.table], () => loaders.metadata(request.database, request.table));
	if (!loaders.current()) return null;
	const columns = new Set(metadata.columns.map((column) => column.name));
	if (!columns.size || request.filters.some((filter) => !columns.has(filter.column)) || (request.sortColumn !== null && !columns.has(request.sortColumn))) throw new Error("Bookmarked filter or sort columns no longer exist. The current view was kept; no filters were dropped.");
	request.pageSize = Math.min(request.pageSize, Math.max(1, Math.floor(4000 / columns.size)));
	request.sortColumn ??= metadata.indexes.find((index) => index.name === "PRIMARY")?.column ?? metadata.columns[0].name;
	const page = bookmark.view === "rows"
		? await loaders.cache.read(["rows", request, metadata.columns], () => loaders.rows(request))
		: { rows: [], hasMore: false, truncatedCells: false } satisfies BrowserPage;
	if (!loaders.current()) return null;
	if (page.pageSize) request.pageSize = page.pageSize;
	return { request, databases, tables, metadata, page, view: bookmark.view };
}
