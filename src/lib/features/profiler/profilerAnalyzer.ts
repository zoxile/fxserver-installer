export type ResourceState = "excellent" | "good" | "watch" | "heavy" | "critical";
export type SpanKind = "frame" | "tick" | "thread" | "event" | "ref";

export type ResourceProfile = {
	name: string;
	resource?: string;
	state: ResourceState;
	kind: SpanKind;
	dominantKind: SpanKind;
	score: number;
	totalTime: number;
	selfTime: number;
	averageTime: number;
	ticks: number;
	samples: number;
	totalMs: number;
	percentage: number;
	averageMs: number;
	worstMs: number;
	worstKind: SpanKind;
	calls: number;
	hitchHits: number;
	hitchMs: number;
};

export type FrameProfile = {
	index: number;
	startMs: number;
	durationMs: number;
	resourceManagerMs: number;
	state: ResourceState;
	entryCount: number;
	topEntry?: string;
};

export type ProfilerStats = {
	totalEvents: number;
	totalSpans: number;
	totalScriptMs: number;
	recordingMs: number;
	resourceCount: number;
	entryCount: number;
	frameCount: number;
	averageScriptMsPerFrame: number;
	hitchCount: number;
	hitchPercent: number;
	heavyTickCount: number;
	heavyTickPercent: number;
	worstHitchMs: number;
	averageHitchMs: number;
	worstFrameMs: number;
	resourceManagerTotalMs: number;
	resourceManagerCalls: number;
	browserFrames: number;
	screenshots: number;
};

export type ProfilerAnalysis = {
	stats: ProfilerStats;
	resources: ResourceProfile[];
	totalScore: number;
	topTotal: ResourceProfile[];
	topWorst: ResourceProfile[];
	topHitches: ResourceProfile[];
	graph: ResourceProfile[];
	frameTimeline: FrameProfile[];
	tips: string[];
};

type TraceEvent = {
	name?: unknown;
	ph?: unknown;
	pid?: unknown;
	tid?: unknown;
	ts?: unknown;
};

type OpenSpan = {
	name: string;
	kind?: SpanKind;
	resource?: string;
	start: number;
	threadKey: string;
};

type CompletedSpan = OpenSpan & {
	durationMs: number;
	end: number;
};

type EntryAccumulator = {
	name: string;
	resource?: string;
	kind: SpanKind;
	totalMs: number;
	calls: number;
	worstMs: number;
	hitchHits: number;
	hitchMs: number;
};

type HitchWindow = {
	start: number;
	end: number;
	durationMs: number;
};

type FrameBucket = HitchWindow & {
	index: number;
	resourceManagerMs: number;
	entryTotals: Map<string, number>;
	topEntry?: string;
};

const scriptBudgetMs = 25;
const spansPerProfilerFrame = 100;
const topLimit = 20;

