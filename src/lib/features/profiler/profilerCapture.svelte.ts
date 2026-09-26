import { invoke } from "@tauri-apps/api/core";
import { trackTask, taskSession } from "$lib/core/tasks.svelte";
import { getWorkspaceId } from "$lib/core/workspaces.svelte";
import { getSavedFxserverRconPassword } from "$lib/modules/fxserver";
import { readSavedEnvironment } from "$lib/features/fxserver/fxserverSettings.svelte";
import type { ProfilerAnalysis, ProfilerComparison } from "./profilerAnalyzer";
import { analyzeProfilerCapture } from "./profilerAnalysis";

export { MAX_PROFILER_BYTES } from "./profilerAnalysis";
export const CAPTURE_FRAME_OPTIONS = [100, 250, 500] as const;

type CaptureStatus = {
	id: string;
	frames: number;
	phase: string;
	capturedFrames: number;
	cancelRequested: boolean;
};

type CaptureResult = { id: string; fileName: string; frames: number; json: string };
export type ProfilerCapture = {
	id: string;
	name: string;
	workspaceId: string;
	createdAt: number;
	source: "guided" | "import";
	analysis: ProfilerAnalysis;
};

class ProfilerCaptureSession {
	busy = $state(false);
	analyzing = $state(false);
	workspaceId = $state("");
	activeId = $state("");
	phase = $state("idle");
	requestedFrames = $state(250);
	capturedFrames = $state(0);
	cancelRequested = $state(false);
	error = $state("");
	captures = $state.raw<ProfilerCapture[]>([]);
	comparison = $state.raw<ProfilerComparison | null>(null);
	selectedId = $state("");
	openRequest = $state(0);
}
export const profilerCaptureSession = new ProfilerCaptureSession();

let generation = 0;
let cancelSent = false;
let nativeCapturePending = false;
let captureAnalysisAbort: AbortController | undefined;

export function selectProfilerCapture(id: string) {
	if (profilerCaptureSession.captures.some((capture) => capture.id === id)) {
		profilerCaptureSession.selectedId = id;
	}
}

export async function importProfilerCapture(input: string | Blob, name: string, options: { id?: string; workspaceId?: string; source?: "guided" | "import"; signal?: AbortSignal } = {}) {
	if (profilerCaptureSession.analyzing || (profilerCaptureSession.busy && options.source !== "guided")) {
		throw new Error("Wait for the current profiler capture or analysis to finish.");
	}
	const id = options.id ?? crypto.randomUUID();
	const workspaceId = options.workspaceId ?? getWorkspaceId();
	const previous = profilerCaptureSession.captures.filter((capture) => capture.id !== id).at(-1);
	profilerCaptureSession.analyzing = true;
	try {
		const { analysis, comparison } = await analyzeProfilerCapture(input, {
			signal: options.signal,
			baseline: previous ? { stats: previous.analysis.stats, resources: previous.analysis.resources } : undefined,
		});
		if (options.signal?.aborted) throw new DOMException("Profiler analysis cancelled.", "AbortError");
		const capture: ProfilerCapture = { id, name, workspaceId, createdAt: Date.now(), source: options.source ?? "import", analysis };
		profilerCaptureSession.captures = previous ? [previous, capture] : [capture];
		profilerCaptureSession.comparison = comparison;
		profilerCaptureSession.selectedId = capture.id;
		return capture;
	} finally { profilerCaptureSession.analyzing = false; }
}

async function sendPendingCancellation(token: number) {
	if (token !== generation || !nativeCapturePending || !profilerCaptureSession.cancelRequested || !profilerCaptureSession.activeId || cancelSent) return;
	cancelSent = true;
	try {
		await invoke("cancel_profiler_capture", { id: profilerCaptureSession.activeId });
	} catch (error) {
		if (token !== generation || !profilerCaptureSession.busy) return;
		cancelSent = false;
		profilerCaptureSession.error = `Cancellation not confirmed: ${String(error)}`;
	}
}

