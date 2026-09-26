import { analyzeProfilerJson, compareProfilerCaptures } from "./profilerAnalyzer";
import { MAX_PROFILER_BYTES, type ProfilerAnalysisRequest, type ProfilerAnalysisResponse } from "./profilerAnalysisProtocol";

self.onmessage = async (event: MessageEvent<ProfilerAnalysisRequest>) => {
	const reply = (response: ProfilerAnalysisResponse) => self.postMessage(response);
	try {
		const { input, baseline } = event.data;
		if ((typeof input === "string" ? input.length : input.size) > MAX_PROFILER_BYTES) {
			reply({ error: "Profiler exports are limited to 16 MiB." });
			return;
		}
		const json = typeof input === "string" ? input : await input.text();
		if (new TextEncoder().encode(json).byteLength > MAX_PROFILER_BYTES) {
			reply({ error: "Profiler exports are limited to 16 MiB." });
			return;
		}
		const analysis = analyzeProfilerJson(JSON.parse(json));
		if (!analysis.stats.totalSpans) {
			reply({ error: "No completed timing spans were found in this profiler export." });
			return;
		}
		reply({ result: { analysis, comparison: baseline ? compareProfilerCaptures(baseline, analysis) : null } });
	} catch {
		// JSON errors can include raw trace contents; never forward those to app logs.
		reply({ error: "Profiler export is invalid or exceeds the 500,000-event limit. Use a complete Cfx profiler saveJSON export." });
	}
};
