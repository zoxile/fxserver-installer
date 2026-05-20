import { invoke } from "@tauri-apps/api/core";

function hasTauriRuntime() {
	return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function readTextFile(path: string, maxBytes = 10 * 1024 * 1024) {
	if (!hasTauriRuntime()) {
		throw new Error("File reading is available in the Tauri desktop app.");
	}

	return invoke<string>("read_text_file", { path, maxBytes });
}
