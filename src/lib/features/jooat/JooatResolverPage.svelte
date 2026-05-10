<script lang="ts">
	import BinaryIcon from "@lucide/svelte/icons/binary";
	import CopyIcon from "@lucide/svelte/icons/copy";
	import DatabaseZapIcon from "@lucide/svelte/icons/database-zap";
	import DownloadIcon from "@lucide/svelte/icons/download";
	import HashIcon from "@lucide/svelte/icons/hash";
	import HardDriveDownloadIcon from "@lucide/svelte/icons/hard-drive-download";
	import Loader2Icon from "@lucide/svelte/icons/loader-2";
	import ListChecksIcon from "@lucide/svelte/icons/list-checks";
	import RotateCcwIcon from "@lucide/svelte/icons/rotate-ccw";
	import SearchIcon from "@lucide/svelte/icons/search";
	import ShieldCheckIcon from "@lucide/svelte/icons/shield-check";
	import SparklesIcon from "@lucide/svelte/icons/sparkles";
	import Trash2Icon from "@lucide/svelte/icons/trash-2";
	import { onMount } from "svelte";
	import { Button } from "$lib/components/ui/button/index.js";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import {
		getJooatResolverStatus,
		installJooatResolverDatabase,
		removeJooatResolverDatabase,
		resolveJooatHashes,
		type JooatResolvedHash,
		type JooatResolverStatus,
		type JooatInstallProgress,
	} from "$lib/modules/jooat";
	import HashResultTable from "./HashResultTable.svelte";
	import JooatStatCard from "./JooatStatCard.svelte";
	import ResolverResultList from "./ResolverResultList.svelte";
	import { formatHash, resolveHashes, uniqueHashResults, type ResolveResult } from "./joaat";

	const sampleNames = ["adder", "police", "s_m_y_cop_01", "weapon_pistol", "prop_roadcone02a", "WEAPON_CARBINERIFLE"].join("\n");
	const sampleHashes = "0xB779A091\n0x79FBB0C5\n453432689\n-2084633992";
	const defaultManifestUrl = "https://github.com/zoxile/fxserver-installer/releases/download/jooat-db/manifest.json";
	const defaultDictionary = [
		"adder",
		"police",
		"police2",
		"police3",
		"s_m_y_cop_01",
		"s_m_m_paramedic_01",
		"weapon_pistol",
		"weapon_carbinerifle",
		"weapon_stungun",
		"prop_roadcone02a",
		"prop_barrier_work05",
		"prop_mp_cone_02",
	].join("\n");

	let namesInput = $state(sampleNames);
	let hashesInput = $state(sampleHashes);
	let dictionaryInput = $state(defaultDictionary);
	let copiedLabel = $state("");
	let resolverMode = $state<"database" | "dictionary">("database");
	let resolverStatus = $state<JooatResolverStatus | null>(null);
	let databaseResults = $state<ResolveResult[]>([]);
	let resolverBusy = $state(false);
	let resolverNotice = $state("");
	let manifestUrl = $state(defaultManifestUrl);
	let installProgress = $state<JooatInstallProgress | null>(null);

	let hashRows = $derived(uniqueHashResults(namesInput));
	let dictionaryResults = $derived(resolveHashes(hashesInput, dictionaryInput));
	let resolverResults = $derived(resolverMode === "database" ? databaseResults : dictionaryResults);
	let resolvedCount = $derived(resolverResults.filter((result) => result.matches.length > 0).length);
	let uniqueHashCount = $derived(new Set(hashRows.map((row) => row.value)).size);
	let resolverReady = $derived(resolverMode === "dictionary" || Boolean(resolverStatus?.available));

	onMount(() => {
		void refreshResolverStatus();
	});

	async function copyText(value: string, label: string) {
		await navigator.clipboard.writeText(value);
		copiedLabel = label;
		window.setTimeout(() => {
			if (copiedLabel === label) copiedLabel = "";
		}, 1600);
	}

	function copyHashTable() {
		const output = hashRows.map((row) => `${row.input}\t${row.hex}\t${row.unsigned}\t${row.signed}`).join("\n");
		void copyText(output, "hash table");
	}

	function useHashNamesAsDictionary() {
		const merged = [...new Set([...dictionaryInput.split(/\r?\n/), ...namesInput.split(/\r?\n/)].map((line) => line.trim()).filter(Boolean))];
		dictionaryInput = merged.join("\n");
	}

	function resetSamples() {
		namesInput = sampleNames;
		hashesInput = sampleHashes;
		dictionaryInput = defaultDictionary;
	}

	async function refreshResolverStatus() {
		try {
			resolverStatus = await getJooatResolverStatus();
			if (!resolverStatus.available) {
				resolverMode = "dictionary";
			}
		} catch (error) {
			resolverNotice = error instanceof Error ? error.message : String(error);
			resolverMode = "dictionary";
		}
	}

	async function resolveWithDatabase() {
		resolverBusy = true;
		resolverNotice = "";

		try {
			const queries = hashesInput.split(/[\s,;]+/).map((entry) => entry.trim()).filter(Boolean);
			const results = await resolveJooatHashes(queries);
			databaseResults = results.map(toDisplayResult);
		} catch (error) {
			databaseResults = [];
			resolverNotice = error instanceof Error ? error.message : String(error);
		} finally {
			resolverBusy = false;
		}
	}

	async function installResolverDatabase() {
		resolverBusy = true;
		resolverNotice = "";
		installProgress = null;

		try {
			resolverStatus = await installJooatResolverDatabase({
				manifestUrl,
				onProgress: (progress) => (installProgress = progress),
			});
			resolverMode = resolverStatus.available ? "database" : "dictionary";
			resolverNotice = resolverStatus.message;
		} catch (error) {
			resolverNotice = error instanceof Error ? error.message : String(error);
		} finally {
			installProgress = null;
			resolverBusy = false;
		}
	}

	async function removeResolverDatabase() {
		resolverBusy = true;
		resolverNotice = "";

		try {
			resolverStatus = await removeJooatResolverDatabase();
			resolverMode = "dictionary";
			databaseResults = [];
			resolverNotice = resolverStatus.message;
		} catch (error) {
			resolverNotice = error instanceof Error ? error.message : String(error);
		} finally {
			resolverBusy = false;
		}
	}

	function toDisplayResult(result: JooatResolvedHash): ResolveResult {
		return {
			query: result.query,
			hash: result.value == null ? undefined : formatHash(result.value),
			matches: result.matches,
			error: result.error ?? undefined,
		};
	}

	function formatBytes(value: number) {
		if (!value) return "0 B";
		const units = ["B", "KB", "MB", "GB"];
		const unitIndex = Math.min(Math.floor(Math.log(value) / Math.log(1024)), units.length - 1);
		return `${(value / 1024 ** unitIndex).toFixed(unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
	}
</script>

<section class="space-y-6">
	<div class="flex flex-col gap-4 lg:flex-row lg:items-end lg:justify-between">
		<div>
			<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">Tools</p>
			<h1 class="mt-2 text-3xl font-semibold tracking-normal text-foreground">JOOAT Resolver & Hasher</h1>
			<p class="mt-2 max-w-2xl text-sm leading-6 text-muted-foreground">
				Hash GTA/FiveM names instantly, or enable the optional offline resolver database for local hash lookups without loading a giant dataset into the UI.
			</p>
		</div>

		<div class="flex flex-wrap gap-2">
			<Button variant="outline" class="rounded-sm" onclick={copyHashTable} disabled={!hashRows.length} title="Copy every generated hash row as tab-separated text">
				<CopyIcon class="size-4" />
				Copy Table
			</Button>
			<Button variant="outline" class="rounded-sm" onclick={resetSamples} title="Restore sample names, hashes, and dictionary values">
				<RotateCcwIcon class="size-4" />
				Reset Samples
			</Button>
		</div>
	</div>

	<div class="grid gap-4 md:grid-cols-3">
		<JooatStatCard label="Names" value={String(hashRows.length)} description="unique inputs ready to hash" icon={HashIcon} />
		<JooatStatCard label="Hashes" value={String(uniqueHashCount)} description="unique JOOAT outputs" icon={BinaryIcon} />
		<JooatStatCard label="Resolved" value={`${resolvedCount} / ${resolverResults.length}`} description={resolverMode === "database" ? "database matches" : "dictionary matches"} icon={ShieldCheckIcon} />
	</div>

	<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-transform duration-300 hover:-translate-y-0.5">
		<div class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"></div>
		<Card.Header class="border-b border-border pb-4">
			<div class="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
				<div class="flex items-start gap-3">
					<div class="flex size-9 shrink-0 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
						<HardDriveDownloadIcon class="size-4" />
					</div>
					<div>
						<Card.Title>Optional Resolver Database</Card.Title>
						<Card.Description>
							Hasher-only mode stays lightweight. Installing a resolver pack stores sharded lookup files locally and only reads the shard needed for each hash.
						</Card.Description>
					</div>
				</div>
				<div
					class={[
						"shrink-0 rounded-sm border px-2.5 py-1.5 text-xs",
						resolverStatus?.available ? "border-emerald-400/30 bg-emerald-400/10 text-emerald-300" : "border-border bg-muted text-muted-foreground",
					]}
				>
					{resolverStatus?.available ? "Resolver installed" : "Hasher-only ready"}
				</div>
			</div>
		</Card.Header>
		<Card.Content class="grid gap-4 lg:grid-cols-[minmax(0,1fr)_auto] lg:items-end">
			<div class="grid gap-3">
				<div class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">GitHub manifest URL</span>
					<Input bind:value={manifestUrl} placeholder={defaultManifestUrl} title="Manifest JSON URL for an optional sharded JOOAT resolver database." class="rounded-sm font-mono text-xs" />
				</div>
				<div class="grid gap-2 text-xs text-muted-foreground sm:grid-cols-3">
					<div class="rounded-sm border border-border bg-background/70 p-2">
						<p class="text-foreground">{resolverStatus?.installedShards ?? 0} / {resolverStatus?.expectedShards ?? 0}</p>
						<p>shards installed</p>
					</div>
					<div class="rounded-sm border border-border bg-background/70 p-2">
						<p class="text-foreground">{formatBytes(resolverStatus?.sizeBytes ?? 0)}</p>
						<p>local size</p>
					</div>
					<div class="rounded-sm border border-border bg-background/70 p-2">
						<p class="truncate text-foreground">{resolverStatus?.manifest?.version ?? "none"}</p>
						<p>database version</p>
					</div>
				</div>
				{#if resolverNotice || resolverStatus?.message || installProgress}
					<p class="rounded-sm border border-border bg-background/70 px-3 py-2 text-xs text-muted-foreground">
						{#if installProgress}
							Installing shard {installProgress.current} / {installProgress.total}: {installProgress.label}
						{:else}
							{resolverNotice || resolverStatus?.message}
						{/if}
					</p>
				{/if}
			</div>
			<div class="flex flex-wrap gap-2 lg:justify-end">
				<Button variant="outline" class="rounded-sm" onclick={refreshResolverStatus} disabled={resolverBusy} title="Refresh local resolver database status">
					<RotateCcwIcon class="size-4" />
					Refresh
				</Button>
				<Button class="rounded-sm" onclick={installResolverDatabase} disabled={resolverBusy || !manifestUrl.trim()} title="Download the optional resolver database pack from the manifest URL">
					{#if resolverBusy && installProgress}
						<Loader2Icon class="size-4 animate-spin" />
					{:else}
						<DownloadIcon class="size-4" />
					{/if}
					Install Pack
				</Button>
				<Button variant="destructive" class="rounded-sm" onclick={removeResolverDatabase} disabled={resolverBusy || !resolverStatus?.manifest} title="Remove the local resolver database and keep hasher-only mode">
					<Trash2Icon class="size-4" />
					Remove
				</Button>
			</div>
		</Card.Content>
	</Card.Root>

	<div class="grid gap-4 xl:grid-cols-[minmax(0,1.15fr)_minmax(360px,0.85fr)]">
		<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-transform duration-300 hover:-translate-y-0.5">
			<div class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"></div>
			<Card.Header class="border-b border-border pb-4">
				<div class="flex items-center gap-3">
					<div class="flex size-9 shrink-0 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
						<HashIcon class="size-4" />
					</div>
					<div>
						<Card.Title>Hasher</Card.Title>
						<Card.Description>One value per line. Inputs are trimmed and lowercased before hashing.</Card.Description>
					</div>
				</div>
			</Card.Header>
			<Card.Content class="space-y-4">
				<label class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">Names or keys</span>
					<textarea
						bind:value={namesInput}
						placeholder={"adder\nweapon_pistol\nprop_roadcone02a"}
						title="Enter one model, weapon, native key, or resource string per line."
						class="min-h-40 w-full resize-y rounded-sm border border-input bg-background px-3 py-3 font-mono text-sm shadow-xs outline-none transition-[color,box-shadow] placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
					></textarea>
				</label>

				<HashResultTable rows={hashRows} onCopy={copyText} />
			</Card.Content>
		</Card.Root>

		<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-transform duration-300 hover:-translate-y-0.5">
			<div class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"></div>
			<Card.Header class="border-b border-border pb-4">
				<div class="flex items-center gap-3">
					<div class="flex size-9 shrink-0 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
						<SearchIcon class="size-4" />
					</div>
					<div>
						<Card.Title>Resolver</Card.Title>
						<Card.Description>Use the optional local database, or fall back to a manual candidate dictionary.</Card.Description>
					</div>
				</div>
			</Card.Header>
			<Card.Content class="space-y-4">
				<div class="grid grid-cols-2 gap-2 rounded-sm border border-border bg-background/70 p-1">
					<Button
						variant={resolverMode === "database" ? "secondary" : "ghost"}
						class="rounded-sm"
						onclick={() => (resolverMode = "database")}
						disabled={!resolverStatus?.available}
						title={resolverStatus?.available ? "Use the local resolver database for offline lookups." : "Install the resolver database to enable database mode."}
					>
						<HardDriveDownloadIcon class="size-4" />
						Database
					</Button>
					<Button
						variant={resolverMode === "dictionary" ? "secondary" : "ghost"}
						class="rounded-sm"
						onclick={() => (resolverMode = "dictionary")}
						title="Use a manual candidate dictionary without installing the resolver database."
					>
						<ListChecksIcon class="size-4" />
						Dictionary
					</Button>
				</div>

				<label class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">Hashes to resolve</span>
					<textarea
						bind:value={hashesInput}
						placeholder={"0xB779A091\n3078201489\n-1216765807"}
						title="Enter hashes separated by spaces, commas, or new lines."
						class="min-h-28 w-full resize-y rounded-sm border border-input bg-background px-3 py-3 font-mono text-sm shadow-xs outline-none transition-[color,box-shadow] placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
					></textarea>
				</label>

				{#if resolverMode === "database"}
					<div class="rounded-sm border border-border bg-background/70 p-3">
						<div class="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
							<div>
								<p class="text-sm font-medium text-foreground">Local database lookup</p>
								<p class="mt-1 text-xs text-muted-foreground">Reads only the matching prefix shard for the hashes entered above.</p>
							</div>
							<Button class="rounded-sm" onclick={resolveWithDatabase} disabled={!resolverReady || resolverBusy} title="Resolve entered hashes against the installed local database">
								{#if resolverBusy && !installProgress}
									<Loader2Icon class="size-4 animate-spin" />
								{:else}
									<SearchIcon class="size-4" />
								{/if}
								Resolve Hashes
							</Button>
						</div>
					</div>
				{:else}
					<label class="grid gap-2">
						<div class="flex items-center justify-between gap-2">
							<span class="text-xs font-medium text-muted-foreground">Candidate dictionary</span>
							<Button variant="ghost" size="xs" class="rounded-sm" onclick={useHashNamesAsDictionary} title="Merge the hasher names into this resolver dictionary">
								<SparklesIcon class="size-3" />
								Use Names
							</Button>
						</div>
						<textarea
							bind:value={dictionaryInput}
							placeholder={"adder\npolice\nweapon_pistol"}
							title="The resolver hashes these candidate names and checks them against the entered hash values."
							class="min-h-44 w-full resize-y rounded-sm border border-input bg-background px-3 py-3 font-mono text-sm shadow-xs outline-none transition-[color,box-shadow] placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
						></textarea>
					</label>
				{/if}

				{#if resolverMode === "database" && !resolverStatus?.available}
					<div class="rounded-sm border border-dashed border-border bg-background/60 p-4 text-sm text-muted-foreground">
						Install the optional resolver pack to resolve hashes without a manual dictionary. The hasher and dictionary fallback remain available without the pack.
					</div>
				{:else}
					<ResolverResultList results={resolverResults} />
				{/if}
			</Card.Content>
		</Card.Root>
	</div>

	<div class="grid gap-4 lg:grid-cols-3">
		{#each [
			{ title: "Lowercase Input", description: "FiveM/GTA JOOAT lookups are normally compared against lowercased strings.", icon: ListChecksIcon },
			{ title: "32-bit Output", description: "Every result is shown as hex, unsigned decimal, and signed decimal for config compatibility.", icon: BinaryIcon },
			{ title: "Optional Resolver Pack", description: "Users can keep a tiny hasher-only app or download the larger offline resolver database from GitHub later.", icon: DatabaseZapIcon },
		] as item}
			{@const Icon = item.icon}
			<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-transform duration-300 hover:-translate-y-0.5">
				<div class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"></div>
				<Card.Content class="flex gap-3 p-4">
					<div class="flex size-9 shrink-0 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
						<Icon class="size-4" />
					</div>
					<div>
						<p class="text-sm font-medium text-foreground">{item.title}</p>
						<p class="mt-1 text-sm leading-6 text-muted-foreground">{item.description}</p>
					</div>
				</Card.Content>
			</Card.Root>
		{/each}
	</div>

	{#if copiedLabel}
		<div class="fixed right-6 bottom-6 z-50 rounded-sm border border-border bg-card px-3 py-2 text-sm text-foreground shadow-lg">
			Copied {copiedLabel}
		</div>
	{/if}
</section>
