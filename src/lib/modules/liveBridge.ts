import { taskInvoke } from "$lib/core/tasks.svelte";

export interface BridgeTarget { workspaceId: string; txDataPath: string; profile: string; port: number }
export interface BridgeResource { name: string; state: "missing" | "started" | "starting" | "stopped" | "stopping" | "uninitialized" | "unknown"; version: string }
export interface BridgeSnapshot {
	protocol: number; version: string; instanceId: string; timestamp: number;
	uptimeSeconds: number; schedulerDelayMs: number; hostname: string; gameBuild: string; onesync: string;
	maxPlayers: number; playerCount: number; resourceCount: number;
	resources: BridgeResource[];
	players: { id: string; name: string; ping: number }[];
	events: { id: number; timestamp: number; kind: string; resource: string }[];
}
export interface BridgeStatus {
	workspaceId: string; enabled: boolean; connected: boolean; receivedAt: number | null;
	error: string | null; snapshot: BridgeSnapshot | null;
}
export interface BridgeInstallation {
	workspaceId: string; installed: boolean; managed: boolean; resourcePath: string;
	version: string | null; cfgEnabled: boolean; keyAvailable: boolean; warning: string | null;
}
export interface BridgePreview { id: string; remove: boolean; resourcePath: string; files: string[]; configLines: string[]; expiresInSeconds: number }
export type BridgeAction = "start" | "stop" | "restart" | "ensure";

export const inspectBridge = (target: BridgeTarget) => taskInvoke<BridgeInstallation>("get_live_bridge_installation", { target });
export const previewBridge = (target: BridgeTarget, remove: boolean) => taskInvoke<BridgePreview>("preview_live_bridge_change", { target, remove });
export const applyBridge = (previewId: string) => taskInvoke<BridgeInstallation>("apply_live_bridge_change", { previewId });
export const configureBridge = (target: BridgeTarget, enabled: boolean) => taskInvoke<BridgeStatus>("configure_live_bridge", { target, enabled });
export const getBridgeStatus = () => taskInvoke<BridgeStatus>("get_live_bridge_status");
export const sendBridgeAction = (workspaceId: string, action: BridgeAction, resource: string) => taskInvoke<void>("send_live_bridge_action", { workspaceId, action, resource });
