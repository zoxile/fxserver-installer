import { invoke } from "@tauri-apps/api/core";

export interface JooatShardManifest {
	prefix: string;
	path: string;
	hashes?: number | null;
	bytes?: number | null;
}

export interface JooatResolverManifest {
	version: string;
	source?: string | null;
	generatedAt?: string | null;
	totalHashes?: number | null;
	totalNames?: number | null;
	sizeBytes?: number | null;
	shards: JooatShardManifest[];
}

export interface JooatResolverStatus {
	available: boolean;
	databaseDir: string;
	manifest?: JooatResolverManifest | null;
	installedShards: number;
	expectedShards: number;
	sizeBytes: number;
	message: string;
}

export interface JooatResolvedHash {
	query: string;
	value?: number | null;
	hex?: string | null;
	unsigned?: string | null;
	signed?: string | null;
	matches: string[];
	error?: string | null;
}

export interface InstallJooatResolverOptions {
	manifestUrl: string;
	onProgress?: (progress: JooatInstallProgress) => void;
}

export interface JooatInstallProgress {
	current: number;
	total: number;
	label: string;
}

function hasTauriRuntime() {
	return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function unavailableOutsideTauri<T>(): Promise<T> {
	return Promise.reject(new Error("The offline JOOAT resolver database is available in the Tauri desktop app."));
}

export function getJooatResolverStatus() {
	if (!hasTauriRuntime()) {
		return Promise.resolve({
			available: false,
			databaseDir: "",
			manifest: null,
			installedShards: 0,
			expectedShards: 0,
			sizeBytes: 0,
			message: "Desktop resolver database is not available in the browser preview. The hasher still works.",
		} satisfies JooatResolverStatus);
	}

	return invoke<JooatResolverStatus>("get_jooat_resolver_status");
}

export function resolveJooatHashes(queries: string[]) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<JooatResolvedHash[]>();
	return invoke<JooatResolvedHash[]>("resolve_jooat_hashes", { queries });
}

export function removeJooatResolverDatabase() {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<JooatResolverStatus>();
	return invoke<JooatResolverStatus>("remove_jooat_resolver_database");
}

export async function installJooatResolverDatabase({ manifestUrl, onProgress }: InstallJooatResolverOptions) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<JooatResolverStatus>();

	const manifestResponse = await fetch(manifestUrl);
	if (!manifestResponse.ok) {
		throw new Error(`Could not download resolver manifest: ${manifestResponse.status}`);
	}

	const manifest = (await manifestResponse.json()) as JooatResolverManifest;
	validateManifest(manifest);

	let status = await invoke<JooatResolverStatus>("prepare_jooat_resolver_database", { manifest });
	const baseUrl = new URL(".", manifestUrl);

	for (let index = 0; index < manifest.shards.length; index += 1) {
		const shard = manifest.shards[index];
		onProgress?.({ current: index + 1, total: manifest.shards.length, label: shard.prefix });

		const shardUrl = new URL(shard.path, baseUrl);
		const shardResponse = await fetch(shardUrl);
		if (!shardResponse.ok) {
			throw new Error(`Could not download JOOAT shard ${shard.prefix}: ${shardResponse.status}`);
		}

		status = await invoke<JooatResolverStatus>("save_jooat_resolver_shard", {
			prefix: shard.prefix,
			content: await shardResponse.text(),
		});
	}

	return status;
}

function validateManifest(manifest: JooatResolverManifest) {
	if (!manifest || typeof manifest !== "object") {
		throw new Error("Resolver manifest must be a JSON object.");
	}

	if (!manifest.version || !Array.isArray(manifest.shards) || manifest.shards.length === 0) {
		throw new Error("Resolver manifest must include a version and at least one shard.");
	}

	for (const shard of manifest.shards) {
		if (!/^[0-9a-f]{2}$/i.test(shard.prefix) || !shard.path) {
			throw new Error("Resolver manifest contains an invalid shard entry.");
		}
	}
}
