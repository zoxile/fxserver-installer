import { invoke } from "@tauri-apps/api/core";
import { trackTask } from "$lib/core/tasks.svelte";
import type { DependencyInventory, DependencyRequest } from "./resourceDependencies";

export function scanResourceDependencies(request: DependencyRequest): Promise<DependencyInventory> {
	return trackTask("scan_resource_dependencies", "Inspect resource dependencies", () => invoke("scan_resource_dependencies", { request }));
}
