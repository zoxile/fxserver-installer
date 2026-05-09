<script lang="ts">
	import AlertCircleIcon from "@lucide/svelte/icons/alert-circle";
	import BarChart3Icon from "@lucide/svelte/icons/bar-chart-3";
	import FileJsonIcon from "@lucide/svelte/icons/file-json";
	import GaugeIcon from "@lucide/svelte/icons/gauge";
	import LightbulbIcon from "@lucide/svelte/icons/lightbulb";
	import TerminalIcon from "@lucide/svelte/icons/terminal";
	import UploadCloudIcon from "@lucide/svelte/icons/upload-cloud";
	import * as Card from "$lib/components/ui/card/index.js";
	import { analyzeProfilerJson, type ProfilerAnalysis, type ResourceState } from "./profilerAnalyzer";

	let analysis = $state<ProfilerAnalysis | null>(null);
	let fileName = $state("");
	let error = $state("");
	let dragging = $state(false);

	const stateLabels: Record<ResourceState, string> = {
		excellent: "Excellent",
		good: "Good",
		watch: "Watch",
		heavy: "Heavy",
		critical: "Critical",
	};

	async function handleFile(file?: File) {
		if (!file) return;
		fileName = file.name || "profiler.json";
		error = "";

		try {
			const text = await file.text();
			const parsed: unknown = JSON.parse(text);
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

	function formatMs(value: number) {
		return `${value.toLocaleString(undefined, { maximumFractionDigits: value >= 100 ? 0 : 2 })} ms`;
	}

	function formatNumber(value: number) {
		return value.toLocaleString(undefined, { maximumFractionDigits: 0 });
	}

	function formatPercent(value: number) {
		return `${value.toLocaleString(undefined, { maximumFractionDigits: 1 })}%`;
	}

	function barWidth(value: number, max: number) {
		if (!max) return 0;
		return Math.max(3, Math.min(100, (value / max) * 100));
	}

	function stateClass(state: ResourceState) {
		return {
			excellent: "border-emerald-400/30 bg-emerald-400/10 text-emerald-200",
			good: "border-sky-400/30 bg-sky-400/10 text-sky-200",
			watch: "border-amber-400/30 bg-amber-400/10 text-amber-200",
			heavy: "border-orange-400/30 bg-orange-400/10 text-orange-200",
			critical: "border-red-400/30 bg-red-400/10 text-red-200",
		}[state];
	}
</script>

<section class="space-y-5 pb-8">
	<div class="flex flex-col justify-between gap-4 lg:flex-row lg:items-end">
		<div>
			<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">Tools</p>
			<h1 class="mt-2 text-3xl font-semibold tracking-normal text-foreground">Profiler Analyzer</h1>
			<p class="mt-2 max-w-2xl text-sm text-muted-foreground">
				Drop in a FiveM profiler JSON export to rank expensive resources, hitch presence, worst spans, and script-time share.
			</p>
		</div>
		<div class="inline-flex items-center gap-2 rounded-sm border border-border bg-card px-3 py-2 text-xs text-muted-foreground">
			<GaugeIcon class="size-3.5" />
			Local trace analysis
		</div>
	</div>

	<div class="grid gap-4 xl:grid-cols-12">
		<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-transform duration-300 hover:-translate-y-0.5 xl:col-span-7">
			<div class="pointer-events-none absolute inset-x-4 top-0 h-px bg-gradient-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"></div>
			<Card.Header class="border-b border-border pb-4">
				<div class="flex items-start gap-3">
					<div class="flex size-9 shrink-0 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
						<TerminalIcon class="size-5" />
					</div>
					<div>
						<Card.Title>Before You Upload</Card.Title>
						<Card.Description>Record a clean sample from the FiveM console first.</Card.Description>
					</div>
				</div>
			</Card.Header>
			<Card.Content class="space-y-3">
				<div class="grid min-w-0 gap-3 rounded-sm border border-border bg-background/60 p-3 sm:grid-cols-[2rem_minmax(0,1fr)]">
					<div class="flex size-8 items-center justify-center rounded-sm bg-muted text-xs font-semibold text-muted-foreground ring-1 ring-border">1</div>
					<div class="min-w-0">
						<p class="text-sm font-medium text-foreground">Record a short profiler sample</p>
						<p class="mt-1 text-xs text-muted-foreground">Run this in the FiveM console while the server is under normal load.</p>
						<code class="mt-2 block max-w-full rounded-sm bg-muted px-2 py-2 font-mono text-xs whitespace-normal text-foreground break-words">profiler record 500</code>
					</div>
				</div>
				<div class="grid min-w-0 gap-3 rounded-sm border border-border bg-background/60 p-3 sm:grid-cols-[2rem_minmax(0,1fr)]">
					<div class="flex size-8 items-center justify-center rounded-sm bg-muted text-xs font-semibold text-muted-foreground ring-1 ring-border">2</div>
					<div class="min-w-0">
						<p class="text-sm font-medium text-foreground">Save the finished capture</p>
						<p class="mt-1 text-xs text-muted-foreground">After the recording finishes, export it as JSON.</p>
						<code class="mt-2 block max-w-full rounded-sm bg-muted px-2 py-2 font-mono text-xs whitespace-normal text-foreground break-words">profiler saveJSON profiler.json</code>
					</div>
				</div>
				<div class="grid min-w-0 gap-3 rounded-sm border border-border bg-background/60 p-3 sm:grid-cols-[2rem_minmax(0,1fr)]">
					<div class="flex size-8 items-center justify-center rounded-sm bg-muted text-xs font-semibold text-muted-foreground ring-1 ring-border">3</div>
					<div class="min-w-0">
						<p class="text-sm font-medium text-foreground">Upload the generated file</p>
						<p class="mt-1 text-xs text-muted-foreground">FiveM usually writes the capture inside the citizen profiler folder.</p>
						<code class="mt-2 block max-w-full rounded-sm bg-muted px-2 py-2 font-mono text-xs whitespace-normal text-foreground break-words">FiveM/FiveM.app/citizen/profiler.json</code>
					</div>
				</div>
			</Card.Content>
		</Card.Root>

		<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-transform duration-300 hover:-translate-y-0.5 xl:col-span-5">
			<div class="pointer-events-none absolute inset-x-4 top-0 h-px bg-gradient-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"></div>
			<Card.Header class="border-b border-border pb-4">
				<Card.Title>Upload Profile</Card.Title>
				<Card.Description>Use the `.json` file created by `profiler saveJSON`.</Card.Description>
			</Card.Header>
			<Card.Content class="space-y-4">
				<label
					class={[
						"flex min-h-48 cursor-pointer flex-col items-center justify-center gap-4 rounded-sm border border-dashed bg-background/60 px-6 text-center transition-colors",
						dragging ? "border-primary/60 bg-primary/10" : "border-border hover:border-primary/40",
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
	</div>

	{#if analysis}
		<div class="grid gap-4 md:grid-cols-2 xl:grid-cols-5">
			{#each [
				{ label: "Recording", value: formatMs(analysis.stats.recordingMs) },
				{ label: "Script CPU", value: formatMs(analysis.stats.totalScriptMs) },
				{ label: "Resources", value: formatNumber(analysis.stats.resourceCount) },
				{ label: "Hitches", value: formatNumber(analysis.stats.hitchCount) },
				{ label: "Worst Hitch", value: formatMs(analysis.stats.worstHitchMs) },
			] as stat}
				<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-transform duration-300 hover:-translate-y-0.5">
					<div class="pointer-events-none absolute inset-x-4 top-0 h-px bg-gradient-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"></div>
					<Card.Content class="p-4">
						<p class="text-xs text-muted-foreground">{stat.label}</p>
						<p class="mt-2 truncate text-xl font-semibold text-foreground">{stat.value}</p>
					</Card.Content>
				</Card.Root>
			{/each}
		</div>

		<div class="grid gap-4 xl:grid-cols-12">
			<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-transform duration-300 hover:-translate-y-0.5 xl:col-span-7">
				<div class="pointer-events-none absolute inset-x-4 top-0 h-px bg-gradient-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"></div>
				<Card.Header class="border-b border-border pb-4">
					<div class="flex items-center gap-3">
						<div class="flex size-9 shrink-0 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
							<BarChart3Icon class="size-5" />
						</div>
						<div>
							<Card.Title>Top CPU Time Graph</Card.Title>
							<Card.Description>Largest total script-time consumers in this sample.</Card.Description>
						</div>
					</div>
				</Card.Header>
				<Card.Content class="space-y-3">
					{#each analysis.graph as resource}
						<div class="grid gap-2 sm:grid-cols-[minmax(0,12rem)_1fr_auto] sm:items-center">
							<div class="min-w-0">
								<p class="truncate text-sm font-medium text-foreground">{resource.name}</p>
								<p class="text-xs text-muted-foreground">{stateLabels[resource.state]} / {resource.dominantKind}</p>
							</div>
							<div class="h-3 overflow-hidden rounded-sm bg-muted">
								<div
									class="h-full rounded-sm bg-gradient-to-r from-primary via-sky-400 to-emerald-300"
									style={`width: ${barWidth(resource.totalMs, analysis.graph[0]?.totalMs ?? 0)}%`}
								></div>
							</div>
							<p class="text-right text-xs text-muted-foreground">{formatMs(resource.totalMs)}</p>
						</div>
					{/each}
				</Card.Content>
			</Card.Root>

			<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-transform duration-300 hover:-translate-y-0.5 xl:col-span-5">
				<div class="pointer-events-none absolute inset-x-4 top-0 h-px bg-gradient-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"></div>
				<Card.Header class="border-b border-border pb-4">
					<div class="flex items-center gap-3">
						<div class="flex size-9 shrink-0 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
							<LightbulbIcon class="size-5" />
						</div>
						<div>
							<Card.Title>Optimization Tips</Card.Title>
							<Card.Description>Suggested places to investigate first.</Card.Description>
						</div>
					</div>
				</Card.Header>
				<Card.Content class="space-y-3">
					{#each analysis.tips as tip}
						<div class="rounded-sm border border-border bg-background/70 p-3 text-sm text-muted-foreground">{tip}</div>
					{/each}
				</Card.Content>
			</Card.Root>
		</div>

		<div class="grid gap-4 xl:grid-cols-3">
			{#each [
				{
					title: "Most Total CPU Time",
					description: "Top 20 resources by all measured resource spans.",
					resources: analysis.topTotal,
					metric: "total",
				},
				{
					title: "Worst Single Tick",
					description: "Top 20 by worst single tick, thread, or event span.",
					resources: analysis.topWorst,
					metric: "worst",
				},
				{
					title: "Present During Hitches",
					description: "Resources active during Resource Manager Tick hitches.",
					resources: analysis.topHitches,
					metric: "hitch",
				},
			] as ranking}
				<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-transform duration-300 hover:-translate-y-0.5">
					<div class="pointer-events-none absolute inset-x-4 top-0 h-px bg-gradient-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"></div>
					<Card.Header class="border-b border-border pb-4">
						<Card.Title>{ranking.title}</Card.Title>
						<Card.Description>{ranking.description}</Card.Description>
					</Card.Header>
					<Card.Content class="space-y-2">
						{#if ranking.resources.length}
							{#each ranking.resources as resource, index}
								<div class="rounded-sm border border-border bg-background/70 p-3 transition-colors hover:bg-muted/30">
									<div class="flex items-start justify-between gap-3">
										<div class="min-w-0">
											<p class="truncate text-sm font-medium text-foreground">{index + 1}. {resource.name}</p>
											<p class="mt-1 text-xs text-muted-foreground">
												{#if ranking.metric === "total"}
													{formatPercent(resource.percentage)} of scripts / {resource.dominantKind}
												{:else if ranking.metric === "worst"}
													worst {resource.worstKind} / avg {formatMs(resource.averageMs)}
												{:else}
													{formatNumber(resource.hitchHits)} hitch hits / {formatMs(resource.hitchMs)} overlap
												{/if}
											</p>
										</div>
										<span class={["shrink-0 rounded-sm border px-2 py-1 text-xs", stateClass(resource.state)]}>
											{stateLabels[resource.state]}
										</span>
									</div>
									<div class="mt-3 flex items-center justify-between gap-3 text-xs text-muted-foreground">
										<span>
											{#if ranking.metric === "total"}
												{formatMs(resource.totalMs)}
											{:else if ranking.metric === "worst"}
												{formatMs(resource.worstMs)}
											{:else}
												{formatMs(resource.worstMs)} worst
											{/if}
										</span>
										<span>{formatNumber(resource.calls)} calls</span>
									</div>
								</div>
							{/each}
						{:else}
							<div class="rounded-sm border border-dashed border-border bg-background/60 p-4 text-sm text-muted-foreground">
								No resources matched this ranking in the uploaded profile.
							</div>
						{/if}
					</Card.Content>
				</Card.Root>
			{/each}
		</div>

		<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-transform duration-300 hover:-translate-y-0.5">
			<div class="pointer-events-none absolute inset-x-4 top-0 h-px bg-gradient-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"></div>
			<Card.Header class="border-b border-border pb-4">
				<div class="flex items-center gap-3">
					<div class="flex size-9 shrink-0 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
						<FileJsonIcon class="size-5" />
					</div>
					<div>
						<Card.Title>Complete Resource List</Card.Title>
						<Card.Description>Total ms, script share, average span, worst span, and calls for every resource found.</Card.Description>
					</div>
				</div>
			</Card.Header>
			<Card.Content>
				<div class="overflow-x-auto">
					<table class="w-full min-w-[860px] text-left text-sm">
						<thead class="text-xs text-muted-foreground">
							<tr class="border-b border-border">
								<th class="py-3 pr-4 font-medium">Resource</th>
								<th class="py-3 pr-4 font-medium">State</th>
								<th class="py-3 pr-4 font-medium">Total</th>
								<th class="py-3 pr-4 font-medium">% Scripts</th>
								<th class="py-3 pr-4 font-medium">Avg/Tick</th>
								<th class="py-3 pr-4 font-medium">Worst</th>
								<th class="py-3 pr-4 font-medium">Calls</th>
								<th class="py-3 pr-4 font-medium">Threads / Ticks</th>
							</tr>
						</thead>
						<tbody>
							{#each analysis.resources as resource}
								<tr class="border-b border-border/70 transition-colors hover:bg-muted/30">
									<td class="max-w-64 py-3 pr-4">
										<p class="truncate font-medium text-foreground">{resource.name}</p>
										<p class="text-xs text-muted-foreground">dominant {resource.dominantKind}</p>
									</td>
									<td class="py-3 pr-4">
										<span class={["inline-flex rounded-sm border px-2 py-1 text-xs", stateClass(resource.state)]}>
											{stateLabels[resource.state]}
										</span>
									</td>
									<td class="py-3 pr-4 text-muted-foreground">{formatMs(resource.totalMs)}</td>
									<td class="py-3 pr-4 text-muted-foreground">{formatPercent(resource.percentage)}</td>
									<td class="py-3 pr-4 text-muted-foreground">{formatMs(resource.averageMs)}</td>
									<td class="py-3 pr-4 text-muted-foreground">{formatMs(resource.worstMs)} <span class="text-xs">({resource.worstKind})</span></td>
									<td class="py-3 pr-4 text-muted-foreground">{formatNumber(resource.calls)}</td>
									<td class="py-3 pr-4 text-muted-foreground">{formatNumber(resource.threadCalls)} / {formatNumber(resource.tickCalls)}</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			</Card.Content>
		</Card.Root>
	{:else}
		<Card.Root class="group relative overflow-hidden rounded-sm border-dashed border-border bg-card/80 shadow-sm transition-transform duration-300 hover:-translate-y-0.5">
			<div class="pointer-events-none absolute inset-x-4 top-0 h-px bg-gradient-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"></div>
			<Card.Content class="flex min-h-44 flex-col items-center justify-center gap-3 text-center">
				<FileJsonIcon class="size-8 text-muted-foreground" />
				<div>
					<p class="text-sm font-medium text-foreground">Waiting for profiler data</p>
					<p class="mt-1 text-xs text-muted-foreground">Upload a FiveM profiler export to populate the stats, graph, top lists, and complete resource table.</p>
				</div>
			</Card.Content>
		</Card.Root>
	{/if}
</section>
