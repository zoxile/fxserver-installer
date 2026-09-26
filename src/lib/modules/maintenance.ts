export type MaintenanceStepId = "validate" | "announce" | "players" | "stop" | "backup" | "updates" | "preflight" | "start" | "health";
export interface MaintenanceStep {
	id: MaintenanceStepId;
	label: string;
	status: "pending" | "running" | "passed" | "failed";
	detail: string;
}
export interface MaintenanceRun {
	status: "idle" | "running" | "completed" | "paused";
	cancelRequested: boolean;
	steps: MaintenanceStep[];
	error: string;
	startedAt: number;
	finishedAt: number | null;
}

export function createMaintenanceRun(announce: boolean, waitForPlayers: boolean): MaintenanceRun {
	const steps: [MaintenanceStepId, string][] = [["validate", "Verify reviewed plan"]];
	if (announce) steps.push(["announce", "Announce maintenance"]);
	if (waitForPlayers) steps.push(["players", "Wait for an empty server"]);
	steps.push(["stop", "Stop managed server"], ["backup", "Back up selected databases"], ["updates", "Apply reviewed resource updates"],
		["preflight", "Check server readiness"], ["start", "Start server"], ["health", "Verify startup"]);
	return { status: "idle", cancelRequested: false, steps: steps.map(([id, label]) => ({ id, label, status: "pending", detail: "" })), error: "", startedAt: 0, finishedAt: null };
}

export async function executeMaintenance(run: MaintenanceRun, actions: Record<MaintenanceStepId, () => Promise<string>>, checkContext: () => void) {
	if (run.status !== "idle") throw new Error("Review a new plan before running maintenance again.");
	run.status = "running";
	run.startedAt = Date.now();
	try {
		for (const step of run.steps) {
			if (run.cancelRequested) throw new Error("Stopped at a safe boundary. Review the completed steps before continuing manually.");
			checkContext();
			step.status = "running";
			try { step.detail = await actions[step.id](); step.status = "passed"; }
			catch (error) { step.status = "failed"; throw error; }
		}
		run.status = "completed";
	} catch (error) {
		run.status = "paused";
		run.error = error instanceof Error ? error.message : String(error);
		throw error;
	} finally { run.finishedAt = Date.now(); }
}

export function sameMaintenancePath(a: string, b: string) {
	return a.trim().replaceAll("\\", "/").replace(/\/+$/, "").toLowerCase() === b.trim().replaceAll("\\", "/").replace(/\/+$/, "").toLowerCase();
}
