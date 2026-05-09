export type ResourceProfile = {
	name: string;
	score: number;
	totalTime: number;
	selfTime: number;
	averageTime: number;
	ticks: number;
	samples: number;
};

export type ProfilerAnalysis = {
	resources: ResourceProfile[];
	totalScore: number;
	tips: string[];
};

type ResourceAccumulator = {
	name: string;
	score: number;
	totalTime: number;
	selfTime: number;
	averageTime: number;
	ticks: number;
	samples: number;
};

const resourceKeys = ["resource", "resourceName", "resName", "script", "name", "label"];
const totalKeys = ["total", "totalTime", "totalMs", "time", "elapsed", "duration", "ms", "cpu"];
const selfKeys = ["self", "selfTime", "selfMs", "exclusive"];
const averageKeys = ["avg", "average", "averageTime", "avgMs", "mean"];
const tickKeys = ["ticks", "tick", "calls", "samples", "count"];

export function analyzeProfilerJson(value: unknown): ProfilerAnalysis {
	const resources = new Map<string, ResourceAccumulator>();
	visit(value, resources);

	const ranked = [...resources.values()]
		.map((resource) => ({
			...resource,
			averageTime: resource.averageTime || (resource.ticks ? resource.totalTime / resource.ticks : 0),
		}))
		.filter((resource) => resource.score > 0 || resource.totalTime > 0 || resource.selfTime > 0 || resource.averageTime > 0)
		.sort((left, right) => right.score - left.score);

	return {
		resources: ranked,
		totalScore: ranked.reduce((sum, resource) => sum + resource.score, 0),
		tips: buildTips(ranked),
	};
}

function visit(value: unknown, resources: Map<string, ResourceAccumulator>, inheritedName?: string) {
	if (!value || typeof value !== "object") return;

	if (Array.isArray(value)) {
		for (const entry of value) {
			visit(entry, resources, inheritedName);
		}
		return;
	}

	const record = value as Record<string, unknown>;
	const name = getResourceName(record) ?? inheritedName;
	const metrics = getMetrics(record);

	if (name && hasMetrics(metrics)) {
		const entry = resources.get(name) ?? {
			name,
			score: 0,
			totalTime: 0,
			selfTime: 0,
			averageTime: 0,
			ticks: 0,
			samples: 0,
		};
		entry.totalTime += metrics.totalTime;
		entry.selfTime += metrics.selfTime;
		entry.averageTime = Math.max(entry.averageTime, metrics.averageTime);
		entry.ticks += metrics.ticks;
		entry.samples += 1;
		entry.score += metrics.totalTime + metrics.selfTime * 1.25 + metrics.averageTime * 8 + metrics.ticks * 0.01;
		resources.set(name, entry);
	}

	for (const child of Object.values(record)) {
		visit(child, resources, name);
	}
}

function getResourceName(record: Record<string, unknown>) {
	for (const key of resourceKeys) {
		const value = record[key];
		if (typeof value === "string" && value.trim()) {
			return value.trim();
		}
	}

	return undefined;
}

function getMetrics(record: Record<string, unknown>) {
	return {
		totalTime: maxNumber(record, totalKeys),
		selfTime: maxNumber(record, selfKeys),
		averageTime: maxNumber(record, averageKeys),
		ticks: maxNumber(record, tickKeys),
	};
}

function maxNumber(record: Record<string, unknown>, keys: string[]) {
	let value = 0;

	for (const [key, entry] of Object.entries(record)) {
		if (!keys.some((candidate) => key.toLowerCase().includes(candidate.toLowerCase()))) continue;
		if (typeof entry === "number" && Number.isFinite(entry)) {
			value = Math.max(value, entry);
		}
	}

	return value;
}

function hasMetrics(metrics: ReturnType<typeof getMetrics>) {
	return metrics.totalTime > 0 || metrics.selfTime > 0 || metrics.averageTime > 0 || metrics.ticks > 0;
}

function buildTips(resources: ResourceProfile[]) {
	const tips: string[] = [];
	const top = resources[0];

	if (!top) {
		return ["No obvious resource timings were found. Confirm this is a FiveM profiler JSON export."];
	}

	tips.push(`${top.name} is the top offender in this profile. Start by checking loops, event spam, database calls, and per-frame handlers in that resource.`);

	const highAverage = resources.find((resource) => resource.averageTime >= 5);
	if (highAverage) {
		tips.push(`${highAverage.name} has a high average sample time. Look for synchronous work that can be cached, batched, or moved out of hot paths.`);
	}

	const highTick = resources.find((resource) => resource.ticks >= 1000);
	if (highTick) {
		tips.push(`${highTick.name} appears frequently in the profile. Reduce tick frequency or add early exits where possible.`);
	}

	if (resources.length > 5) {
		tips.push("Compare the top five resources first. Small changes in the worst offenders usually matter more than polishing low-score resources.");
	}

	return tips;
}
