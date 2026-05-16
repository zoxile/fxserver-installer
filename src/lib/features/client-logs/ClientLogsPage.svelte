<script lang="ts">
	import AlertCircleIcon from "@lucide/svelte/icons/alert-circle";
	import CheckCircle2Icon from "@lucide/svelte/icons/check-circle-2";
	import ClipboardIcon from "@lucide/svelte/icons/clipboard";
	import FileTextIcon from "@lucide/svelte/icons/file-text";
	import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import ScrollTextIcon from "@lucide/svelte/icons/scroll-text";
	import { onDestroy, onMount } from "svelte";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Checkbox } from "$lib/components/ui/checkbox/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import * as Select from "$lib/components/ui/select/index.js";
	import { log } from "$lib/core/logger.svelte";
	import { chooseFolder } from "$lib/core/selectFolder";
	import { readClientLogs, type ClientLogResult } from "$lib/modules/clientLogs";

	type LogLevel = "all" | "debug" | "info" | "warn" | "error";

	interface ParsedClientLine {
		id: string;
		tick: string;
		process: string;
		thread: string;
		level: Exclude<LogLevel, "all">;
		message: string;
		raw: string;
	}

	const defaultDirectory = "C:\\Users\\Zox\\AppData\\Local\\FiveM\\FiveM.app\\logs";
	const levels: LogLevel[] = ["all", "debug", "info", "warn", "error"];

	let directory = $state(defaultDirectory);
	let selectedFile = $state("");
	let maxLines = $state("700");
	let query = $state("");
	let level = $state<LogLevel>("all");
	let autoRefresh = $state(true);
	let result = $state<ClientLogResult | null>(null);
	let busy = $state(false);
	let notice = $state("");
	let noticeLevel = $state<"success" | "error">("success");
	let refreshTimer: number | undefined;

	const fileOptions = $derived((result?.files ?? []).map((file) => ({ value: file.name, label: `${file.name} (${formatBytes(file.size)})` })));
	const entries = $derived(parseLines(result?.content ?? ""));
	const filteredEntries = $derived(
		entries.filter((entry) => {
			const haystack = `${entry.level} ${entry.process} ${entry.thread} ${entry.message} ${entry.raw}`.toLowerCase();
			return (level === "all" || entry.level === level) && (!query.trim() || haystack.includes(query.trim().toLowerCase()));
		}),
	);
	const pathPreview = $derived(result?.path ?? `${directory}\\${selectedFile || "latest log file"}`);

	onMount(() => {
		void refresh(false);
		refreshTimer = window.setInterval(() => {
			if (autoRefresh && !busy) {
				void refresh(false);
			}
		}, 1500);
	});

	onDestroy(() => {
		if (refreshTimer) window.clearInterval(refreshTimer);
	});

	async function refresh(showNotice = true) {
		busy = true;
		if (showNotice) notice = "";

		try {
			const nextResult = await readClientLogs({
				directory: directory.trim() || null,
				fileName: selectedFile || null,
				maxLines: Number.parseInt(maxLines, 10) || 700,
			});
			result = nextResult;
			directory = nextResult.directory || directory;
			selectedFile = nextResult.selectedFile ?? "";

			if (showNotice) {
				notice = nextResult.selectedFile ? `${nextResult.selectedFile} refreshed.` : "No FiveM client log files were found.";
				noticeLevel = nextResult.selectedFile ? "success" : "error";
			}
		} catch (error) {
			if (showNotice) {
				notice = error instanceof Error ? error.message : String(error);
				noticeLevel = "error";
			}
			log("FiveM client log viewer could not refresh logs.", {
				level: "error",
				scope: "client.logs",
				detail: error instanceof Error ? error.message : String(error),
			});
		} finally {
			busy = false;
		}
	}

	async function chooseLogFolder() {
		notice = "";
		const selectedPath = await chooseFolder();
		if (!selectedPath) return;

		directory = selectedPath;
		selectedFile = "";
		await refresh();
	}

	async function copyPath() {
		const path = result?.path ?? pathPreview;
		await navigator.clipboard.writeText(path);
		notice = "Log path copied.";
		noticeLevel = "success";
	}

	function parseLines(content: string): ParsedClientLine[] {
		return content
			.split("\n")
			.filter((line) => line.trim())
			.map((line, index) => {
				const match = line.match(/^\[\s*(?<tick>\d+)\]\s+\[\s*(?<process>[^\]]+?)\s*\]\s+(?<thread>[^/]+?)\/\s*(?<message>.*)$/u);
				const rawMessage = match?.groups?.message ?? line;
				const message = stripFivemColors(rawMessage);

				return {
					id: `${index}-${line.length}-${rawMessage.slice(0, 24)}`,
					tick: match?.groups?.tick ?? "",
					process: match?.groups?.process?.trim() ?? "client",
					thread: match?.groups?.thread?.trim() ?? "-",
					level: classifyLine(rawMessage),
					message,
					raw: line,
				};
			});
	}

	function stripFivemColors(value: string) {
		return value.replace(/\^[0-9]/g, "").trim();
	}

	function classifyLine(line: string): ParsedClientLine["level"] {
		const lower = stripFivemColors(line).toLowerCase();
		if (lower.includes("script error") || lower.includes("error") || lower.includes("failed") || lower.includes("invalid")) return "error";
		if (lower.includes("[warn]") || lower.includes("warning") || lower.includes("warn") || lower.includes("could not")) return "warn";
		if (lower.includes("[debug]") || lower.includes("debug")) return "debug";
		return "info";
	}

	function levelClass(entryLevel: ParsedClientLine["level"]) {
		return {
			debug: "border-muted bg-muted/50 text-muted-foreground",
			info: "border-sky-400/30 bg-sky-400/10 text-sky-200",
			warn: "border-amber-400/30 bg-amber-400/10 text-amber-200",
			error: "border-red-400/30 bg-red-400/10 text-red-200",
		}[entryLevel];
	}

	function formatBytes(bytes: number) {
		if (bytes < 1024) return `${bytes} B`;
		if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
		return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
	}
