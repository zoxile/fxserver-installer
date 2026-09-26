import { parseSlowQueryLog } from "$lib/modules/slowQueries";

self.onmessage = (event: MessageEvent<string>) => {
	try {
		self.postMessage({ report: parseSlowQueryLog(event.data) });
	} catch {
		self.postMessage({ error: "The log is oversized or contains unsupported control characters. Import a UTF-8 .log file up to 4 MiB." });
	}
};
