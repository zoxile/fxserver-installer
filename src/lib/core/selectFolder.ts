import { open } from "@tauri-apps/plugin-dialog";
import { setInstallPath } from "./paths.svelte";

export async function chooseInstallFolder() {
	const selected = await open({
		directory: true,
		multiple: false,
	});

	if (selected) {
		setInstallPath(selected as string);
	}
}
