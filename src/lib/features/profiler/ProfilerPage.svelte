<script lang="ts">
	import AlertCircleIcon from "@lucide/svelte/icons/alert-circle";
	import GaugeIcon from "@lucide/svelte/icons/gauge";
	import LightbulbIcon from "@lucide/svelte/icons/lightbulb";
	import UploadCloudIcon from "@lucide/svelte/icons/upload-cloud";
	import * as Card from "$lib/components/ui/card/index.js";
	import { Button } from "$lib/components/ui/button/index.js";
	import { analyzeProfilerJson, type ProfilerAnalysis } from "./profilerAnalyzer";

	let analysis = $state<ProfilerAnalysis | null>(null);
	let fileName = $state("");
	let error = $state("");
	let dragging = $state(false);

	async function handleFile(file?: File) {
		if (!file) return;
		fileName = file.name || "profiler.json";
		error = "";

		try {
			const text = await file.text();
			const parsed = JSON.parse(text);
			analysis = analyzeProfilerJson(parsed);
		} catch (caught) {
			analysis = null;
			error = caught instanceof Error ? caught.message : String(caught);
		}
	}

	function onDrop(event: DragEvent) {
		event.preventDefault();
		dragging = false;
		void handleFile(event.dataTransfer?.files?.[0]);
	}

	function scorePercent(score: number) {
		if (!analysis?.resources[0]?.score) return 0;
		return Math.max(4, Math.round((score / analysis.resources[0].score) * 100));
	}

	function formatNumber(value: number) {
		return value ? value.toLocaleString(undefined, { maximumFractionDigits: 2 }) : "0";
	}
</script>

<section class="space-y-6">
	<div class="flex flex-col justify-between gap-4 lg:flex-row lg:items-end">
		<div>
			<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">Tools</p>
			<h1 class="mt-2 text-3xl font-semibold tracking-normal text-foreground">Profiler Analyzer</h1>
			<p class="mt-2 max-w-2xl text-sm text-muted-foreground">
				Drop in a FiveM profiler JSON export to rank expensive resources and get optimization hints.
			</p>
		</div>
		<div class="inline-flex items-center gap-2 rounded-sm border border-border bg-card px-3 py-2 text-xs text-muted-foreground">
			<GaugeIcon class="size-3.5" />
			Local analysis
		</div>
	</div>

	<div class="grid gap-4 xl:grid-cols-12">
		<Card.Root class="rounded-md border-border bg-card shadow-sm xl:col-span-5">
			<Card.Header class="border-b border-border pb-4">
				<Card.Title>Upload Profile</Card.Title>
				<Card.Description>Use a `.json` profiler export from `profiler saveJSON`.</Card.Description>
			</Card.Header>
			<Card.Content class="space-y-4">
				<label
					class={[
						"flex min-h-72 cursor-pointer flex-col items-center justify-center gap-4 rounded-sm border border-dashed bg-background/60 px-6 text-center transition-colors",
						dragging ? "border-primary/60 bg-primary/10" : "border-border hover:border-primary/40 hover:bg-accent/30",
					]}
					ondragover={(event) => {
						event.preventDefault();
						dragging = true;
					}}
					ondragleave={() => (dragging = false)}
					ondrop={onDrop}
				>
					<UploadCloudIcon class="size-10 text-muted-foreground" />
					<div>
						<p class="text-sm font-medium text-foreground">Drop profiler.json here</p>
						<p class="mt-1 text-xs text-muted-foreground">or click to browse</p>
					</div>
					<input
						type="file"
						accept=".json,application/json"
						class="sr-only"
						onchange={(event) => void handleFile(event.currentTarget.files?.[0])}
					/>
				</label>

				{#if fileName}
					<p class="rounded-sm border border-border bg-background/70 px-3 py-2 text-xs text-muted-foreground">Loaded {fileName}</p>
				{/if}
				{#if error}
					<div class="flex gap-2 rounded-sm border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
						<AlertCircleIcon class="size-4 shrink-0" />
						<span>{error}</span>
					</div>
				{/if}
			</Card.Content>
		</Card.Root>

		<Card.Root class="rounded-md border-border bg-card shadow-sm xl:col-span-7">
			<Card.Header class="border-b border-border pb-4">
				<Card.Title>Resource Offenders</Card.Title>
				<Card.Description>Ranked by combined total time, self time, average time, and sample count.</Card.Description>
			</Card.Header>
			<Card.Content class="space-y-4">
				{#if analysis?.resources.length}
					<div class="grid gap-3 sm:grid-cols-3">
						<div class="rounded-sm border border-border bg-background/70 p-3">
							<p class="text-xs text-muted-foreground">Resources</p>
							<p class="mt-1 text-lg font-semibold">{analysis.resources.length}</p>
						</div>
						<div class="rounded-sm border border-border bg-background/70 p-3">
							<p class="text-xs text-muted-foreground">Top offender</p>
							<p class="mt-1 truncate text-lg font-semibold">{analysis.resources[0].name}</p>
						</div>
						<div class="rounded-sm border border-border bg-background/70 p-3">
							<p class="text-xs text-muted-foreground">Total score</p>
							<p class="mt-1 text-lg font-semibold">{formatNumber(analysis.totalScore)}</p>
						</div>
					</div>

					<div class="space-y-2">
						{#each analysis.resources.slice(0, 12) as resource, index}
							<div class="rounded-sm border border-border bg-background/70 p-3">
								<div class="flex items-center justify-between gap-3">
									<p class="truncate text-sm font-medium text-foreground">{index + 1}. {resource.name}</p>
									<p class="text-xs text-muted-foreground">score {formatNumber(resource.score)}</p>
								</div>
								<div class="mt-2 h-2 overflow-hidden rounded-sm bg-muted">
									<div class="h-full rounded-sm bg-primary" style={`width: ${scorePercent(resource.score)}%`}></div>
								</div>
								<div class="mt-2 grid gap-2 text-xs text-muted-foreground sm:grid-cols-4">
									<span>total {formatNumber(resource.totalTime)}</span>
									<span>self {formatNumber(resource.selfTime)}</span>
									<span>avg {formatNumber(resource.averageTime)}</span>
									<span>ticks {formatNumber(resource.ticks)}</span>
								</div>
							</div>
						{/each}
					</div>
				{:else}
					<div class="rounded-sm border border-dashed border-border bg-background/60 p-6 text-sm text-muted-foreground">
						Upload a profiler JSON file to see ranked resource timings.
					</div>
				{/if}
			</Card.Content>
		</Card.Root>
	</div>

	{#if analysis}
		<Card.Root class="rounded-md border-border bg-card shadow-sm">
			<Card.Header class="border-b border-border pb-4">
				<div class="flex items-center gap-3">
					<div class="flex size-9 shrink-0 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
						<LightbulbIcon class="size-5" />
					</div>
					<div>
						<Card.Title>Optimization Tips</Card.Title>
						<Card.Description>Suggested places to start investigating hitches.</Card.Description>
					</div>
				</div>
			</Card.Header>
			<Card.Content class="grid gap-3 md:grid-cols-2">
				{#each analysis.tips as tip}
					<div class="rounded-sm border border-border bg-background/70 p-3 text-sm text-muted-foreground">{tip}</div>
				{/each}
			</Card.Content>
		</Card.Root>
	{/if}
</section>
