<script lang="ts">
	import ChevronLeftIcon from "@lucide/svelte/icons/chevron-left";
	import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
	import RefreshCwIcon from "@lucide/svelte/icons/refresh-cw";
	import XIcon from "@lucide/svelte/icons/x";
	import { untrack } from "svelte";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import * as ToggleGroup from "$lib/components/ui/toggle-group/index.js";
	import { getInstallPath } from "$lib/core/paths.svelte";
	import { taskSession } from "$lib/core/tasks.svelte";
	import { getWorkspaceId, workspaceSession } from "$lib/core/workspaces.svelte";
	import { getFxserverTerminal } from "$lib/modules/fxserver";
	import { fxserverSettings } from "./fxserverSettings.svelte";
	import { CONSOLE_ISSUE_LIMIT, CONSOLE_ISSUE_PAGE_SIZE, filterConsoleIssues, issueTime, type ConsoleIssue, type ConsoleIssueIndex, type IssueLevel } from "./consoleIssues";
	import { createConsoleIssueFeed } from "./consoleIssueFeed";
	import ConsoleIssueContext from "./ConsoleIssueContext.svelte";

	let issues = $state.raw<readonly ConsoleIssue[]>([]);
	let index = $state.raw<ConsoleIssueIndex | null>(null);
	let revision = $state(0);
	let buffered = $state(0);
	let busy = $state(false);
	let failed = $state(false);
	let query = $state("");
	let level = $state<IssueLevel | "all">("all");
	let page = $state(0);
	let selectedId = $state<string | null>(null);
	let occurrenceId = $state<number | undefined>(undefined);
	let feed: ReturnType<typeof createConsoleIssueFeed> | undefined;
	const filtered = $derived(filterConsoleIssues(issues, query, level));
	const pageCount = $derived(Math.max(1, Math.ceil(filtered.length / CONSOLE_ISSUE_PAGE_SIZE)));
	const currentPage = $derived(Math.min(page, pageCount - 1));
	const visible = $derived(filtered.slice(currentPage * CONSOLE_ISSUE_PAGE_SIZE, (currentPage + 1) * CONSOLE_ISSUE_PAGE_SIZE));
	const selected = $derived(issues.find((issue) => issue.id === selectedId));
	const context = $derived.by(() => {
		revision;
		return selectedId ? index?.context(selectedId, occurrenceId) ?? null : null;
	});

	function currentScope() {
		return JSON.stringify([getWorkspaceId(), workspaceSession.revision, getInstallPath(), fxserverSettings.txDataPath, fxserverSettings.profile]);
	}

	$effect(() => {
		const scope = currentScope();
		const switching = taskSession.switching;
		untrack(() => {
			issues = []; index = null; buffered = 0; busy = false; failed = false;
			selectedId = null; occurrenceId = undefined; query = ""; level = "all"; page = 0;
		});
		if (switching) return;
		const nextFeed = createConsoleIssueFeed({
			scope,
			isCurrent: () => scope === currentScope() && !taskSession.switching,
			read: getFxserverTerminal,
			publish: (next) => { index = next; issues = next.issues(); buffered = next.size; revision++; },
			onBusy: (value) => { busy = value; },
			onError: (value) => { failed = value; },
			schedule: (callback, delay) => window.setTimeout(callback, delay),
			cancel: (handle) => window.clearTimeout(handle),
			yieldToBrowser: () => new Promise((resolve) => window.setTimeout(resolve, 0)),
		});
		feed = nextFeed;
		untrack(() => { void nextFeed.refresh(); });
		return () => { nextFeed.dispose(); feed = undefined; };
	});

	function select(issue: ConsoleIssue) {
		selectedId = issue.id;
		occurrenceId = issue.lastId;
	}
</script>

