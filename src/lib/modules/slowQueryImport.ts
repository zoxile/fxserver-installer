import { open } from "@tauri-apps/plugin-dialog";
import { readTextFile } from "./system";
import { SLOW_QUERY_LIMITS } from "./slowQueries";

export function isLocalSlowLogPath(path: string) {
	// Accept local POSIX paths without weakening Windows UNC/device/ADS checks.
	const absolute = path.startsWith("/") && !path.startsWith("//")
		|| /^[a-z]:[\\/]/i.test(path) && !path.slice(2).includes(":");
	return absolute && !/[\u0000-\u001f\u007f]/.test(path) && /\.log$/i.test(path);
}

export async function readSelectedSlowLog(isCurrent: () => boolean): Promise<string | null> {
	const path = await open({ directory: false, multiple: false, filters: [{ name: "Slow query logs", extensions: ["log"] }] });
	if (!isCurrent() || path === null) return null;
	if (typeof path !== "string" || !isLocalSlowLogPath(path)) throw new Error("Choose a local .log file.");
	// No task tracker: filenames, contents and read errors must not reach app logs.
	try {
		const content = await readTextFile(path, SLOW_QUERY_LIMITS.bytes);
		return isCurrent() ? content : null;
	} catch {
		throw new Error("Could not read the log. Choose a readable UTF-8 .log file up to 4 MiB.");
	}
}