</script>

<section class="space-y-6">
	<div class="flex flex-col justify-between gap-4 lg:flex-row lg:items-end">
		<div>
			<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">Tools</p>
			<h1 class="mt-2 text-3xl font-semibold tracking-normal text-foreground">Client Logs</h1>
			<p class="mt-2 max-w-2xl text-sm text-muted-foreground">Inspect FiveM client logs from the local FiveM.app logs folder as they update.</p>
		</div>
		<div class="inline-flex items-center gap-2 rounded-sm border border-border bg-card px-3 py-2 text-xs text-muted-foreground">
			<ScrollTextIcon class="size-3.5" />
			{filteredEntries.length} visible entries
		</div>
	</div>

	<Card.Root class="overflow-hidden rounded-sm border-border bg-card shadow-sm">
		<Card.Header class="border-b border-border pb-4">
			<div class="flex flex-col gap-4 xl:flex-row xl:items-end xl:justify-between">
				<div class="min-w-0">
					<Card.Title>FiveM Client Log File</Card.Title>
					<Card.Description class="mt-1 truncate font-mono text-xs">{pathPreview}</Card.Description>
				</div>
				<div class="flex flex-wrap gap-2">
					<Button variant="outline" onclick={copyPath} disabled={!result?.path} title="Copy the selected client log path">
						<ClipboardIcon />
						Copy Path
					</Button>
					<Button variant="outline" onclick={() => refresh()} disabled={busy} title="Reload the selected FiveM client log from disk">
						<RefreshCwIcon class={busy ? "animate-spin" : undefined} />
						Refresh
					</Button>
				</div>
			</div>
		</Card.Header>

		<Card.Content class="space-y-4">
			<div class="grid gap-3 xl:grid-cols-[minmax(0,1fr)_minmax(0,0.65fr)_8rem]">
				<label class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">FiveM Logs Folder</span>
					<div class="grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto]">
						<Input bind:value={directory} placeholder={defaultDirectory} title="FiveM.app logs folder." class="rounded-sm font-mono text-xs" />
						<Button variant="outline" onclick={chooseLogFolder} title="Browse for the FiveM client logs folder">
							<FolderOpenIcon />
							Browse
						</Button>
					</div>
				</label>
				<label class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">Log File</span>
					<Select.Root bind:value={selectedFile} type="single" items={fileOptions}>
						<Select.Trigger title="Choose a FiveM client log file" class="w-full rounded-sm font-mono text-xs">
							<FileTextIcon class="size-3.5" />
							{selectedFile || "Newest log file"}
						</Select.Trigger>
						<Select.Content class="rounded-sm">
							{#if fileOptions.length}
								{#each fileOptions as option}
									<Select.Item value={option.value} label={option.label}>
										{option.label}
									</Select.Item>
								{/each}
							{:else}
								<Select.Item value="" label="No log files detected" disabled>No log files detected</Select.Item>
							{/if}
						</Select.Content>
					</Select.Root>
				</label>
				<label class="grid gap-2">
					<span class="text-xs font-medium text-muted-foreground">Lines</span>
					<Input bind:value={maxLines} type="number" min="50" max="5000" placeholder="700" title="Number of latest log lines to load." class="rounded-sm font-mono text-xs" />
				</label>
			</div>

			<div class="grid gap-3 lg:grid-cols-[minmax(0,1fr)_auto_auto] lg:items-center">
				<Input bind:value={query} placeholder="Filter by message, process, thread, level, or raw line..." title="Filter FiveM client log entries." class="rounded-sm" />
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
				<label class="flex h-9 items-center gap-2 rounded-sm border border-border px-3 text-xs text-muted-foreground">
					<Checkbox bind:checked={autoRefresh} title="Refresh the selected log automatically" />
					Auto refresh
				</label>
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
							<article class="grid gap-3 px-4 py-3 text-sm lg:grid-cols-[6rem_6rem_minmax(0,10rem)_minmax(0,10rem)_minmax(0,1fr)] lg:items-start">
								<time class="font-mono text-xs text-muted-foreground">{entry.tick || "-"}</time>
								<span class={`w-fit rounded-xs border px-2 py-0.5 text-xs font-medium uppercase ${levelClass(entry.level)}`}>{entry.level}</span>
								<span class="truncate font-mono text-xs text-muted-foreground">{entry.process}</span>
								<span class="truncate font-mono text-xs text-muted-foreground">{entry.thread}</span>
								<p class="min-w-0 wrap-break-word text-foreground">{entry.message}</p>
							</article>
						{/each}
					</div>
				{:else}
					<div class="flex min-h-48 items-center justify-center px-4 text-center text-sm text-muted-foreground">Load a client log file or adjust the filters to see entries.</div>
				{/if}
			</div>
		</Card.Content>
	</Card.Root>
</section>