async function pollProgress(token: number) {
	if (token !== generation || !nativeCapturePending) return;
	try {
		const status = await invoke<CaptureStatus | null>("get_profiler_capture_status");
		if (token !== generation || !nativeCapturePending) return;
		if (status) {
			profilerCaptureSession.activeId = status.id;
			profilerCaptureSession.phase = status.phase;
			profilerCaptureSession.capturedFrames = status.capturedFrames;
			await sendPendingCancellation(token);
		}
	} catch {
		// The long-running command is authoritative; a missed progress poll cannot end its task.
	} finally {
		if (token === generation && nativeCapturePending) setTimeout(() => void pollProgress(token), 500);
	}
}

export async function cancelProfilerCapture() {
	if (!profilerCaptureSession.busy) return;
	profilerCaptureSession.cancelRequested = true;
	captureAnalysisAbort?.abort();
	await sendPendingCancellation(generation);
}

export async function startProfilerCapture(frames: number) {
	if (profilerCaptureSession.busy) return;
	profilerCaptureSession.error = "";
	if (profilerCaptureSession.analyzing) {
		profilerCaptureSession.error = "Wait for the current profiler analysis to finish.";
		return;
	}
	if (!CAPTURE_FRAME_OPTIONS.some((option) => option === frames)) {
		profilerCaptureSession.error = "Choose 100, 250, or 500 frames.";
		return;
	}
	if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
		profilerCaptureSession.error = "Guided recording is available in the desktop app.";
		return;
	}
	if (taskSession.switching) {
		profilerCaptureSession.error = "Wait for the workspace switch to finish.";
		return;
	}
	const token = ++generation;
	const workspaceId = getWorkspaceId();
	const environment = readSavedEnvironment();
	const host = environment.TXHOST_RCON_HOST || "127.0.0.1";
	const port = Number(environment.TXHOST_RCON_PORT || "30120");
	Object.assign(profilerCaptureSession, { busy: true, workspaceId, activeId: "", phase: "checking", requestedFrames: frames, capturedFrames: 0, cancelRequested: false });
	cancelSent = false;
	captureAnalysisAbort = new AbortController();
	try {
		await trackTask("capture_fxserver_profile", "Record FXServer performance", async () => {
			const password = await getSavedFxserverRconPassword(workspaceId);
			if (profilerCaptureSession.cancelRequested) throw new DOMException("Profiler capture cancelled.", "AbortError");
			if (!password) throw new Error("Save the RCON password in Manage Server before recording.");
			if (getWorkspaceId() !== workspaceId) throw new Error("Workspace changed before recording.");
			nativeCapturePending = true;
			const pending = invoke<CaptureResult>("capture_fxserver_profile", { request: { frames, rcon: { host, port, password } } });
			void pollProgress(token);
			let result: CaptureResult;
			try { result = await pending; }
			catch (error) {
				if (String(error).startsWith("Profiler capture cancelled.")) throw new DOMException(String(error), "AbortError");
				throw error;
			} finally { nativeCapturePending = false; profilerCaptureSession.activeId = ""; }
			profilerCaptureSession.phase = "analyzing";
			await importProfilerCapture(result.json, result.fileName, { id: result.id, workspaceId, source: "guided", signal: captureAnalysisAbort?.signal });
			profilerCaptureSession.error = "";
			profilerCaptureSession.phase = "completed";
			profilerCaptureSession.openRequest += 1;
		});
	} catch (error) {
		profilerCaptureSession.phase = error instanceof DOMException && error.name === "AbortError" ? "cancelled" : "failed";
		profilerCaptureSession.error = error instanceof Error ? error.message : String(error);
	} finally {
		captureAnalysisAbort?.abort();
		captureAnalysisAbort = undefined;
		nativeCapturePending = false;
		generation += 1;
		profilerCaptureSession.busy = false;
		profilerCaptureSession.activeId = "";
	}
}
