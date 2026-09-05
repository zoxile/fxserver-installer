import { taskSession, trackTask } from "$lib/core/tasks.svelte";
import { fxserverSettings } from "$lib/features/fxserver/fxserverSettings.svelte";
import { applyResourceUpdate, discardResourcePreview, type ResourceTarget, type ResourceUpdatePreview } from "./resourceUpdates";
import { addReviewedUpdate, newResourcePlan, parseResourcePreferences, resourcePreferenceKey, runReviewedUpdates, type ResourcePlan, type ResourcePreference, type ResourcePreferences } from "./resourcePlan";

export const resourcePlanSession = $state({
	plans: {} as Record<string, ResourcePlan>,
	preferences: {} as Record<string, ResourcePreferences>,
	preferenceErrors: {} as Record<string, string>,
});

const storageKey = (workspaceId: string) => `fxserver-installer.resource-preferences.v1.${workspaceId}`;

export function getResourcePlan(workspaceId: string) {
	return resourcePlanSession.plans[workspaceId] ??= newResourcePlan();
}

export function loadResourcePreferences(workspaceId: string) {
	if (resourcePlanSession.preferences[workspaceId]) return;
	try { resourcePlanSession.preferences[workspaceId] = parseResourcePreferences(localStorage.getItem(storageKey(workspaceId))); }
	catch (caught) { resourcePlanSession.preferenceErrors[workspaceId] = String(caught); }
}

export function getResourcePreference(target: ResourceTarget): ResourcePreference {
	if (resourcePlanSession.preferenceErrors[target.workspaceId]) throw new Error("Resource preferences could not be loaded. Updates are blocked to preserve existing pins and ignores.");
	return resourcePlanSession.preferences[target.workspaceId]?.[resourcePreferenceKey(target)] ?? { pinnedVersion: null, ignored: false };
}

export function saveResourcePreference(target: ResourceTarget, preference: ResourcePreference) {
	const plan = getResourcePlan(target.workspaceId);
	if (plan.status === "running") throw new Error("Pause the queue before changing resource preferences.");
	if (resourcePlanSession.preferenceErrors[target.workspaceId]) throw new Error("Saved preferences could not be read. No preferences were overwritten.");
	const next = { ...resourcePlanSession.preferences[target.workspaceId], [resourcePreferenceKey(target)]: { ...preference } };
	localStorage.setItem(storageKey(target.workspaceId), JSON.stringify(next));
	resourcePlanSession.preferences[target.workspaceId] = next;
	if (preference.ignored || preference.pinnedVersion !== null) {
		for (const entry of plan.entries.filter((entry) => entry.status === "ready" && resourcePreferenceKey(entry.target) === resourcePreferenceKey(target))) {
			entry.status = "cancelled"; entry.outcome = "Cancelled by pin/ignore preference";
			void discardResourcePreview(entry.preview.id).catch(() => {});
		}
	}
}

export function queueResourceUpdate(target: ResourceTarget, name: string, preview: ResourceUpdatePreview, protectedPaths: string[]) {
	if (target.workspaceId !== taskSession.workspaceId || taskSession.switching) throw new Error("The active workspace changed. Review the resource again.");
	addReviewedUpdate(getResourcePlan(target.workspaceId), target, name, preview, protectedPaths, getResourcePreference(target));
}

export async function runResourcePlan(workspaceId: string) {
	if (taskSession.workspaceId !== workspaceId || taskSession.switching) throw new Error("Switch to this workspace before applying its reviewed queue.");
	const plan = getResourcePlan(workspaceId);
	if (plan.status === "running") return;
	const path = (value: string) => value.trim().replaceAll("\\", "/").replace(/\/+$/, "").toLowerCase();
	if (plan.entries.some((entry) => entry.status === "ready" && (path(entry.target.txDataPath) !== path(fxserverSettings.txDataPath) || entry.target.profile.toLowerCase() !== fxserverSettings.profile.toLowerCase()))) {
		throw new Error("Server paths changed since this queue was reviewed. Remove its entries and review updates for the current server.");
	}
	await trackTask("apply_resource_plan", "Apply reviewed resource updates", async () => {
		await runReviewedUpdates(plan, workspaceId, { preference: getResourcePreference, apply: applyResourceUpdate, discard: discardResourcePreview });
		if (plan.error) throw new Error(plan.error);
	});
}

export function pauseResourcePlan(workspaceId: string) {
	getResourcePlan(workspaceId).pauseRequested = true;
}

export async function stopResourcePlan(workspaceId: string) {
	const plan = getResourcePlan(workspaceId);
	plan.stopRequested = true;
	if (plan.status === "running") return;
	plan.status = "stopped";
	const pending = plan.entries.filter((entry) => entry.status === "ready");
	for (const entry of pending) {
		entry.status = "cancelled"; entry.outcome = "Stopped before apply";
	}
	for (const entry of pending) {
		await discardResourcePreview(entry.preview.id).catch(() => {});
	}
}

export async function removeResourcePlanEntry(workspaceId: string, id: string) {
	const plan = getResourcePlan(workspaceId);
	if (plan.status === "running") return;
	const entry = plan.entries.find((entry) => entry.id === id);
	if (!entry) return;
	if (entry.status === "ready") {
		entry.status = "cancelled"; entry.outcome = "Removed before apply";
		await discardResourcePreview(entry.preview.id);
	}
	plan.entries = plan.entries.filter((entry) => entry.id !== id);
}
