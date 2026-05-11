import { open } from "@tauri-apps/plugin-dialog";
import { setInstallPath } from "./paths.svelte";

export async function chooseInstallFolder() {
	const selected = await open({
		directory: true,
		multiple: false,
	});

	if (selected) {
		const path = selected as string;
		setInstallPath(path);
		return path;
	}

	return null;
}
