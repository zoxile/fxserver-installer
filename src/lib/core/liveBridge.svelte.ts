import { configureBridge, type BridgeSnapshot, type BridgeStatus, type BridgeTarget } from "$lib/modules/liveBridge";
import { appendIncidents } from "./incidents.svelte";

const preferenceKey = "fxserver-installer.live-bridge.v1";
let syncRevision = 0;
let eventInstance = "";
let eventCursor = 0;

class BridgeSession {
	workspaceId = $state("default");
	enabled = $state(false);
	connected = $state(false);
	receivedAt = $state<number | null>(null);
	error = $state("");
	snapshot = $state.raw<BridgeSnapshot | null>(null);
	preferenceRevision = $state(0);
}
export const bridgeSession = new BridgeSession();

function preferences(): Record<string, { enabled: boolean; port: number }> {
	try {
		const raw = JSON.parse(localStorage.getItem(preferenceKey) || "{}");
		return raw && typeof raw === "object" && !Array.isArray(raw) ? raw : {};
	} catch { return {}; }
}

export function bridgePreference(workspaceId: string) {
	const entry = preferences()[workspaceId];
	return { enabled: entry?.enabled === true, port: Number.isInteger(entry?.port) && entry.port > 0 && entry.port <= 65535 ? entry.port : 30120 };
}

export function saveBridgePreference(workspaceId: string, enabled: boolean, port: number) {
	const saved = preferences();
	saved[workspaceId] = { enabled, port };
	localStorage.setItem(preferenceKey, JSON.stringify(saved));
	bridgeSession.preferenceRevision++;
}

export function acceptBridgeStatus(status: BridgeStatus) {
	if (status.workspaceId !== bridgeSession.workspaceId) return;
	bridgeSession.enabled = status.enabled;
	bridgeSession.connected = status.connected;
	bridgeSession.receivedAt = status.receivedAt;
	bridgeSession.error = status.error ?? "";
	bridgeSession.snapshot = status.connected ? status.snapshot : null;
	if (!status.connected || !status.snapshot) return;
	const snapshot = status.snapshot;
	if (eventInstance !== snapshot.instanceId) {
		eventInstance = snapshot.instanceId;
		eventCursor = 0;
	}
	const pending = snapshot.events.filter((event) => event.id > eventCursor);
	appendIncidents(pending.map((event) => ({ id: `bridge:${snapshot.instanceId}:${event.id}`, workspaceId: status.workspaceId,
			timestamp: event.timestamp, type: "resource", level: "info", panel: "resource-manager",
			title: `${event.resource}: ${event.kind.replaceAll("-", " ")}` })));
	if (pending.length) eventCursor = pending[pending.length - 1].id;
}

export async function syncBridge(target: Omit<BridgeTarget, "port">) {
	const revision = ++syncRevision;
	const preference = bridgePreference(target.workspaceId);
	bridgeSession.workspaceId = target.workspaceId;
	bridgeSession.connected = false;
	bridgeSession.snapshot = null;
	bridgeSession.error = "";
	bridgeSession.enabled = preference.enabled;
	eventInstance = "";
	eventCursor = 0;
	if (!("__TAURI_INTERNALS__" in window)) return;
	try {
		const status = await configureBridge({ ...target, port: preference.port }, preference.enabled && Boolean(target.txDataPath && target.profile));
		if (revision === syncRevision) acceptBridgeStatus(status);
	} catch (error) {
		if (revision === syncRevision) bridgeSession.error = String(error);
	}
}
