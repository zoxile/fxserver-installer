import { invoke } from "@tauri-apps/api/core";

export interface ClientLogRequest {
	directory?: string | null;
	fileName?: string | null;
	maxLines?: number;
}

export interface ClientLogFile {
	name: string;
	path: string;
	size: number;
	modified?: number | null;
}

export interface ClientLogResult {
	directory: string;
	files: ClientLogFile[];
	selectedFile?: string | null;
	path?: string | null;
	content: string;
	lineCount: number;
}

function hasTauriRuntime() {
	return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function readClientLogs(request: ClientLogRequest) {
	if (!hasTauriRuntime()) {
		return {
			directory: "C:\\Users\\Zox\\AppData\\Local\\FiveM\\FiveM.app\\logs",
			files: [],
			selectedFile: null,
			path: null,
			content: "",
			lineCount: 0,
		} satisfies ClientLogResult;
	}

	return invoke<ClientLogResult>("read_client_logs", { request });
}
