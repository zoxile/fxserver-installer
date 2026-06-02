<script lang="ts">
	import AlertCircleIcon from "@lucide/svelte/icons/alert-circle";
	import CheckCircle2Icon from "@lucide/svelte/icons/check-circle-2";
	import FileTextIcon from "@lucide/svelte/icons/file-text";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import ScrollTextIcon from "@lucide/svelte/icons/scroll-text";
	import { onMount } from "svelte";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import * as Select from "$lib/components/ui/select/index.js";
	import { log } from "$lib/core/logger.svelte";
	import { chooseFolder } from "$lib/core/selectFolder";
	import { readTxDataLog, type TxDataLogResult } from "$lib/modules/fxserver";
	import { fxserverSettings, loadFxserverSettings, refreshTxDataProfiles, setServerProfile, setTxDataPath } from "./fxserverSettings.svelte";

	type LogName = "fxserver.log" | "admin.log" | "server.log";
	type LogLevel = "all" | "info" | "success" | "warn" | "error";

	interface ParsedLine {
		id: string;
		source: string;
		message: string;
		level: Exclude<LogLevel, "all">;
		time: string;
		raw: string;
	}

	const logNames: LogName[] = ["fxserver.log", "admin.log", "server.log"];
	const levels: LogLevel[] = ["all", "info", "success", "warn", "error"];

	let dataPath = $state("");
	let profile = $state("");
	let logName = $state<LogName>("fxserver.log");
	let maxLines = $state("500");
	let query = $state("");
	let level = $state<LogLevel>("all");
	let result = $state<TxDataLogResult | null>(null);
	let busy = $state(false);
	let notice = $state("");
	let noticeLevel = $state<"success" | "error">("success");
	let storageReady = false;
	let previousProfile: string | undefined;

	const entries = $derived(parseLines(result?.content ?? "", logName));
	const filteredEntries = $derived(
		entries.filter((entry) => {
			const haystack = `${entry.level} ${entry.source} ${entry.message} ${entry.raw}`.toLowerCase();
			return (level === "all" || entry.level === level) && (!query.trim() || haystack.includes(query.trim().toLowerCase()));
		}),
	);
	const pathPreview = $derived(dataPath.trim() ? `${dataPath.trim()}${profile.trim() ? `\\${profile.trim()}` : ""}\\logs\\${logName}` : "Set TXHOST_DATA_PATH to view FXServer logs.");
	const profileOptions = $derived([
		...(fxserverSettings.hasRootLogs ? [{ value: "", label: "Root logs folder" }] : []),
		...fxserverSettings.profiles.map((profileName) => ({ value: profileName, label: profileName })),
	]);

	onMount(() => {
		loadFxserverSettings();
		dataPath = fxserverSettings.txDataPath;
		profile = fxserverSettings.profile;
		storageReady = true;
		void (async () => {
			await refreshTxDataProfiles();
			dataPath = fxserverSettings.txDataPath;
			profile = fxserverSettings.profile;
			if (dataPath.trim()) {
				await refresh();
			}
		})();
	});

	$effect(() => {
		if (!storageReady) return;
		const profileChanged = previousProfile !== undefined && previousProfile !== profile;
		previousProfile = profile;

		setTxDataPath(dataPath);
		setServerProfile(profile);

		if (profileChanged && result && dataPath.trim() && !busy) {
			void refresh();
		}
	});

	async function refresh() {
		busy = true;
		notice = "";
		setTxDataPath(dataPath);
		setServerProfile(profile);

		try {
			result = await readTxDataLog({
				dataPath: dataPath.trim(),
				profile: profile.trim() || null,
				logName,
				maxLines: Number.parseInt(maxLines, 10) || 500,
			});
			notice = `${logName} refreshed.`;
			noticeLevel = "success";
			log("FXServer log viewer refreshed a txData log.", { level: "debug", scope: "fxserver.logs-page", detail: result.path });
		} catch (error) {
			notice = error instanceof Error ? error.message : String(error);
			noticeLevel = "error";
			log("FXServer log viewer could not refresh txData logs.", { level: "error", scope: "fxserver.logs-page", detail: notice });
		} finally {
			busy = false;
		}
	}

	async function chooseTxDataFolder() {
		notice = "";

		const selectedPath = await chooseFolder();
		if (!selectedPath) return;

		dataPath = selectedPath;
		setTxDataPath(selectedPath);
		await refreshTxDataProfiles();
	}

	async function handleTxDataChange(event: Event) {
		dataPath = (event.currentTarget as HTMLInputElement).value;
		setTxDataPath(dataPath);
		await refreshTxDataProfiles();
	}

	function parseLines(content: string, currentLog: LogName): ParsedLine[] {
		return content
			.split("\n")
			.filter((line) => line.trim())
			.map((line, index) => {
				const txAdmin = line.match(/^\[(?<time>[^\]]+)\]\s*(?<message>.*)$/);
				const fxserver = line.match(/^\[\s*(?<source>[^\]]+?)\s*\]\s*(?<message>.*)$/);
				const source = currentLog === "fxserver.log" ? (fxserver?.groups?.source ?? "fxserver") : currentLog.replace(".log", "");
				const message = currentLog === "fxserver.log" ? (fxserver?.groups?.message ?? line) : (txAdmin?.groups?.message ?? line);
				const time = currentLog === "fxserver.log" ? "" : (txAdmin?.groups?.time ?? "");

				return {
					id: `${currentLog}-${index}-${line.length}`,
					source,
					message,
					level: classifyLine(line),
					time,
					raw: line,
				};
			});
	}

	function classifyLine(line: string): ParsedLine["level"] {
		const lower = line.toLowerCase();
		if (lower.includes("error") || lower.includes("failed") || lower.includes("could not")) return "error";
		if (lower.includes("warning") || lower.includes("warn")) return "warn";
		if (lower.includes("started") || lower.includes("established") || lower.includes("authenticated") || lower.includes("logged in")) return "success";
		return "info";
	}

	function levelClass(entryLevel: ParsedLine["level"]) {
		return {
			info: "border-sky-400/30 bg-sky-400/10 text-sky-200",
			success: "border-emerald-400/30 bg-emerald-400/10 text-emerald-200",
			warn: "border-amber-400/30 bg-amber-400/10 text-amber-200",
			error: "border-red-400/30 bg-red-400/10 text-red-200",
		}[entryLevel];
	}