export function analyzeProfilerJson(value: unknown): ProfilerAnalysis {
	const events = getTraceEvents(value);
	const { spans, firstTs, lastTs } = pairTraceSpans(events);
	const relevantSpans = spans.filter((span) => span.kind && span.durationMs > 0);
	const resourceManagerSpans = relevantSpans.filter((span) => span.kind === "frame").sort((left, right) => left.start - right.start);
	const frameBuckets = buildFrameBuckets(relevantSpans, resourceManagerSpans, firstTs, lastTs);
	const hitches = frameBuckets.filter(isHeavyFrame);
	const entries = aggregateEntries(relevantSpans, frameBuckets);
	const totalScriptMs = entries.reduce((sum, entry) => sum + entry.totalMs, 0);
	const profiles = entries.map((entry) => toProfile(entry, totalScriptMs)).sort((left, right) => right.totalMs - left.totalMs);
	const resourceNames = new Set(profiles.map((profile) => profile.resource).filter((resource): resource is string => Boolean(resource)));
	const frameTimeline = buildFrameTimeline(frameBuckets, firstTs ?? 0);
	const resourceManager = profiles.find((profile) => profile.name === "Resource Manager Tick");
	const frameCount = frameTimeline.length;

	const stats: ProfilerStats = {
		totalEvents: events.length,
		totalSpans: spans.length,
		totalScriptMs,
		recordingMs: firstTs === undefined || lastTs === undefined ? 0 : Math.max(0, (lastTs - firstTs) / 1000),
		resourceCount: resourceNames.size,
		entryCount: profiles.length,
		frameCount,
		averageScriptMsPerFrame: frameCount ? totalScriptMs / frameCount : 0,
		hitchCount: hitches.length,
		hitchPercent: frameCount ? (hitches.length / frameCount) * 100 : 0,
		heavyTickCount: hitches.length,
		heavyTickPercent: frameCount ? (hitches.length / frameCount) * 100 : 0,
		worstHitchMs: hitches.reduce((max, hitch) => Math.max(max, hitch.durationMs), 0),
		averageHitchMs: hitches.length ? hitches.reduce((sum, hitch) => sum + hitch.durationMs, 0) / hitches.length : 0,
		worstFrameMs: frameBuckets.reduce((max, frame) => Math.max(max, frame.durationMs), 0),
		resourceManagerTotalMs: resourceManager?.totalMs ?? 0,
		resourceManagerCalls: resourceManager?.calls ?? 0,
		browserFrames: countEvents(events, "BeginFrame"),
		screenshots: countEvents(events, "Screenshot"),
	};

	return {
		stats,
		resources: profiles,
		totalScore: totalScriptMs,
		topTotal: profiles.slice(0, topLimit),
		topWorst: [...profiles].sort((left, right) => right.worstMs - left.worstMs).slice(0, topLimit),
		topHitches: [...profiles]
			.filter((profile) => profile.hitchHits > 0)
			.sort((left, right) => right.hitchHits - left.hitchHits || right.hitchMs - left.hitchMs || right.worstMs - left.worstMs)
			.slice(0, topLimit),
		graph: profiles.slice(0, topLimit),
		frameTimeline,
		tips: buildTips(profiles, stats),
	};
}

function getTraceEvents(value: unknown): TraceEvent[] {
	if (!value || typeof value !== "object") {
		throw new Error("Profiler JSON must be an object with a traceEvents array.");
	}

	const traceEvents = (value as { traceEvents?: unknown }).traceEvents;
	if (!Array.isArray(traceEvents)) {
		throw new Error("Profiler JSON is missing traceEvents. Use FiveM's profiler saveJSON output.");
	}

	return traceEvents as TraceEvent[];
}

function pairTraceSpans(events: TraceEvent[]) {
	const stacks = new Map<string, OpenSpan[]>();
	const spans: CompletedSpan[] = [];
	let firstTs: number | undefined;
	let lastTs: number | undefined;

	for (const event of events) {
		const name = typeof event.name === "string" ? event.name : "";
		const phase = typeof event.ph === "string" ? event.ph : "";
		const ts = typeof event.ts === "number" ? event.ts : undefined;
		if (!name || ts === undefined) continue;

		firstTs = firstTs === undefined ? ts : Math.min(firstTs, ts);
		lastTs = lastTs === undefined ? ts : Math.max(lastTs, ts);

		const threadKey = `${String(event.pid ?? "0")}:${String(event.tid ?? "0")}`;
		const stack = stacks.get(threadKey) ?? [];

		if (phase === "B") {
			stack.push({
				name,
				...getSpanInfo(name),
				start: ts,
				threadKey,
			});
			stacks.set(threadKey, stack);
			continue;
		}

		if (phase !== "E" || !stack.length) continue;

		const index = findOpenSpanIndex(stack, name);
		if (index === -1) continue;

		const [open] = stack.splice(index, 1);
		const durationMs = Math.max(0, (ts - open.start) / 1000);
		spans.push({ ...open, end: ts, durationMs });
	}

	return { spans, firstTs, lastTs };
}

function findOpenSpanIndex(stack: OpenSpan[], name: string) {
	for (let index = stack.length - 1; index >= 0; index -= 1) {
		if (stack[index].name === name) return index;
	}

	return -1;
}

