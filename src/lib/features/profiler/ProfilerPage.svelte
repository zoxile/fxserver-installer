<script lang="ts">
	import AlertCircleIcon from "@lucide/svelte/icons/alert-circle";
	import BarChart3Icon from "@lucide/svelte/icons/bar-chart-3";
	import FileJsonIcon from "@lucide/svelte/icons/file-json";
	import GaugeIcon from "@lucide/svelte/icons/gauge";
	import LightbulbIcon from "@lucide/svelte/icons/lightbulb";
	import TerminalIcon from "@lucide/svelte/icons/terminal";
	import UploadCloudIcon from "@lucide/svelte/icons/upload-cloud";
	import * as Card from "$lib/components/ui/card/index.js";
	import { log } from "$lib/core/logger.svelte";
	import { Progress } from "$lib/components/ui/progress/index.js";
	import { analyzeProfilerJson, type FrameProfile, type ProfilerAnalysis, type ResourceState } from "./profilerAnalyzer";

	let analysis = $state<ProfilerAnalysis | null>(null);
	let hoveredFrame = $state<FrameProfile | null>(null);
	let fileName = $state("");
	let error = $state("");
	let dragging = $state(false);
	let activeFrame = $derived(hoveredFrame ?? analysis?.frameTimeline[0] ?? null);

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
		log("Profiler file load started.", { scope: "profiler", detail: `${fileName} (${file.size} bytes)` });

		try {
			const text = await file.text();
			const parsed: unknown = JSON.parse(text);
			const nextAnalysis = analyzeProfilerJson(parsed);
			analysis = nextAnalysis;
			hoveredFrame = nextAnalysis.frameTimeline[0] ?? null;
			log("Profiler file analyzed successfully.", {
				level: "success",
				scope: "profiler",
				detail: `${nextAnalysis.stats.frameCount} frames, ${nextAnalysis.resources.length} resources`,
			});
		} catch (caught) {
			analysis = null;
			hoveredFrame = null;
			error = caught instanceof Error ? caught.message : String(caught);
			log("Profiler file analysis failed.", { level: "error", scope: "profiler", detail: error });
		}
	}

	function onDrop(event: DragEvent) {
		event.preventDefault();
		dragging = false;
		void handleFile(event.dataTransfer?.files?.[0]);
	}

	function formatMs(value: number) {
		return `${value.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 2 })} ms`;
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

	function stateBarClass(state: ResourceState) {
		return {
			excellent: "bg-emerald-400",
			good: "bg-sky-400",
			watch: "bg-amber-400",
			heavy: "bg-orange-500",
			critical: "bg-red-500",
		}[state];
	}

	function stateTextClass(state: ResourceState) {
		return {
			excellent: "text-emerald-300",
			good: "text-sky-300",
			watch: "text-amber-300",
			heavy: "text-orange-300",
			critical: "text-red-300",
		}[state];
	}
</script>

<section class="space-y-5 pb-8">
	<div class="flex flex-col justify-between gap-4 lg:flex-row lg:items-end">
		<div>
			<p class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">Tools</p>
			<h1 class="mt-2 text-3xl font-semibold tracking-normal text-foreground">Profiler Analyzer</h1>
			<p class="mt-2 max-w-2xl text-sm text-muted-foreground">Drop in a FiveM profiler JSON export to rank expensive resources, hitch presence, worst spans, and script-time share.</p>
		</div>
		<div class="inline-flex items-center gap-2 rounded-sm border border-border bg-card px-3 py-2 text-xs text-muted-foreground">
			<GaugeIcon class="size-3.5" />
			Local trace analysis
		</div>
	</div>

	<div class="grid gap-4 xl:grid-cols-12">
		<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-shadow duration-500 ease-[cubic-bezier(0.22,1,0.36,1)] xl:col-span-7">
			<div
				class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"
			></div>
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
						<code class="mt-2 block max-w-full rounded-sm border border-border bg-black/50 px-3 py-2 font-mono text-xs whitespace-normal text-sky-100 shadow-inner wrap-break-word">
							<span class="text-muted-foreground">$</span> profiler record 500
						</code>
					</div>
				</div>
				<div class="grid min-w-0 gap-3 rounded-sm border border-border bg-background/60 p-3 sm:grid-cols-[2rem_minmax(0,1fr)]">
					<div class="flex size-8 items-center justify-center rounded-sm bg-muted text-xs font-semibold text-muted-foreground ring-1 ring-border">2</div>
					<div class="min-w-0">
						<p class="text-sm font-medium text-foreground">Save the finished capture</p>
						<p class="mt-1 text-xs text-muted-foreground">After the recording finishes, export it as JSON.</p>
						<code class="mt-2 block max-w-full rounded-sm border border-border bg-black/50 px-3 py-2 font-mono text-xs whitespace-normal text-sky-100 shadow-inner wrap-break-word">
							<span class="text-muted-foreground">$</span> profiler saveJSON profiler.json
						</code>
					</div>
				</div>
				<div class="grid min-w-0 gap-3 rounded-sm border border-border bg-background/60 p-3 sm:grid-cols-[2rem_minmax(0,1fr)]">
					<div class="flex size-8 items-center justify-center rounded-sm bg-muted text-xs font-semibold text-muted-foreground ring-1 ring-border">3</div>
					<div class="min-w-0">
						<p class="text-sm font-medium text-foreground">Upload the generated file</p>
						<p class="mt-1 text-xs text-muted-foreground">FiveM usually writes the capture inside the citizen profiler folder.</p>
						<code class="mt-2 block max-w-full rounded-sm border border-border bg-black/50 px-3 py-2 font-mono text-xs whitespace-normal text-emerald-100 shadow-inner wrap-break-word">
							FiveM/FiveM.app/citizen/profiler.json
						</code>
					</div>
				</div>
			</Card.Content>
		</Card.Root>

		<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-shadow duration-500 ease-[cubic-bezier(0.22,1,0.36,1)] xl:col-span-5">
			<div
				class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"
			></div>
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
					<input type="file" accept=".json,application/json" class="sr-only" onchange={(event) => void handleFile(event.currentTarget.files?.[0])} />
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
		<div class="grid gap-4 md:grid-cols-2 xl:grid-cols-6">
			{#each [{ label: "Recording", value: formatMs(analysis.stats.recordingMs), description: "Capture length" }, { label: "Avg script / frame", value: formatMs(analysis.stats.averageScriptMsPerFrame), description: "Measured script time" }, { label: "Hitches", value: `${formatNumber(analysis.stats.hitchCount)} / ${formatNumber(analysis.stats.frameCount)}`, description: "frames >25ms" }, { label: "Heavy ticks", value: `${formatNumber(analysis.stats.heavyTickCount)} / ${formatNumber(analysis.stats.frameCount)}`, description: "ticks >25ms scripts" }, { label: "Profiler entries", value: formatNumber(analysis.stats.entryCount), description: "Trace rows" }, { label: "Resource manager", value: `${formatMs(analysis.stats.resourceManagerTotalMs)} / ${formatNumber(analysis.stats.resourceManagerCalls)}`, description: "total / frames" }] as stat}
				<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-shadow duration-500 ease-[cubic-bezier(0.22,1,0.36,1)]">
					<div
						class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"
					></div>
					<Card.Content class="p-4">
						<p class="text-xs text-muted-foreground">{stat.label}</p>
						<p class="mt-2 truncate text-xl font-semibold text-foreground">{stat.value}</p>
						<p class="mt-1 truncate text-xs text-muted-foreground">{stat.description}</p>
					</Card.Content>
				</Card.Root>
			{/each}
		</div>

		<div class="grid gap-4 xl:grid-cols-12">
			<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-shadow duration-500 ease-[cubic-bezier(0.22,1,0.36,1)] xl:col-span-7">
				<div
					class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"
				></div>
				<Card.Header class="border-b border-border pb-4">
					<div class="flex items-center gap-3">
						<div class="flex size-9 shrink-0 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
							<BarChart3Icon class="size-5" />
						</div>
						<div>
							<Card.Title>Top CPU Time Graph</Card.Title>
							<Card.Description>Largest measured profiler entries in this sample.</Card.Description>
						</div>
					</div>
				</Card.Header>
				<Card.Content class="space-y-3">
					{#each analysis.graph as resource}
						<div class="grid gap-2 sm:grid-cols-[minmax(0,12rem)_minmax(0,1fr)_5rem] sm:items-center">
							<div class="min-w-0">
								<p class="truncate text-sm font-medium text-foreground">{resource.name}</p>
								<p class="text-xs text-muted-foreground">{stateLabels[resource.state]} / {resource.kind}</p>
							</div>

							<Progress value={barWidth(resource.totalMs, analysis.graph[0]?.totalMs ?? 0)} class="h-2 rounded-xs" indicatorClass={stateBarClass(resource.state)} />

							<p class="w-20 text-right text-xs text-muted-foreground">
								{formatMs(resource.totalMs)}
							</p>
						</div>
					{/each}
				</Card.Content>
			</Card.Root>

			<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-shadow duration-500 ease-[cubic-bezier(0.22,1,0.36,1)] xl:col-span-5">
				<div
					class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"
				></div>
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

		<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-shadow duration-500 ease-[cubic-bezier(0.22,1,0.36,1)]">
			<div
				class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"
			></div>
			<Card.Header class="border-b border-border pb-4">
				<div class="flex items-center justify-between gap-4">
					<div>
						<Card.Title>Frame Timeline</Card.Title>
						<Card.Description>Total measured script time per profiler frame. The dashed marker is the 25ms script budget.</Card.Description>
					</div>
					<div class="hidden items-center gap-3 text-xs text-muted-foreground sm:flex">
						<span class="inline-flex items-center gap-1"><span class="size-2 rounded-full bg-emerald-400"></span> Normal</span>
						<span class="inline-flex items-center gap-1"><span class="size-2 rounded-full bg-amber-400"></span> Watch</span>
						<span class="inline-flex items-center gap-1"><span class="size-2 rounded-full bg-red-500"></span> Critical</span>
					</div>
				</div>
			</Card.Header>
			<Card.Content>
				{#if analysis.frameTimeline.length}
					<div class="space-y-4 rounded-sm border border-border bg-background/70 p-4">
						<div class="relative h-52 overflow-hidden rounded-sm bg-background/70">
							<div class="absolute inset-x-0 border-t border-dashed border-muted-foreground/35" style={`bottom: ${barWidth(25, Math.max(25, analysis.stats.worstFrameMs))}%`}>
								<span class="absolute -top-5 left-1 text-xs text-muted-foreground">25ms script budget</span>
							</div>
							<div class="absolute inset-x-0 bottom-0 h-px bg-emerald-500/70"></div>
							<div class="absolute inset-0 flex items-end gap-px pt-8">
								{#each analysis.frameTimeline as frame}
									<button
										type="button"
										class="group/frame flex h-full min-w-0 flex-1 cursor-pointer items-end rounded-t-[1px] outline-none transition-opacity hover:opacity-90 focus-visible:ring-2 focus-visible:ring-ring"
										aria-label={`Frame ${frame.index}, ${formatMs(frame.durationMs)}`}
										onmouseenter={() => (hoveredFrame = frame)}
										onfocus={() => (hoveredFrame = frame)}
									>
										<span
											class={["block w-full rounded-t-[1px] transition-all", stateBarClass(frame.state)]}
											style={`height: ${barWidth(frame.durationMs, Math.max(25, analysis.stats.worstFrameMs))}%`}
										></span>
									</button>
								{/each}
							</div>
						</div>

						{#if activeFrame}
							<div class="rounded-sm bg-muted px-3 py-2 text-xs text-muted-foreground">
								<div class="flex flex-wrap items-center gap-x-3 gap-y-1">
									<span class="font-semibold text-foreground">Frame {activeFrame.index}</span>
									<span class={stateTextClass(activeFrame.state)}>{formatMs(activeFrame.durationMs)}</span>
									<span>
										Top:
										{#each activeFrame.topEntries as entry, index}
											{#if index > 0},
											{/if}{entry.name} [{formatMs(entry.totalMs)}]
										{/each}
									</span>
								</div>
							</div>
						{/if}
					</div>
				{:else}
					<div class="rounded-sm border border-dashed border-border bg-background/60 p-4 text-sm text-muted-foreground">No Resource Manager Tick frames were found in this profiler export.</div>
				{/if}
			</Card.Content>
		</Card.Root>

		<div class="grid gap-4 xl:grid-cols-3">
			{#each [{ title: "Most Total CPU Time", description: "Top 20 entries by total measured time.", resources: analysis.topTotal, metric: "total" }, { title: "Worst Single Tick", description: "Top 20 by worst single frame, tick, thread, event, or ref span.", resources: analysis.topWorst, metric: "worst" }, { title: "Present During Hitches", description: "Entries active during frames over the 25ms budget.", resources: analysis.topHitches, metric: "hitch" }] as ranking}
				<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-shadow duration-500 ease-[cubic-bezier(0.22,1,0.36,1)]">
					<div
						class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"
					></div>
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
													{formatPercent(resource.percentage)} of scripts / {resource.kind}
												{:else if ranking.metric === "worst"}
													worst {resource.kind} / avg {formatMs(resource.averageMs)}
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
										<Progress value={barWidth(resource.totalMs, ranking.resources[0]?.totalMs ?? 0)} class="h-1.5 min-w-0 flex-1 rounded-xs" indicatorClass={stateBarClass(resource.state)} />
										<span>
											{#if ranking.metric === "total"}
												{formatMs(resource.totalMs)}
											{:else if ranking.metric === "worst"}
												{formatMs(resource.worstMs)}
											{:else}
												{formatMs(resource.worstMs)} worst
											{/if}
										</span>
										<span>{formatNumber(resource.calls)} frames</span>
									</div>
								</div>
							{/each}
						{:else}
							<div class="rounded-sm border border-dashed border-border bg-background/60 p-4 text-sm text-muted-foreground">No resources matched this ranking in the uploaded profile.</div>
						{/if}
					</Card.Content>
				</Card.Root>
			{/each}
		</div>

		<Card.Root class="group relative overflow-hidden rounded-sm border-border bg-card shadow-sm transition-shadow duration-500 ease-[cubic-bezier(0.22,1,0.36,1)]">
			<div
				class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"
			></div>
			<Card.Header class="border-b border-border pb-4">
				<div class="flex items-center gap-3">
					<div class="flex size-9 shrink-0 items-center justify-center rounded-sm bg-muted text-muted-foreground ring-1 ring-border">
						<FileJsonIcon class="size-5" />
					</div>
					<div>
						<Card.Title>Complete Profiler Entry List</Card.Title>
						<Card.Description>
							{formatNumber(analysis.stats.entryCount)} entries across {formatNumber(analysis.stats.resourceCount)} resources. Trace includes {formatNumber(analysis.stats.totalEvents)} events, {formatNumber(
								analysis.stats.browserFrames,
							)} browser frames, and {formatNumber(analysis.stats.screenshots)} screenshots.
						</Card.Description>
					</div>
				</div>
			</Card.Header>
			<Card.Content>
				<div class="overflow-x-auto">
					<table class="w-full min-w-240 text-left text-sm">
						<thead class="text-xs text-muted-foreground">
							<tr class="border-b border-border">
								<th class="py-3 pr-4 font-medium">Entry</th>
								<th class="py-3 pr-4 font-medium">Type</th>
								<th class="py-3 pr-4 font-medium">State</th>
								<th class="py-3 pr-4 font-medium">Total</th>
								<th class="py-3 pr-4 font-medium">% Scripts</th>
								<th class="py-3 pr-4 font-medium">Avg/Call</th>
								<th class="py-3 pr-4 font-medium">Worst</th>
								<th class="py-3 pr-4 font-medium">Frames</th>
								<th class="py-3 pr-4 font-medium">Weight</th>
							</tr>
						</thead>
						<tbody>
							{#each analysis.resources as resource}
								<tr class="border-b border-border/70 transition-colors hover:bg-muted/30">
									<td class="max-w-96 py-3 pr-4">
										<p class="truncate font-medium text-foreground">{resource.name}</p>
										<p class="text-xs text-muted-foreground">{resource.resource ?? "trace runtime"}</p>
									</td>
									<td class="py-3 pr-4 text-muted-foreground">{resource.kind}</td>
									<td class="py-3 pr-4">
										<span class={["inline-flex rounded-sm border px-2 py-1 text-xs", stateClass(resource.state)]}>
											{stateLabels[resource.state]}
										</span>
									</td>
									<td class="py-3 pr-4 text-muted-foreground">{formatMs(resource.totalMs)}</td>
									<td class="py-3 pr-4 text-muted-foreground">{formatPercent(resource.percentage)}</td>
									<td class="py-3 pr-4 text-muted-foreground">{formatMs(resource.averageMs)}</td>
									<td class={["py-3 pr-4", stateTextClass(resource.state)]}>{formatMs(resource.worstMs)}</td>
									<td class="py-3 pr-4 text-muted-foreground">{formatNumber(resource.calls)}</td>
									<td class="py-3 pr-4">
										<Progress value={barWidth(resource.totalMs, analysis.resources[0]?.totalMs ?? 0)} class="h-1.5 w-24 rounded-xs" indicatorClass={stateBarClass(resource.state)} />
									</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			</Card.Content>
		</Card.Root>
	{:else}
		<Card.Root class="group relative overflow-hidden rounded-sm border-dashed border-border bg-card/80 shadow-sm transition-shadow duration-500 ease-[cubic-bezier(0.22,1,0.36,1)]">
			<div
				class="pointer-events-none absolute inset-x-4 top-0 h-px bg-linear-to-r from-transparent via-primary/70 to-transparent opacity-0 transition-opacity duration-300 group-hover:opacity-100"
			></div>
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
