import type { ProfilerComparableAnalysis } from "./profilerAnalyzer";
import { MAX_PROFILER_BYTES, PROFILER_ANALYSIS_DEADLINE_MS, type ProfilerAnalysisRequest, type ProfilerAnalysisResult, type ProfilerAnalysisResponse } from "./profilerAnalysisProtocol";
export { MAX_PROFILER_BYTES } from "./profilerAnalysisProtocol";

export function analyzeProfilerCapture(input: string | Blob, options: { signal?: AbortSignal; baseline?: ProfilerComparableAnalysis } = {}): Promise<ProfilerAnalysisResult> {
	return new Promise((resolve, reject) => {
		const { signal } = options;
		if (signal?.aborted) { reject(new DOMException("Profiler analysis cancelled.", "AbortError")); return; }
		// String length is a cheap lower bound. Exact UTF-8 sizing happens in the worker.
		if ((typeof input === "string" ? input.length : input.size) > MAX_PROFILER_BYTES) {
			reject(new Error("Profiler exports are limited to 16 MiB."));
			return;
		}
		let worker: Worker | undefined;
		let deadline: ReturnType<typeof setTimeout> | undefined;
		let settled = false;
		const finish = (result?: ProfilerAnalysisResult, error?: Error) => {
			if (settled) return;
			settled = true;
			clearTimeout(deadline);
			signal?.removeEventListener("abort", abort);
			if (worker) {
				worker.onmessage = null;
				worker.onerror = null;
				worker.onmessageerror = null;
				worker.terminate();
			}
			if (error) reject(error);
			else resolve(result!);
		};
		const abort = () => finish(undefined, new DOMException("Profiler analysis cancelled.", "AbortError"));
		const fail = () => finish(undefined, new Error("Profiler analysis failed. Try a smaller, complete Cfx JSON export."));
		const timeout = () => finish(undefined, new Error("Profiler analysis exceeded the 15-second limit. Try a smaller capture."));
		const expiresAt = Date.now() + PROFILER_ANALYSIS_DEADLINE_MS;
		try {
			worker = new Worker(new URL("./profiler.worker.ts", import.meta.url), { type: "module" });
			worker.onerror = (event) => { event.preventDefault(); fail(); };
			worker.onmessageerror = fail;
			worker.onmessage = (event: MessageEvent<ProfilerAnalysisResponse>) => {
				if (Date.now() >= expiresAt) { timeout(); return; }
				if (event.data && "result" in event.data && event.data.result?.analysis) finish(event.data.result);
				else finish(undefined, new Error(event.data && "error" in event.data ? event.data.error : "Profiler analysis returned an invalid result."));
			};
			signal?.addEventListener("abort", abort, { once: true });
			deadline = setTimeout(timeout, PROFILER_ANALYSIS_DEADLINE_MS);
			worker.postMessage({ input, baseline: options.baseline } satisfies ProfilerAnalysisRequest);
		} catch { fail(); }
	});
}
