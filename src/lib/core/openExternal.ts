import { invoke } from "@tauri-apps/api/core";
import { log } from "$lib/core/logger";

function hasTauriRuntime() {
	return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function openExternalUrl(url: string) {
	log(`Opening external link: ${url}`, { level: "debug", scope: "system.links" });

	if (hasTauriRuntime()) {
		await invoke<void>("open_external_url", { url });
		return;
	}

	window.open(url, "_blank", "noopener,noreferrer");
}
