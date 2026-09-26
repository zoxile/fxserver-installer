import type { ProfilerAnalysis, ProfilerComparableAnalysis, ProfilerComparison } from "./profilerAnalyzer";

export const MAX_PROFILER_BYTES = 16 * 1024 * 1024;
export const PROFILER_ANALYSIS_DEADLINE_MS = 15_000;
export type ProfilerAnalysisRequest = { input: string | Blob; baseline?: ProfilerComparableAnalysis };
export type ProfilerAnalysisResult = { analysis: ProfilerAnalysis; comparison: ProfilerComparison | null };
export type ProfilerAnalysisResponse = { result: ProfilerAnalysisResult } | { error: string };
