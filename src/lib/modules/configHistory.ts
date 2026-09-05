import { invoke } from "@tauri-apps/api/core";
import { trackTask } from "$lib/core/tasks.svelte";
import type { ServerConfigFile } from "$lib/modules/fxserver";

export interface ConfigFileRequest { txDataPath: string; profile: string; path: string }
export interface ConfigHistoryVersion { id: string; createdAt: number; reason: string; size: number; digest: string }
export interface ConfigHistoryContent { version: ConfigHistoryVersion; content: string }

function run<T>(command: string, label: string, args: Record<string, unknown>): Promise<T> {
    return trackTask(command, label, () => invoke<T>(command, args));
}

function configFileName(path: string): string {
    return path.split(/[\\/]/).pop() || "configuration";
}

export function saveConfigWithHistory(request: ConfigFileRequest, expectedContent: string, content: string): Promise<ServerConfigFile> {
    return run("save_server_config", `Save ${configFileName(request.path)}`, {
        request: { path: request.path, content }, txDataPath: request.txDataPath, profile: request.profile, expectedContent,
    });
}

export function readConfigHistoryFile(request: ConfigFileRequest): Promise<ServerConfigFile> {
    return run("read_config_history_file", "Read server configuration", { request });
}

export function listConfigHistory(request: ConfigFileRequest): Promise<ConfigHistoryVersion[]> {
    return run("list_config_history", "Read configuration history", { request });
}

export function readConfigHistoryVersion(request: ConfigFileRequest, versionId: string): Promise<ConfigHistoryContent> {
    return run("read_config_history_version", "Review configuration version", { request, versionId });
}

export function restoreConfigHistoryVersion(request: ConfigFileRequest, versionId: string, expectedContent: string): Promise<ServerConfigFile> {
    return run("restore_config_history_version", `Restore ${configFileName(request.path)}`, { request, versionId, expectedContent });
}