function getSpanInfo(name: string): Pick<OpenSpan, "kind" | "resource"> {
	if (name === "Resource Manager Tick") {
		return { kind: "frame", resource: "Resource Manager" };
	}

	const tickResource = /^tick \(([^)]+)\)$/.exec(name)?.[1];
	if (tickResource) {
		return { kind: "tick", resource: normalizeResourceName(tickResource) };
	}

	if (name.startsWith("thread @")) {
		return { kind: "thread", resource: extractResourceFromPath(name) };
	}

	if (name.startsWith("event ") && name.includes("@")) {
		return { kind: "event", resource: extractResourceFromPath(name) };
	}

	if (name.includes("@")) {
		return { kind: "ref", resource: extractResourceFromPath(name) };
	}

	return {};
}

function extractResourceFromPath(name: string) {
	const match = /@([^/\]\s]+)/.exec(name);
	return match ? normalizeResourceName(match[1]) : undefined;
}

function normalizeResourceName(name: string) {
	return name.trim().replace(/^@/, "") || undefined;
}

function aggregateEntries(spans: CompletedSpan[], frameBuckets: FrameBucket[]) {
	const entries = new Map<string, EntryAccumulator>();

	for (const span of spans) {
		if (!span.kind || span.durationMs <= 0) continue;

		const entry = entries.get(span.name) ?? {
			name: span.name,
			resource: span.resource,
			kind: span.kind,
			totalMs: 0,
			calls: 0,
			worstMs: 0,
			hitchHits: 0,
			hitchMs: 0,
		};

		entries.set(span.name, entry);
	}

	for (const frame of frameBuckets) {
		for (const [entryName, frameTotal] of frame.entryTotals) {
			const entry = entries.get(entryName);
			if (!entry || frameTotal <= 0) continue;

			entry.totalMs += frameTotal;
			entry.calls += 1;
			entry.worstMs = Math.max(entry.worstMs, frameTotal);

			if (isHeavyFrame(frame)) {
				entry.hitchHits += 1;
				entry.hitchMs += frameTotal;
			}
		}
	}

	return [...entries.values()];
}

function buildFrameBuckets(spans: CompletedSpan[], resourceManagerSpans: CompletedSpan[], firstTs?: number, lastTs?: number) {
	if (!spans.length) return [];

	if (!resourceManagerSpans.length) {
		const start = firstTs ?? Math.min(...spans.map((span) => span.start));
		const end = lastTs ?? Math.max(...spans.map((span) => span.end));
		return [buildFrameBucket(1, start, end, spans)];
	}

	const buckets: FrameBucket[] = [];
	const bucketCount = Math.max(1, Math.ceil(resourceManagerSpans.length / spansPerProfilerFrame));

	for (let index = 0; index < bucketCount; index += 1) {
		const bucketStartFrame = resourceManagerSpans[index * spansPerProfilerFrame];
		const nextBucketStartFrame = resourceManagerSpans[(index + 1) * spansPerProfilerFrame];
		const start = bucketStartFrame?.start ?? resourceManagerSpans[0].start;
		const end = nextBucketStartFrame?.start ?? resourceManagerSpans.at(-1)?.end ?? start;
		buckets.push(buildFrameBucket(index + 1, start, end, spans));
	}

	return buckets;
}

function buildFrameBucket(index: number, start: number, end: number, spans: CompletedSpan[]): FrameBucket {
	const entryTotals = new Map<string, number>();
	let resourceManagerMs = 0;

	for (const span of spans) {
		if (!span.kind || span.start < start || span.start >= end) continue;
		entryTotals.set(span.name, (entryTotals.get(span.name) ?? 0) + span.durationMs);
		if (span.kind === "frame") {
			resourceManagerMs += span.durationMs;
		}
	}

	const durationMs = [...entryTotals.values()].reduce((sum, total) => sum + total, 0);
	const topEntry = [...entryTotals.entries()].filter(([name]) => name !== "Resource Manager Tick").sort((left, right) => right[1] - left[1])[0]?.[0];

	return {
		index,
		start,
		end,
		durationMs,
		resourceManagerMs,
		entryTotals,
		topEntry,
	};
}

