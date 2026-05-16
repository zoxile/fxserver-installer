<script lang="ts">
	import ClipboardIcon from "@lucide/svelte/icons/clipboard";
	import EraserIcon from "@lucide/svelte/icons/eraser";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import ScrollTextIcon from "@lucide/svelte/icons/scroll-text";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import { clearLogs, log, logFilePath, logs, refreshLogs, type AppLogEntry, type LogLevel } from "$lib/core/logger.svelte";

	let query = $state("");
	let level = $state<LogLevel | "all">("all");
	let busy = $state(false);
	let notice = $state("");

	const levels: Array<LogLevel | "all"> = ["all", "debug", "info", "success", "warn", "error"];
	const filteredLogs = $derived(
		logs
			.filter((entry) => {
				const haystack = `${entry.level} ${entry.scope} ${entry.message} ${entry.detail ?? ""}`.toLowerCase();
				return (level === "all" || entry.level === level) && (!query.trim() || haystack.includes(query.trim().toLowerCase()));
			})
			.slice()
			.reverse(),
	);

	async function refresh() {
		busy = true;
		notice = "";

		try {
			await refreshLogs();
			notice = "Log file refreshed.";
			log("Log viewer refreshed persisted logs.", { level: "debug", scope: "logs.viewer" });
		} catch (error) {
			notice = error instanceof Error ? error.message : String(error);
			log("Log viewer could not refresh logs.", { level: "error", scope: "logs.viewer", detail: notice });
		} finally {
			busy = false;
		}
	}

	async function clear() {
		busy = true;
		notice = "";

		try {
			await clearLogs();
			notice = "Log file cleared.";
		} catch (error) {
			notice = error instanceof Error ? error.message : String(error);
			log("Log viewer could not clear logs.", { level: "error", scope: "logs.viewer", detail: notice });
		} finally {
			busy = false;
		}
	}

	async function copyPath() {
		await navigator.clipboard.writeText(logFilePath.value);
		notice = "Log path copied.";
		log("Application log path copied.", { level: "debug", scope: "logs.viewer", detail: logFilePath.value });
	}

	function formatTime(entry: AppLogEntry) {
		return new Date(entry.timestamp).toLocaleString(undefined, {
			month: "short",
			day: "2-digit",
			hour: "2-digit",
			minute: "2-digit",
			second: "2-digit",
		});
	}

	function levelClass(entryLevel: LogLevel) {
		return {
			debug: "border-muted bg-muted/50 text-muted-foreground",
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
			<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">Tools</p>
			<h1 class="mt-2 text-3xl font-semibold tracking-normal text-foreground">Application Logs</h1>
			<p class="mt-2 max-w-2xl text-sm text-muted-foreground">Inspect recent app actions, errors, and desktop operations from the persisted local log file.</p>
		</div>
		<div class="inline-flex items-center gap-2 rounded-sm border border-border bg-card px-3 py-2 text-xs text-muted-foreground">
			<ScrollTextIcon class="size-3.5" />
			{logs.length} visible entries
		</div>
	</div>

	<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-transform duration-300 hover:-translate-y-0.5">
		<div
			class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"
		></div>
		<Card.Header class="border-b border-border pb-4">
			<div class="flex flex-col gap-4 xl:flex-row xl:items-end xl:justify-between">
				<div class="min-w-0">
					<Card.Title>Log File</Card.Title>
					<Card.Description class="mt-1 truncate font-mono text-xs">{logFilePath.value}</Card.Description>
				</div>
				<div class="flex flex-wrap gap-2">
					<Button variant="outline" onclick={copyPath} title="Copy the persisted log file path">
						<ClipboardIcon />
						Copy Path
					</Button>
					<Button variant="outline" onclick={refresh} disabled={busy} title="Reload logs from disk">
						<RefreshCwIcon class={busy ? "animate-spin" : undefined} />
						Refresh
					</Button>
					<Button variant="outline" onclick={clear} disabled={busy} title="Clear the persisted log file and current log list">
						<EraserIcon />
						Clear
					</Button>
				</div>
			</div>
		</Card.Header>
		<Card.Content class="space-y-4">
			<div class="grid gap-3 lg:grid-cols-[minmax(0,1fr)_auto]">
				<Input bind:value={query} placeholder="Filter by message, scope, detail, or level..." title="Filter application log entries." class="rounded-sm" />
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
				<p class="rounded-sm border border-border bg-background/70 px-3 py-2 text-xs text-muted-foreground">{notice}</p>
			{/if}

			<div class="max-h-144 overflow-auto rounded-sm border border-border bg-background/60">
				{#if filteredLogs.length}
					<div class="divide-y divide-border/70">
						{#each filteredLogs as entry, index (`${entry.id}-${index}`)}
							<article class="grid gap-3 px-4 py-3 text-sm lg:grid-cols-[9rem_7rem_minmax(0,10rem)_minmax(0,1fr)] lg:items-start">
								<time class="font-mono text-xs text-muted-foreground">{formatTime(entry)}</time>
								<span class={`w-fit rounded-xs border px-2 py-0.5 text-xs font-medium uppercase ${levelClass(entry.level)}`}>{entry.level}</span>
								<span class="truncate font-mono text-xs text-muted-foreground">{entry.scope}</span>
								<div class="min-w-0">
									<p class="text-foreground">{entry.message}</p>
									{#if entry.detail}
										<p class="mt-1 break-words font-mono text-xs text-muted-foreground">{entry.detail}</p>
									{/if}
								</div>
							</article>
						{/each}
					</div>
				{:else}
					<div class="flex min-h-48 items-center justify-center px-4 text-center text-sm text-muted-foreground">No log entries match the current filters.</div>
				{/if}
			</div>
		</Card.Content>
	</Card.Root>
</section>
