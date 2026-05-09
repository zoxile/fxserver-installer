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

const scriptBudgetMs = 25;
const topLimit = 20;

export function analyzeProfilerJson(value: unknown): ProfilerAnalysis {
	const events = getTraceEvents(value);
	const { spans, firstTs, lastTs } = pairTraceSpans(events);
	const frames = spans.filter((span) => span.kind === "frame").sort((left, right) => left.start - right.start);
	const hitches = frames.filter((frame) => frame.durationMs >= scriptBudgetMs).map(toHitchWindow);
	const entries = aggregateEntries(spans, hitches);
	const totalScriptMs = entries.reduce((sum, entry) => sum + entry.totalMs, 0);
	const profiles = entries
		.map((entry) => toProfile(entry, totalScriptMs))
		.sort((left, right) => right.totalMs - left.totalMs);
	const resourceNames = new Set(profiles.map((profile) => profile.resource).filter((resource): resource is string => Boolean(resource)));
	const frameTimeline = buildFrameTimeline(frames, spans, firstTs ?? 0);
	const resourceManager = profiles.find((profile) => profile.name === "Resource Manager Tick");
	const frameCount = frames.length || frameTimeline.length;

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
		worstHitchMs: hitches.reduce((max, hitch) => Math.max(max, hitch.durationMs), 0),
		averageHitchMs: hitches.length ? hitches.reduce((sum, hitch) => sum + hitch.durationMs, 0) / hitches.length : 0,
		worstFrameMs: frames.reduce((max, frame) => Math.max(max, frame.durationMs), 0),
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
		graph: profiles.slice(0, 10),
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

function aggregateEntries(spans: CompletedSpan[], hitches: HitchWindow[]) {
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

		entry.totalMs += span.durationMs;
		entry.calls += 1;
		entry.worstMs = Math.max(entry.worstMs, span.durationMs);

		const hitchOverlap = getHitchOverlap(span, hitches);
		if (hitchOverlap > 0) {
			entry.hitchHits += 1;
			entry.hitchMs += hitchOverlap;
		}

		entries.set(span.name, entry);
	}

	return [...entries.values()];
}

function getHitchOverlap(span: CompletedSpan, hitches: HitchWindow[]) {
	let overlapMs = 0;

	for (const hitch of hitches) {
		if (span.end < hitch.start || span.start > hitch.end) continue;
		const overlapStart = Math.max(span.start, hitch.start);
		const overlapEnd = Math.min(span.end, hitch.end);
		overlapMs += Math.max(0, (overlapEnd - overlapStart) / 1000);
	}

	return overlapMs;
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

function buildFrameTimeline(frames: CompletedSpan[], spans: CompletedSpan[], firstTs: number) {
	return frames.map((frame, index) => {
		const childSpans = spans.filter((span) => span !== frame && span.kind && span.start >= frame.start && span.end <= frame.end);
		const topEntry = [...childSpans].sort((left, right) => right.durationMs - left.durationMs)[0]?.name;

		return {
			index: index + 1,
			startMs: (frame.start - firstTs) / 1000,
			durationMs: frame.durationMs,
			state: getState({ averageMs: frame.durationMs, worstMs: frame.durationMs, percentage: 0, hitchHits: frame.durationMs >= scriptBudgetMs ? 1 : 0 }),
			entryCount: childSpans.length,
			topEntry,
		};
	});
}

function toHitchWindow(frame: CompletedSpan): HitchWindow {
	return {
		start: frame.start,
		end: frame.end,
		durationMs: frame.durationMs,
	};
}

function countEvents(events: TraceEvent[], name: string) {
	return events.filter((event) => event.name === name && event.ph === "B").length;
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
	const topWorst = [...resources].sort((left, right) => right.worstMs - left.worstMs)[0];
	const topHitch = [...resources].sort((left, right) => right.hitchHits - left.hitchHits || right.hitchMs - left.hitchMs)[0];

	if (!topTotal) {
		return ["No profiler entries were found. Confirm this is a FiveM profiler JSON created with profiler saveJSON."];
	}

	tips.push(`Open ${topTotal.name} first. It is a ${topTotal.kind} entry using ${topTotal.percentage.toFixed(1)}% of measured script time.`);

	if (topWorst) {
		tips.push(`${topWorst.name} has the worst single ${topWorst.kind} at ${topWorst.worstMs.toFixed(2)} ms. Look for loops, sync exports, heavy events, or missing waits near that path.`);
	}

	if (stats.hitchCount && topHitch?.hitchHits) {
		tips.push(`${topHitch.name} was present during ${topHitch.hitchHits} heavy frame${topHitch.hitchHits === 1 ? "" : "s"}. Compare it against the frame timeline.`);
	}

	if (stats.browserFrames || stats.screenshots) {
		tips.push(`The trace also contains ${stats.browserFrames.toLocaleString()} browser frame markers and ${stats.screenshots.toLocaleString()} screenshots, which helps align script spikes with visual frame timing.`);
	}

	return tips;
}