function toProfile(entry: EntryAccumulator, totalScriptMs: number): ResourceProfile {
	const averageMs = entry.calls ? entry.totalMs / entry.calls : 0;
	const percentage = totalScriptMs ? (entry.totalMs / totalScriptMs) * 100 : 0;
	const state = getState({ averageMs, worstMs: entry.worstMs, percentage, hitchHits: entry.hitchHits });

	return {
		...entry,
		state,
		dominantKind: entry.kind,
		score: entry.totalMs,
		totalTime: entry.totalMs,
		selfTime: entry.kind === "frame" ? 0 : entry.totalMs,
		averageTime: averageMs,
		ticks: entry.calls,
		samples: entry.calls,
		averageMs,
		percentage,
		worstKind: entry.kind,
	};
}

function buildFrameTimeline(frames: FrameBucket[], firstTs: number) {
	return frames.map((frame) => ({
		index: frame.index,
		startMs: (frame.start - firstTs) / 1000,
		durationMs: frame.durationMs,
		resourceManagerMs: frame.resourceManagerMs,
		state: getState({ averageMs: frame.durationMs, worstMs: frame.durationMs, percentage: 0, hitchHits: isHeavyFrame(frame) ? 1 : 0 }),
		entryCount: frame.entryTotals.size,
		topEntry: frame.topEntry,
	}));
}

function countEvents(events: TraceEvent[], name: string) {
	return events.filter((event) => event.name === name && event.ph === "B").length;
}

function isHeavyFrame(frame: Pick<FrameBucket, "durationMs">) {
	return frame.durationMs > scriptBudgetMs;
}

function getState(resource: { averageMs: number; worstMs: number; percentage: number; hitchHits: number }): ResourceState {
	if (resource.worstMs >= 50 || resource.averageMs >= 5 || resource.percentage >= 20 || resource.hitchHits >= 10) return "critical";
	if (resource.worstMs >= 25 || resource.averageMs >= 2 || resource.percentage >= 10 || resource.hitchHits >= 4) return "heavy";
	if (resource.worstMs >= 8 || resource.averageMs >= 1 || resource.percentage >= 5 || resource.hitchHits >= 1) return "watch";
	if (resource.worstMs < 1 && resource.averageMs < 0.25 && resource.percentage < 1) return "excellent";
	return "good";
}

function buildTips(resources: ResourceProfile[], stats: ProfilerStats) {
	const tips: string[] = [];
	const topTotal = resources[0];
	const editableEntries = resources.filter((resource) => resource.kind !== "frame");

	const topWorst = [...editableEntries].sort((left, right) => right.worstMs - left.worstMs)[0];
	const topHitch = [...resources].sort((left, right) => right.hitchHits - left.hitchHits || right.hitchMs - left.hitchMs)[0];

	if (!topTotal) {
		return ["No profiler entries were found. Confirm this is a FiveM profiler JSON created with profiler saveJSON."];
	}

	if (topTotal.kind === "frame") {
		tips.push(`${topTotal.name} is the frame budget container. It is not a script file and cannot be edited directly.`);
		tips.push("Use the frame entry to understand total server frame cost, then inspect the resource entries below it.");
	}

	const openTargets = editableEntries.slice(0, 2).map((resource) => resource.name);

	if (openTargets.length) {
		tips.push(`Start by checking: ${openTargets.join(", ")}.`);
		tips.push("These entries are the most relevant editable code paths in this profile.");
	}

	if (topWorst) {
		tips.push(`${topWorst.name} had the worst single ${topWorst.kind}: ${topWorst.worstMs.toFixed(2)} ms.`);
		tips.push("Check for heavy loops, sync exports, large events, or missing waits.");
	}

	if (stats.hitchCount && topHitch?.hitchHits) {
		tips.push(`${topHitch.name} appeared during ${topHitch.hitchHits} heavy frame${topHitch.hitchHits === 1 ? "" : "s"}.`);
		tips.push("Compare this resource against the frame timeline to confirm if it caused the hitch.");
	}

	if (stats.browserFrames || stats.screenshots) {
		tips.push(`Trace includes ${stats.browserFrames.toLocaleString()} browser frame markers and ${stats.screenshots.toLocaleString()} screenshots.`);
		tips.push("Use these markers to compare script spikes with visual frame timing.");
	}

	tips.push("If monitor or resource-manager has high tick time, run txAdmin as a system service.");
	tips.push("If a script uses Wait(0), make sure the work really needs to run every frame.");
	tips.push("For non-critical repeated work, prefer Wait(500) or Wait(1000).");

	return tips;
}
