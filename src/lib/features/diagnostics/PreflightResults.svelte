<script lang="ts">
	import AlertTriangleIcon from "@lucide/svelte/icons/alert-triangle";
	import CheckCircle2Icon from "@lucide/svelte/icons/check-circle-2";
	import CircleXIcon from "@lucide/svelte/icons/circle-x";
	import InfoIcon from "@lucide/svelte/icons/info";
	import ChevronLeftIcon from "@lucide/svelte/icons/chevron-left";
	import ChevronRightIcon from "@lucide/svelte/icons/chevron-right";
	import SearchIcon from "@lucide/svelte/icons/search";
	import ArrowUpRightIcon from "@lucide/svelte/icons/arrow-up-right";
	import FilePenLineIcon from "@lucide/svelte/icons/file-pen-line";
	import { Button } from "$lib/components/ui/button/index.js";
	import { Input } from "$lib/components/ui/input/index.js";
	import * as Select from "$lib/components/ui/select/index.js";
	import type { DiagnosticSeverity, PreflightReport } from "$lib/modules/diagnostics";
	import type { PageId } from "$lib/navigation";

	let { report, onNavigate, onReviewPatch, disabled = false }: {
		report: PreflightReport; onNavigate?: (page: PageId) => void; onReviewPatch?: () => void; disabled?: boolean;
	} = $props();
	let search = $state("");
	let severity = $state("all");
	let page = $state(0);
	const pageSize = 40;
	const labels: Record<string, string> = { all: "All checks", error: "Blocking errors", warning: "Warnings", info: "Information", pass: "Passed" };
	const icons = { error: CircleXIcon, warning: AlertTriangleIcon, info: InfoIcon, pass: CheckCircle2Icon };
	const colors: Record<DiagnosticSeverity, string> = { error: "text-red-400", warning: "text-amber-400", info: "text-sky-400", pass: "text-emerald-400" };
	const filtered = $derived(report.checks.filter((check) =>
		(severity === "all" || severity === check.severity) &&
		(!search.trim() || [check.title, check.detail, check.resource, check.file, check.category].join(" ").toLowerCase().includes(search.trim().toLowerCase())),
	));
	const pages = $derived(Math.max(1, Math.ceil(filtered.length / pageSize)));
	const currentPage = $derived(Math.min(page, pages - 1));
	const visible = $derived(filtered.slice(currentPage * pageSize, (currentPage + 1) * pageSize));
	$effect(() => { report; search; severity; page = 0; });
</script>

<div class="space-y-4">
	<div class="flex flex-wrap items-center gap-x-5 gap-y-2 text-sm">
		<span class="font-medium" class:text-red-400={report.blocking} class:text-emerald-400={!report.blocking}>
			{report.blocking ? `${report.errorCount} blocking error${report.errorCount === 1 ? "" : "s"}` : "No blocking errors"}
		</span>
		<span class="text-muted-foreground">{report.warningCount} warnings</span>
		<span class="text-muted-foreground">{report.resourceCount} resources</span>
		<span class="text-muted-foreground">{report.configCount} cfg files</span>
	</div>
	<div class="flex flex-col gap-2 sm:flex-row">
		<div class="relative min-w-0 flex-1">
			<SearchIcon class="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
			<Input class="pl-9" bind:value={search} placeholder="Filter checks" aria-label="Filter diagnostic checks" />
		</div>
		<Select.Root type="single" bind:value={severity}>
			<Select.Trigger class="w-full sm:w-44" aria-label="Check severity">{labels[severity]}</Select.Trigger>
			<Select.Content>{#each Object.entries(labels) as [value, label]}<Select.Item {value}>{label}</Select.Item>{/each}</Select.Content>
		</Select.Root>
	</div>
	<div class="divide-y divide-border border-y border-border" aria-live="polite">
		{#each visible as check}
			{@const Icon = icons[check.severity]}
			<div class="flex items-start gap-3 py-3">
				<Icon class={`mt-0.5 size-4 shrink-0 ${colors[check.severity]}`} aria-label={labels[check.severity]} />
				<div class="min-w-0 flex-1 space-y-1">
					<div class="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
						<p class="text-sm font-medium wrap-anywhere">{check.title}</p>
						<span class="text-xs text-muted-foreground">{check.category}</span>
					</div>
					<p class="text-xs leading-5 wrap-anywhere text-muted-foreground">{check.detail}</p>
					{#if check.file || check.resource}
						<p class="font-mono text-xs wrap-anywhere text-muted-foreground">{check.file ?? check.resource}{check.line ? `:${check.line}` : ""}</p>
					{/if}
					{#if check.guidance}
						<details class="pt-1">
							<summary class="cursor-pointer text-xs font-medium">Recommended next steps</summary>
							<ol class="mt-2 list-decimal space-y-1 pl-5 text-xs leading-5 text-muted-foreground">
								{#each check.guidance.steps as step}<li class="wrap-anywhere">{step}</li>{/each}
							</ol>
						</details>
						<div class="flex flex-wrap gap-2 pt-2">
							{#if onNavigate}<Button size="sm" variant="outline" {disabled} onclick={() => onNavigate?.(check.guidance!.page)}><ArrowUpRightIcon />{check.guidance.label}</Button>{/if}
							{#if check.guidance.patchAvailable && onReviewPatch}<Button size="sm" variant="outline" {disabled} onclick={onReviewPatch}><FilePenLineIcon />Review rconlog patch</Button>{/if}
						</div>
					{/if}
				</div>
			</div>
		{:else}<p class="py-8 text-center text-sm text-muted-foreground">No matching checks.</p>{/each}
	</div>
	{#if pages > 1}
		<div class="flex items-center justify-end gap-3">
			<span class="text-xs text-muted-foreground">Page {currentPage + 1} of {pages}</span>
			<Button variant="outline" size="icon-sm" title="Previous checks" aria-label="Previous checks" disabled={currentPage === 0} onclick={() => page = currentPage - 1}><ChevronLeftIcon /></Button>
			<Button variant="outline" size="icon-sm" title="Next checks" aria-label="Next checks" disabled={currentPage >= pages - 1} onclick={() => page = currentPage + 1}><ChevronRightIcon /></Button>
		</div>
	{/if}
</div>
