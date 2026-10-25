import { taskInvoke } from "$lib/core/tasks.svelte";

export type FirewallBackend = "auto" | "windows" | "ufw" | "firewalld" | "unsupported";
export type FirewallRuleState = "owned" | "external" | "defaultAllowed" | "missing" | "blocked" | "unknown";
export interface FirewallRequest { backend: FirewallBackend; tcpPort: number; udpPort: number }
export interface FirewallCheck {
	direction: "inbound" | "outbound";
	protocol: "tcp" | "udp";
	port: number;
	state: FirewallRuleState;
	ownedRulePresent: boolean;
	detail: string;
}
export interface FirewallSnapshot {
	backend: FirewallBackend;
	active: boolean | null;
	requiresElevation: boolean;
	canConfigure: boolean;
	canRemove: boolean;
	reloadRequired: boolean;
	checks: FirewallCheck[];
	notices: string[];
}
export interface FirewallPorts { tcpPorts: number[]; udpPorts: number[]; warnings: string[] }

export const firewallStateLabels: Record<FirewallRuleState, string> = {
	owned: "App rule", external: "Existing rule", defaultAllowed: "Allowed by policy",
	missing: "Rule missing", blocked: "Blocked", unknown: "Needs review",
};
export function validFirewallPort(value: number | undefined): value is number {
	return Number.isInteger(value) && value! >= 1 && value! <= 65535;
}
export function getFirewallPorts(txDataPath: string, profile: string): Promise<FirewallPorts> {
	return taskInvoke("get_fivem_firewall_ports", { request: { txDataPath, profile } });
}
export function inspectFirewall(request: FirewallRequest, elevated = false): Promise<FirewallSnapshot> {
	return taskInvoke("inspect_fivem_firewall", { request, elevated });
}
export function configureFirewall(request: FirewallRequest): Promise<FirewallSnapshot> {
	return taskInvoke("configure_fivem_firewall", { request });
}
export function removeFirewall(request: FirewallRequest): Promise<FirewallSnapshot> {
	return taskInvoke("remove_fivem_firewall", { request });
}
