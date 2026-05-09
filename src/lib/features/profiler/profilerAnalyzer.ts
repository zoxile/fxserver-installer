export type ResourceState = "excellent" | "good" | "watch" | "heavy" | "critical";
export type SpanKind = "tick" | "thread" | "event";

export type ResourceProfile = {
	name: string;
	state: ResourceState;
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
	tickCalls: number;
	threadCalls: number;
	eventCalls: number;
	tickMs: number;
	threadMs: number;
	eventMs: number;
	hitchHits: number;
	hitchMs: number;
};

export type ProfilerStats = {
	totalEvents: number;
	totalSpans: number;
	totalScriptMs: number;
	recordingMs: number;
	resourceCount: number;
	hitchCount: number;
	worstHitchMs: number;
	averageHitchMs: number;
};

export type ProfilerAnalysis = {
	stats: ProfilerStats;
	resources: ResourceProfile[];
	totalScore: number;
	topTotal: ResourceProfile[];
	topWorst: ResourceProfile[];
	topHitches: ResourceProfile[];
	graph: ResourceProfile[];
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

type ResourceAccumulator = {
	name: string;
	totalMs: number;
	tickMs: number;
	threadMs: number;
	eventMs: number;
	calls: number;
	tickCalls: number;
	threadCalls: number;
	eventCalls: number;
	worstMs: number;
	worstKind: SpanKind;
	hitchHits: number;
	hitchMs: number;
};

type HitchWindow = {
	start: number;
	end: number;
	durationMs: number;
};

const hitchThresholdMs = 50;
const topLimit = 20;

export function analyzeProfilerJson(value: unknown): ProfilerAnalysis {
	const events = getTraceEvents(value);
	const { spans, hitches, firstTs, lastTs } = pairTraceSpans(events);
	const resources = aggregateResources(spans, hitches);
	const totalScriptMs = resources.reduce((sum, resource) => sum + resource.totalMs, 0);
	const profiles = resources
		.map((resource) => toProfile(resource, totalScriptMs))
		.sort((left, right) => right.totalMs - left.totalMs);

	const stats: ProfilerStats = {
		totalEvents: events.length,
		totalSpans: spans.length,
		totalScriptMs,
		recordingMs: firstTs === undefined || lastTs === undefined ? 0 : Math.max(0, (lastTs - firstTs) / 1000),
		resourceCount: profiles.length,
		hitchCount: hitches.length,
		worstHitchMs: hitches.reduce((max, hitch) => Math.max(max, hitch.durationMs), 0),
		averageHitchMs: hitches.length ? hitches.reduce((sum, hitch) => sum + hitch.durationMs, 0) / hitches.length : 0,
	};

	return {
		stats,
		resources: profiles,
		totalScore: totalScriptMs,
		topTotal: profiles.slice(0, topLimit),
		topWorst: [...profiles].sort((left, right) => right.worstMs - left.worstMs).slice(0, topLimit),
		topHitches: [...profiles]
			.filter((resource) => resource.hitchHits > 0)
			.sort((left, right) => right.hitchHits - left.hitchHits || right.hitchMs - left.hitchMs || right.worstMs - left.worstMs)
			.slice(0, topLimit),
		graph: profiles.slice(0, 10),
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
	const hitches: HitchWindow[] = [];
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
				...getResourceSpanInfo(name),
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
		const completed = { ...open, end: ts, durationMs };
		spans.push(completed);

		if (name === "Resource Manager Tick" && durationMs >= hitchThresholdMs) {
			hitches.push({ start: open.start, end: ts, durationMs });
		}
	}

	return { spans, hitches, firstTs, lastTs };
}

function findOpenSpanIndex(stack: OpenSpan[], name: string) {
	for (let index = stack.length - 1; index >= 0; index -= 1) {
		if (stack[index].name === name) return index;
	}

	return -1;
}

function getResourceSpanInfo(name: string): Pick<OpenSpan, "kind" | "resource"> {
	const tickResource = /^tick \(([^)]+)\)$/.exec(name)?.[1];
	if (tickResource) {
		return { kind: "tick", resource: normalizeResourceName(tickResource) };
	}

	if (name.startsWith("thread @")) {
		return { kind: "thread", resource: extractResourceFromPath(name) };
	}

	if (name.startsWith("event ")) {
		return { kind: "event", resource: extractResourceFromPath(name) };
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

function aggregateResources(spans: CompletedSpan[], hitches: HitchWindow[]) {
	const resources = new Map<string, ResourceAccumulator>();

	for (const span of spans) {
		if (!span.resource || !span.kind || span.durationMs <= 0) continue;

		const resource = resources.get(span.resource) ?? createAccumulator(span.resource);
		resource.totalMs += span.durationMs;
		resource.calls += 1;
		resource[`${span.kind}Ms`] += span.durationMs;
		resource[`${span.kind}Calls`] += 1;

		if (span.durationMs > resource.worstMs) {
			resource.worstMs = span.durationMs;
			resource.worstKind = span.kind;
		}

		const hitchOverlap = getHitchOverlap(span, hitches);
		if (hitchOverlap > 0) {
			resource.hitchHits += 1;
			resource.hitchMs += hitchOverlap;
		}

		resources.set(span.resource, resource);
	}

	return [...resources.values()];
}

function createAccumulator(name: string): ResourceAccumulator {
	return {
		name,
		totalMs: 0,
		tickMs: 0,
		threadMs: 0,
		eventMs: 0,
		calls: 0,
		tickCalls: 0,
		threadCalls: 0,
		eventCalls: 0,
		worstMs: 0,
		worstKind: "tick",
		hitchHits: 0,
		hitchMs: 0,
	};
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

function toProfile(resource: ResourceAccumulator, totalScriptMs: number): ResourceProfile {
	const dominantKind = getDominantKind(resource);
	const averageMs = resource.calls ? resource.totalMs / resource.calls : 0;
	const percentage = totalScriptMs ? (resource.totalMs / totalScriptMs) * 100 : 0;

	return {
		...resource,
		state: getResourceState({ ...resource, averageMs, percentage }),
		dominantKind,
		score: resource.totalMs,
		totalTime: resource.totalMs,
		selfTime: resource.threadMs + resource.eventMs,
		averageTime: averageMs,
		ticks: resource.calls,
		samples: resource.calls,
		averageMs,
		percentage,
	};
}

function getDominantKind(resource: ResourceAccumulator): SpanKind {
	const entries: Array<[SpanKind, number]> = [
		["tick", resource.tickMs],
		["thread", resource.threadMs],
		["event", resource.eventMs],
	];

	return entries.sort((left, right) => right[1] - left[1])[0][0];
}

function getResourceState(resource: ResourceAccumulator & { averageMs: number; percentage: number }): ResourceState {
	if (resource.worstMs >= 50 || resource.averageMs >= 5 || resource.percentage >= 20 || resource.hitchHits >= 10) return "critical";
	if (resource.worstMs >= 16 || resource.averageMs >= 2 || resource.percentage >= 10 || resource.hitchHits >= 4) return "heavy";
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
		return ["No resource timings were found. Confirm this is a FiveM profiler JSON created with profiler saveJSON."];
	}

	tips.push(`${topTotal.name} uses the most script time at ${topTotal.percentage.toFixed(1)}% of measured resource work. Start there before tuning smaller resources.`);

	if (topWorst) {
		tips.push(`${topWorst.name} has the worst single ${topWorst.worstKind} at ${topWorst.worstMs.toFixed(2)} ms. Look for loops, sync exports, heavy events, or missing waits near that path.`);
	}

	if (stats.hitchCount && topHitch?.hitchHits) {
		tips.push(`${topHitch.name} was present during ${topHitch.hitchHits} hitch span${topHitch.hitchHits === 1 ? "" : "s"}. Compare its tick and thread totals against the hitch timestamps.`);
	}

	if (resources.some((resource) => resource.state === "critical")) {
		tips.push("Critical resources usually need structural changes: split work across ticks, cache repeated calculations, and avoid doing database or file work inside hot loops.");
	}

	return tips;
}
