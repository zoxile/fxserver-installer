import { open } from "@tauri-apps/plugin-dialog";
import { setInstallPath } from "./paths.svelte";

export async function chooseFolder() {
	const selected = await open({
		directory: true,
		multiple: false,
	});

	return selected ? (selected as string) : null;
}

export async function chooseInstallFolder() {
	const selected = await chooseFolder();

	if (selected) {
		setInstallPath(selected);
		return selected;
	}

	return null;
}
