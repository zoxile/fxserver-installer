import { CONSOLE_ISSUE_LIMIT, ConsoleIssueIndex, type ConsoleIssueInput } from "./consoleIssues";

export const CONSOLE_ISSUE_POLL_MS = 1000;
export const CONSOLE_ISSUE_PARSE_BATCH = 50;

interface FeedOptions {
	scope: string;
	isCurrent: () => boolean;
	read: (limit: number) => Promise<{ entries: ConsoleIssueInput[] }>;
	publish: (index: ConsoleIssueIndex) => void;
	onBusy: (busy: boolean) => void;
	onError: (failed: boolean) => void;
	schedule: (callback: () => void, delay: number) => number;
	cancel: (handle: number) => void;
	yieldToBrowser: () => Promise<void>;
}

export function createConsoleIssueFeed(options: FeedOptions) {
	const index = new ConsoleIssueIndex();
	let active = true;
	let busy = false;
	let timer: number | undefined;
	const current = () => active && options.isCurrent();

	async function refresh() {
		if (busy || !current()) return;
		if (timer !== undefined) options.cancel(timer);
		timer = undefined;
		busy = true;
		options.onBusy(true);
		try {
			// Full bounded snapshots expose native clear/retention even when no new line arrives.
			// The index only parses the unseen suffix, not this retained prefix.
			const result = await options.read(CONSOLE_ISSUE_LIMIT);
			if (!current()) return;
			let changed = false;
			do {
				if (!current()) return;
				changed = index.update(result.entries, options.scope, CONSOLE_ISSUE_PARSE_BATCH) || changed;
				if (!result.entries.length || index.latestId === result.entries.at(-1)?.id) break;
				await options.yieldToBrowser();
			} while (current());
			if (!current()) return;
			if (changed) options.publish(index);
			options.onError(false);
		} catch {
			if (current()) options.onError(true);
		} finally {
			busy = false;
			if (current()) {
				options.onBusy(false);
				timer = options.schedule(() => { void refresh(); }, CONSOLE_ISSUE_POLL_MS);
			}
		}
	}

	return {
		refresh,
		dispose() {
			active = false;
			if (timer !== undefined) options.cancel(timer);
			index.reset();
		},
	};
}
