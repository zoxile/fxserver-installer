import type { ResourceTarget, ResourceUpdatePreview, ResourceSnapshot } from "./resourceUpdates";

export interface ResourcePreference { pinnedVersion: string | null; ignored: boolean }
export type ResourcePreferences = Record<string, ResourcePreference>;
export interface ReviewedUpdate {
	id: string;
	target: ResourceTarget;
	name: string;
	preview: ResourceUpdatePreview;
	protectedPaths: string[];
	status: "ready" | "applying" | "completed" | "failed" | "cancelled";
	outcome: string;
	reviewedAt: number;
	snapshotId?: string;
}
export interface ResourcePlan {
	entries: ReviewedUpdate[];
	revision: number;
	status: "idle" | "running" | "paused" | "stopped" | "completed";
	pauseRequested: boolean;
	stopRequested: boolean;
	error: string;
}

export function resourcePreferenceKey(target: ResourceTarget) {
	const path = (value: string) => value.trim().replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
	return JSON.stringify([path(target.txDataPath), target.profile.toLowerCase(), path(target.resourcePath)]);
}

export function parseResourcePreferences(raw: string | null): ResourcePreferences {
	if (!raw) return {};
	const data: unknown = JSON.parse(raw);
	if (!data || typeof data !== "object" || Array.isArray(data) || Object.keys(data).length > 10000) throw new Error("Saved resource preferences are invalid.");
	const result: ResourcePreferences = Object.create(null);
	for (const [key, value] of Object.entries(data)) {
		if (!value || typeof value !== "object" || !("ignored" in value) || typeof value.ignored !== "boolean" || !("pinnedVersion" in value)
			|| !(value.pinnedVersion === null || typeof value.pinnedVersion === "string" && value.pinnedVersion.length <= 200)) throw new Error("Saved resource preferences are invalid.");
		result[key] = { ignored: value.ignored, pinnedVersion: value.pinnedVersion };
	}
	return result;
}

export function protectedResourcePaths(preview: ResourceUpdatePreview, selected: string[]) {
	const allowed = new Set(preview.changes.filter((file) => file.canPreserve).map((file) => file.path));
	return [...new Set([...preview.changes.filter((file) => file.preserve).map((file) => file.path), ...selected])].filter((path) => allowed.has(path));
}

export function newResourcePlan(): ResourcePlan {
	return { entries: [], revision: 0, status: "idle", pauseRequested: false, stopRequested: false, error: "" };
}

export function addReviewedUpdate(plan: ResourcePlan, target: ResourceTarget, name: string, preview: ResourceUpdatePreview, selected: string[], preference: ResourcePreference) {
	if (plan.status === "running") throw new Error("Pause the update queue before adding another review.");
	if (preference.ignored || preference.pinnedVersion !== null) throw new Error("Unpin this resource and remove Ignore before queuing an update.");
	if (plan.entries.some((entry) => ["ready", "applying"].includes(entry.status) && resourcePreferenceKey(entry.target) === resourcePreferenceKey(target))) throw new Error("This resource already has a reviewed update in the queue.");
	if (plan.entries.filter((entry) => entry.status === "ready").length >= 7) throw new Error("Apply or remove a queued update before reviewing more (7 pending updates maximum).");
	if (Date.now() - preview.createdAt * 1000 >= 30 * 60 * 1000) throw new Error("This preview expired. Review the resource again.");
	plan.entries.push({ id: preview.id, target: { ...target }, name, preview: { ...preview, changes: preview.changes.map((file) => ({ ...file })) },
		protectedPaths: protectedResourcePaths(preview, selected), status: "ready", outcome: "Individually reviewed", reviewedAt: Date.now() });
	if (plan.status === "completed" || plan.status === "stopped") {
		plan.status = "idle"; plan.pauseRequested = false; plan.stopRequested = false; plan.error = "";
	}
}

export async function runReviewedUpdates(plan: ResourcePlan, workspaceId: string, dependencies: {
	preference: (target: ResourceTarget) => ResourcePreference;
	apply: (target: ResourceTarget, previewId: string, protectedPaths: string[]) => Promise<ResourceSnapshot>;
	discard: (previewId: string) => Promise<void>;
}) {
	if (plan.status === "running") throw new Error("The update queue is already running.");
	plan.status = "running"; plan.pauseRequested = false; plan.stopRequested = false; plan.error = "";
	for (const entry of plan.entries) {
		if (plan.stopRequested || plan.pauseRequested) break;
		if (entry.status !== "ready") continue;
		try {
			if (entry.target.workspaceId !== workspaceId) throw new Error("This review belongs to another workspace.");
			const preference = dependencies.preference(entry.target);
			if (preference.ignored || preference.pinnedVersion !== null) throw new Error("Resource is pinned or ignored. Review it again after changing its preference.");
			if (Date.now() - entry.preview.createdAt * 1000 >= 30 * 60 * 1000) throw new Error("Preview expired. Review this resource again.");
			entry.status = "applying"; entry.outcome = "Applying reviewed archive";
			const snapshot = await dependencies.apply(entry.target, entry.preview.id, protectedResourcePaths(entry.preview, entry.protectedPaths));
			entry.status = "completed"; entry.snapshotId = snapshot.id;
			entry.outcome = `Updated; snapshot ${snapshot.id}`;
			plan.revision++;
		} catch (caught) {
			entry.status = "failed"; entry.outcome = String(caught); plan.error = `${entry.name}: ${entry.outcome}`;
			plan.pauseRequested = true;
			await dependencies.discard(entry.preview.id).catch(() => {});
			break;
		}
	}
	if (plan.stopRequested) {
		for (const entry of plan.entries.filter((entry) => entry.status === "ready")) {
			entry.status = "cancelled"; entry.outcome = "Stopped before apply";
			await dependencies.discard(entry.preview.id).catch(() => {});
		}
		plan.status = "stopped";
	} else if (plan.pauseRequested) plan.status = "paused";
	else plan.status = "completed";
}
