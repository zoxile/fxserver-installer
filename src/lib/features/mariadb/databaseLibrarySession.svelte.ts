import { onDestroy, untrack } from "svelte";
import { databaseSession, isDatabaseSessionValidated } from "$lib/core/databaseSession.svelte";
import { getWorkspaceId } from "$lib/core/workspaces.svelte";
import { DATABASE_LIBRARY_KEY, databaseLibraryScope, emptyDatabaseLibrary, readDatabaseLibrary, resetDatabaseLibrary, writeDatabaseLibrary, type DatabaseLibrary } from "./databaseLibrary";

export function currentLibrarySecrets() {
	return [databaseSession.credentials?.password ?? "", databaseSession.validated?.credentials.password ?? "", databaseSession.connectionString];
}

export function createDatabaseLibrarySession() {
	const state = $state({ library: emptyDatabaseLibrary(), scope: "", error: "", ready: false, readOnly: false });
	let active = true;
	let generation = 0;
	let validated: typeof databaseSession.validated = null;
	let workspace = "";
	const current = () => active && validated !== null && validated === databaseSession.validated && workspace === getWorkspaceId() && isDatabaseSessionValidated(validated.credentials);
	function reload() {
		if (!current()) return;
		try {
			const result = readDatabaseLibrary(localStorage, currentLibrarySecrets());
			state.library = result.library;
			state.error = result.warning;
			state.readOnly = result.readOnly;
			state.ready = !result.readOnly;
		} catch { state.ready = false; state.error = "Local database library is unavailable. Check storage access."; }
	}
	$effect(() => {
		const connection = databaseSession.validated;
		const workspaceId = getWorkspaceId();
		databaseSession.revision;
		databaseSession.credentials?.password;
		untrack(() => {
			const request = ++generation;
			validated = connection; workspace = workspaceId;
			state.ready = false; state.readOnly = false; state.scope = ""; state.library = emptyDatabaseLibrary(); state.error = "";
			if (!connection || !current()) return;
			void databaseLibraryScope(workspaceId, connection.credentials).then((scope) => {
				if (!current() || request !== generation) return;
				state.scope = scope; reload();
			}).catch(() => { if (active && request === generation) state.error = "Could not establish the saved library's connection scope."; });
		});
	});
	$effect(() => {
		const changed = (event: StorageEvent) => { if (event.key === DATABASE_LIBRARY_KEY || event.key === null) reload(); };
		window.addEventListener("storage", changed);
		return () => window.removeEventListener("storage", changed);
	});
	onDestroy(() => { active = false; generation++; });
	function update(change: (library: DatabaseLibrary, scope: string) => void) {
		if (!current() || !state.ready || !state.scope) return false;
		state.error = "";
		try {
			// Read again before every explicit write so the other feature is preserved.
			const latest = readDatabaseLibrary(localStorage, currentLibrarySecrets());
			if (latest.readOnly) { state.library = latest.library; state.error = latest.warning; state.readOnly = true; state.ready = false; return false; }
			change(latest.library, state.scope);
			writeDatabaseLibrary(localStorage, latest.library, currentLibrarySecrets());
			state.library = latest.library;
			return true;
		} catch (error) {
			state.error = error instanceof Error ? error.message : "Could not update the saved database library.";
			return false;
		}
	}
	function reset(confirmation: string) {
		if (!current() || !state.readOnly) return false;
		try { resetDatabaseLibrary(localStorage, confirmation); reload(); return state.ready; }
		catch (error) { state.error = error instanceof Error ? error.message : "Could not reset the library."; return false; }
	}
	return { state, update, reload, reset };
}