</script>

<section class="space-y-6">
	<div class="flex flex-col justify-between gap-4 lg:flex-row lg:items-end">
		<div>
			<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">FXServer</p>
			<h1 class="mt-2 text-3xl font-semibold tracking-normal text-foreground">Server Logs</h1>
			<p class="mt-2 max-w-2xl text-sm text-muted-foreground">Inspect txData logs for FXServer, txAdmin admin actions, and txAdmin server output.</p>
		</div>
		<div class="inline-flex items-center gap-2 rounded-sm border border-border bg-card px-3 py-2 text-xs text-muted-foreground">
			<ScrollTextIcon class="size-3.5" />
			{filteredEntries.length} visible entries
		</div>
	</div>

	<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-shadow duration-500 ease-[cubic-bezier(0.22,1,0.36,1)]">
		<div
			class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"
		></div>
		<Card.Header class="border-b border-border pb-4">
			<div class="flex flex-col gap-4 xl:flex-row xl:items-end xl:justify-between">
				<div class="min-w-0">
					<Card.Title>txData Log File</Card.Title>
					<Card.Description class="mt-1 truncate font-mono text-xs">{result?.path ?? pathPreview}</Card.Description>
				</div>
				<div class="flex flex-wrap gap-2">
					{#each logNames as item}
						<Button variant={logName === item ? "default" : "outline"} class="rounded-sm" onclick={() => (logName = item)} title={`Open ${item}`}>
							<FileTextIcon />
							{item}
						</Button>
					{/each}
					<Button variant="outline" onclick={refresh} disabled={busy || !dataPath.trim()} title="Reload the selected FXServer log from disk">
						<RefreshCwIcon class={busy ? "animate-spin" : undefined} />
						Refresh
					</Button>
				</div>
			</div>
		</Card.Header>

		<Card.Content class="space-y-4">
			<div class="grid gap-3 lg:grid-cols-[minmax(0,1fr)_minmax(0,0.55fr)_8rem]">
				<label class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">txData Path</span>
					<div class="grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto]">
						<Input
							bind:value={dataPath}
							onchange={handleTxDataChange}
							placeholder="C:\FiveM\txData"
							title="TXHOST_DATA_PATH folder containing profile folders and logs."
							class="rounded-sm font-mono text-xs"
						/>
						<Button variant="outline" onclick={chooseTxDataFolder} title="Browse for the txData folder">
							<FolderOpenIcon />
							Browse
						</Button>
					</div>
				</label>
				<label class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">Profile Folder</span>
					<Select.Root bind:value={profile} type="single" items={profileOptions}>
						<Select.Trigger title="Choose the txData profile folder" class="w-full rounded-sm font-mono text-xs">
							{profile || (fxserverSettings.hasRootLogs ? "Root logs folder" : "Choose profile")}
						</Select.Trigger>
						<Select.Content class="rounded-sm">
							{#if profileOptions.length}
								{#each profileOptions as option}
									<Select.Item value={option.value} label={option.label}>
										{option.label}
									</Select.Item>
								{/each}
							{:else}
								<Select.Item value="" label="No profiles detected" disabled>No profiles detected</Select.Item>
							{/if}
						</Select.Content>
					</Select.Root>
				</label>
				<label class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">Lines</span>
					<Input bind:value={maxLines} type="number" min="50" max="5000" placeholder="500" title="Number of latest log lines to load." class="rounded-sm font-mono text-xs" />
				</label>
			</div>

			{#if fxserverSettings.profileError}
				<div class="rounded-sm border border-red-400/30 bg-red-400/10 px-3 py-2 text-xs text-red-100">
					{fxserverSettings.profileError}
				</div>
			{/if}

			<div class="grid gap-3 lg:grid-cols-[minmax(0,1fr)_auto]">
				<Input bind:value={query} placeholder="Filter by message, source, level, or raw line..." title="Filter FXServer log entries." class="rounded-sm" />
				<div class="flex flex-wrap gap-2">
					{#each levels as item}
						<Button
							variant={level === item ? "default" : "outline"}
							size="sm"
							class="rounded-sm capitalize"
							onclick={() => (level = item)}
							title={`Show ${item === "all" ? "all log levels" : `${item} logs`}`}
						>
							{item}
						</Button>
					{/each}
				</div>
			</div>

			{#if notice}
				<div class={`rounded-sm border px-3 py-2 text-xs ${noticeLevel === "success" ? "border-emerald-400/30 bg-emerald-400/10 text-emerald-100" : "border-red-400/30 bg-red-400/10 text-red-100"}`}>
					<div class="flex items-start gap-2">
						{#if noticeLevel === "success"}
							<CheckCircle2Icon class="mt-0.5 size-3.5 shrink-0" />
						{:else}
							<AlertCircleIcon class="mt-0.5 size-3.5 shrink-0" />
						{/if}
						<p>{notice}</p>
					</div>
				</div>
			{/if}

			<div class="max-h-160 overflow-auto rounded-sm border border-border bg-background/60">
				{#if filteredEntries.length}
					<div class="divide-y divide-border/70">
						{#each filteredEntries as entry (entry.id)}
							<article class="grid gap-3 px-4 py-3 text-sm lg:grid-cols-[7rem_7rem_minmax(0,12rem)_minmax(0,1fr)] lg:items-start">
								<time class="font-mono text-xs text-muted-foreground">{entry.time || "-"}</time>
								<span class={`w-fit rounded-xs border px-2 py-0.5 text-xs font-medium uppercase ${levelClass(entry.level)}`}>{entry.level}</span>
								<span class="truncate font-mono text-xs text-muted-foreground">{entry.source}</span>
								<p class="min-w-0 wrap-break-word text-foreground">{entry.message}</p>
							</article>
						{/each}
					</div>
				{:else}
					<div class="flex min-h-48 items-center justify-center px-4 text-center text-sm text-muted-foreground">Load a log file or adjust the filters to see entries.</div>
				{/if}
			</div>
		</Card.Content>
	</Card.Root>
</section>