<section class="min-w-0 space-y-4" aria-label="Grouped console issues">
	<div class="flex flex-wrap items-center justify-between gap-3">
		<div class="min-w-0">
			<h2 class="text-lg font-semibold">Console Issues</h2>
			<p class="text-xs text-muted-foreground">{buffered} / {CONSOLE_ISSUE_LIMIT} retained lines; {issues.length} {issues.length === 1 ? "group" : "groups"} in this window</p>
		</div>
		<Button variant="outline" size="icon" disabled={busy} onclick={() => void feed?.refresh()} title="Refresh console issues" aria-label="Refresh console issues"><RefreshCwIcon class={busy ? "animate-spin" : undefined} /></Button>
	</div>
	<div class="flex flex-wrap items-center gap-3">
		<Input value={query} oninput={(event) => { query = event.currentTarget.value; page = 0; }} maxlength={256} placeholder="Search issues or resources..." aria-label="Search console issues" class="min-w-0 flex-1 basis-56" />
		<ToggleGroup.Root type="single" value={level} onValueChange={(value) => { if (value === "all" || value === "error" || value === "warn") { level = value; page = 0; } }} aria-label="Issue severity">
			<ToggleGroup.Item value="all">All</ToggleGroup.Item>
			<ToggleGroup.Item value="error">Errors</ToggleGroup.Item>
			<ToggleGroup.Item value="warn">Warnings</ToggleGroup.Item>
		</ToggleGroup.Root>
	</div>
	{#if failed}<p role="status" class="text-sm text-destructive">Console refresh failed. Showing the last observed window.</p>{/if}
	{#if selectedId}
		<section class="min-w-0 space-y-2 border-y border-border py-3" aria-label="Issue context">
			<div class="flex flex-wrap items-center justify-between gap-2">
				<h3 class="min-w-0 break-all text-sm font-medium">{selected?.resource ?? "Expired issue"} / Nearby logs</h3>
				<div class="flex items-center gap-1">
					{#if context}
						<span class="px-2 text-xs text-muted-foreground">{context.position} of {context.count}</span>
						<Button size="icon" variant="ghost" disabled={context.previousId === null} onclick={() => { occurrenceId = context?.previousId ?? undefined; }} title="Previous occurrence" aria-label="Previous occurrence"><ChevronLeftIcon /></Button>
						<Button size="icon" variant="ghost" disabled={context.nextId === null} onclick={() => { occurrenceId = context?.nextId ?? undefined; }} title="Next occurrence" aria-label="Next occurrence"><ChevronRightIcon /></Button>
					{:else if selected}<Button size="sm" variant="outline" onclick={() => { occurrenceId = selected?.lastId; }}>Latest occurrence</Button>{/if}
					<Button size="icon" variant="ghost" onclick={() => { selectedId = null; occurrenceId = undefined; }} title="Close issue context" aria-label="Close issue context"><XIcon /></Button>
				</div>
			</div>
			{#if context}<ConsoleIssueContext {context} />{:else}<p class="text-sm text-muted-foreground">This occurrence is no longer in the retained console window.</p>{/if}
		</section>
	{/if}
	<div class="min-w-0 divide-y divide-border border-y border-border">
		{#each visible as issue (issue.id)}
			<button type="button" class={["grid w-full min-w-0 grid-cols-[minmax(0,1fr)_3.5rem] items-start gap-3 px-2 py-3 text-left transition-colors hover:bg-muted/50 focus-visible:outline-2 focus-visible:outline-ring md:grid-cols-[5rem_8rem_minmax(0,1fr)_3.5rem_9rem]", selectedId === issue.id && "bg-muted/50"]} onclick={() => select(issue)} aria-pressed={selectedId === issue.id} aria-label={`${issue.level === "error" ? "Error" : "Warning"} in ${issue.resource}: ${issue.message.slice(0, 200)}`}>
				<span class={["text-xs font-semibold uppercase", issue.level === "error" ? "text-red-400" : "text-amber-400"]}>{issue.level === "error" ? "Error" : "Warning"}</span>
				<span class="min-w-0 break-all font-mono text-xs text-muted-foreground max-md:col-start-1">{issue.resource}</span>
				<span class="min-w-0 line-clamp-2 break-all font-mono text-xs leading-5 max-md:col-start-1">{issue.message}</span>
				<span class="text-right font-mono text-sm font-semibold tabular-nums max-md:col-start-2 max-md:row-start-1" aria-label={`${issue.count} occurrences`}>{issue.count}</span>
				<span class="space-y-1 text-[11px] text-muted-foreground tabular-nums max-md:col-span-2"><span class="block">First <time datetime={issue.first || undefined} title={issue.first || undefined}>{issueTime(issue.first)}</time></span><span class="block">Last <time datetime={issue.last || undefined} title={issue.last || undefined}>{issueTime(issue.last)}</time></span></span>
			</button>
		{:else}<p class="py-12 text-center text-sm text-muted-foreground">{busy && !buffered ? "Loading console..." : issues.length ? "No matching issues." : "No errors or warnings in the retained console window."}</p>{/each}
	</div>
	<div class="flex flex-wrap items-center justify-between gap-2 text-xs text-muted-foreground">
		<span>{visible.length} of {filtered.length} groups</span>
		<div class="flex items-center gap-2"><Button size="icon" variant="ghost" disabled={currentPage === 0} onclick={() => { page = currentPage - 1; }} title="Previous issue page" aria-label="Previous issue page"><ChevronLeftIcon /></Button><span>Page {currentPage + 1} / {pageCount}</span><Button size="icon" variant="ghost" disabled={currentPage + 1 >= pageCount} onclick={() => { page = currentPage + 1; }} title="Next issue page" aria-label="Next issue page"><ChevronRightIcon /></Button></div>
	</div>
</section>
