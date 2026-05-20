import { open } from "@tauri-apps/plugin-dialog";

export async function chooseSqlFile(defaultPath?: string | null) {
	const selected = await open({
		directory: false,
		multiple: false,
		defaultPath: defaultPath || undefined,
		filters: [{ name: "SQL files", extensions: ["sql"] }],
	});

	return selected ? (selected as string) : null;
}
