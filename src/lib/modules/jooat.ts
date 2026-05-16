import { invoke } from "@tauri-apps/api/core";
import { log } from "$lib/core/logger.svelte";

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

const completeShardCount = 256;

function hasTauriRuntime() {
	return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function unavailableOutsideTauri<T>(): Promise<T> {
	log("Offline JOOAT resolver action blocked outside the desktop runtime.", { level: "warn", scope: "jooat.runtime" });
	return Promise.reject(new Error("The offline JOOAT resolver database is available in the Tauri desktop app."));
}

function errorMessage(error: unknown) {
	return error instanceof Error ? error.message : String(error);
}

export function getJooatResolverStatus() {
	if (!hasTauriRuntime()) {
		log("JOOAT resolver status requested in browser preview.", { level: "debug", scope: "jooat.status" });
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

	log("JOOAT resolver status refresh started.", { scope: "jooat" });
	return invoke<JooatResolverStatus>("get_jooat_resolver_status")
		.then((status) => {
			log(status.available ? `JOOAT resolver is installed with ${status.installedShards} shards.` : "JOOAT resolver database is not installed.", {
				level: status.available ? "success" : "info",
				scope: "jooat",
			});
			return status;
		})
		.catch((error) => {
			log("JOOAT resolver status refresh failed.", { level: "error", scope: "jooat", detail: errorMessage(error) });
			throw error;
		});
}

export function resolveJooatHashes(queries: string[]) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<JooatResolvedHash[]>();
	log(`JOOAT resolver lookup started for ${queries.length} hashes.`, { scope: "jooat" });
	return invoke<JooatResolvedHash[]>("resolve_jooat_hashes", { queries })
		.then((results) => {
			const matched = results.filter((result) => result.matches.length > 0).length;
			log(`JOOAT resolver lookup completed with ${matched} matches.`, { level: "success", scope: "jooat" });
			return results;
		})
		.catch((error) => {
			log("JOOAT resolver lookup failed.", { level: "error", scope: "jooat", detail: errorMessage(error) });
			throw error;
		});
}

export function removeJooatResolverDatabase() {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<JooatResolverStatus>();
	log("JOOAT resolver database removal started.", { level: "warn", scope: "jooat" });
	return invoke<JooatResolverStatus>("remove_jooat_resolver_database")
		.then((status) => {
			log("JOOAT resolver database removed.", { level: "success", scope: "jooat", detail: status.databaseDir });
			return status;
		})
		.catch((error) => {
			log("JOOAT resolver database removal failed.", { level: "error", scope: "jooat", detail: errorMessage(error) });
			throw error;
		});
}

export async function installJooatResolverDatabase({ manifestUrl, onProgress }: InstallJooatResolverOptions) {
	if (!hasTauriRuntime()) return unavailableOutsideTauri<JooatResolverStatus>();

	log("JOOAT resolver database install started.", { scope: "jooat", detail: manifestUrl });
	const manifestResponse = await fetch(manifestUrl);
	if (!manifestResponse.ok) {
		log("JOOAT resolver manifest download failed.", { level: "error", scope: "jooat", detail: `${manifestResponse.status}` });
		throw new Error(`Could not download resolver manifest: ${manifestResponse.status}`);
	}

	const manifest = (await manifestResponse.json()) as JooatResolverManifest;
	validateManifest(manifest);
	log(`JOOAT resolver manifest validated with ${manifest.shards.length} shards.`, { level: "success", scope: "jooat", detail: manifest.version });

	let status = await invoke<JooatResolverStatus>("prepare_jooat_resolver_database", { manifest });
	const baseUrl = new URL(".", manifestUrl);

	for (let index = 0; index < manifest.shards.length; index += 1) {
		const shard = manifest.shards[index];
		onProgress?.({ current: index + 1, total: manifest.shards.length, label: shard.prefix });

		const shardUrl = new URL(shard.path, baseUrl);
		const shardResponse = await fetch(shardUrl);
		if (!shardResponse.ok) {
			log(`JOOAT resolver shard ${shard.prefix} download failed.`, { level: "error", scope: "jooat", detail: `${shardResponse.status}` });
			throw new Error(`Could not download JOOAT shard ${shard.prefix}: ${shardResponse.status}`);
		}

		status = await invoke<JooatResolverStatus>("save_jooat_resolver_shard", {
			prefix: shard.prefix,
			content: await shardResponse.text(),
		});
	}

	log(`JOOAT resolver database install completed with ${status.installedShards} shards.`, { level: "success", scope: "jooat" });
	return status;
}

function validateManifest(manifest: JooatResolverManifest) {
	if (!manifest || typeof manifest !== "object") {
		throw new Error("Resolver manifest must be a JSON object.");
	}

	if (!manifest.version || !Array.isArray(manifest.shards) || manifest.shards.length === 0) {
		throw new Error("Resolver manifest must include a version and at least one shard.");
	}

	if (!isCompleteManifest(manifest)) {
		throw new Error("Resolver manifest must include the complete 256-shard database from 00 through ff.");
	}

	for (const shard of manifest.shards) {
		if (!/^[0-9a-f]{2}$/i.test(shard.prefix) || !shard.path) {
			throw new Error("Resolver manifest contains an invalid shard entry.");
		}
	}
}

function isCompleteManifest(manifest: JooatResolverManifest) {
	if (manifest.shards.length !== completeShardCount) return false;

	const prefixes = new Set(manifest.shards.map((shard) => shard.prefix.toLowerCase()));
	for (let index = 0; index < completeShardCount; index += 1) {
		if (!prefixes.has(index.toString(16).padStart(2, "0"))) return false;
	}

	return true;
}
