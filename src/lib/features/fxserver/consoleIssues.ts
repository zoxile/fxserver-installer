import { redactIncidentText } from "../../core/incidentModel";
import type { FxserverTerminalEntry, FxserverTerminalSegment } from "../../modules/fxserver";

export const CONSOLE_ISSUE_LIMIT = 1000;
export const CONSOLE_ISSUE_PAGE_SIZE = 50;
export const CONSOLE_ISSUE_LINE_LIMIT = 4000;
export const CONSOLE_CONTEXT_RADIUS = 3;
export type IssueLevel = "error" | "warn";

export interface ConsoleIssueInput extends FxserverTerminalEntry {
	level?: string;
	severity?: string;
	resource?: string | null;
}

export interface IssueLine {
	id: number;
	timestamp: string;
	stream: string;
	text: string;
	segments: FxserverTerminalSegment[];
	level: IssueLevel | null;
	resource: string;
	message: string;
	signature: string;
}

export interface ConsoleIssue {
	id: string;
	level: IssueLevel;
	resource: string;
	message: string;
	count: number;
	first: string;
	last: string;
	firstId: number;
	lastId: number;
	search: string;
}

export interface IssueContext {
	lines: IssueLine[];
	anchorId: number;
	previousId: number | null;
	nextId: number | null;
	position: number;
	count: number;
}

function plainText(value: string): string {
	return value.slice(0, CONSOLE_ISSUE_LINE_LIMIT)
		.replace(/\x1b\][^\x07]*(?:\x07|$)/g, "")
		.replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, "")
		.replace(/\^[0-9]/g, "");
}

function redactConsoleText(value: string): string {
	// Avoid the shared credential matcher scanning long ordinary words at every offset.
	const sensitive = /:\/\/|-----BEGIN|cfxk_|gh[pousr]_|github_pat_|sk-|eyJ|password|passwd|dbpass|pwd|secret|token|license.?key|api.?key|connection.?string|authorization|cookie|webhook|credential|default_account|identified\s+by|bearer/i;
	return sensitive.test(value) ? redactIncidentText(value, CONSOLE_ISSUE_LINE_LIMIT)
		: value.replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f]/g, "");
}

export function normalizeIssueMessage(message: string): string {
	return message
		.replace(/\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b/gi, "<id>")
		.replace(/\b0x[0-9a-f]+\b/gi, "<address>")
		.replace(/\b\d+(?:\.\d+)?\s*(ms|milliseconds|seconds)\b/gi, "<duration> $1")
		.replace(/\b((?:player|client|request|session)\s+(?:id\s*)?[#:=]?\s*)\d+\b/gi, "$1<id>")
		.replace(/\s+/g, " ").trim().toLowerCase();
}

function validTimestamp(value: string): string {
	if (!value.trim()) return "";
	const date = new Date(/^\d+(?:\.\d+)?$/.test(value) ? Number(value) * 1000 : value);
	return Number.isFinite(date.getTime()) ? date.toISOString() : "";
}

export function parseIssueLine(entry: ConsoleIssueInput): IssueLine {
	const original = plainText(entry.plainLine ?? (entry.segments?.length ? entry.segments.map((segment) => segment.text).join("") : entry.line));
	const command = entry.stream === "command";
	const text = command ? "[command omitted]" : redactConsoleText(original);
	let message = text.trim();
	let resource = "";
	// FXServer prefixes are padded, and txAdmin can prepend a timestamp before them.
	message = message.replace(/^\[\s*\d{1,4}[-/:T\d .Z+]*\]\s*/, "");
	for (let index = 0; index < 3; index++) {
		const prefix = /^\[\s*([^\]]{1,160})\]\s*/.exec(message);
		if (!prefix || /^(?:error|warn(?:ing)?|fatal|info|debug|trace|success)$/i.test(prefix[1].trim())) break;
		const match = /^(?:script|resources?)\s*:\s*(.+)$/i.exec(prefix[1].trim());
		if (match) resource = match[1].trim();
		message = message.slice(prefix[0].length);
	}
	resource = entry.resource ? redactConsoleText(plainText(entry.resource)).slice(0, 160).trim() : resource;
	resource ||= /(?:^|\s)@([^\s/\\:]+)[/\\]/.exec(message)?.[1] ?? "Server";
	// Leading level markers outrank words such as "failed" in the message body.
	const marker = /^(?:\[\s*(error|warn(?:ing)?|fatal|info|debug|trace|success)\s*\]|(?:script\s+)?(error|warn(?:ing)?|fatal|info|debug|trace|success)\s*:)/i.exec(message);
	const declaredLevel = entry.severity ?? entry.level ?? (marker?.[1] ?? marker?.[2])?.toLowerCase();
	const level: IssueLevel | null = command ? null
		: declaredLevel === "error" || declaredLevel === "fatal" ? "error"
		: declaredLevel === "warn" || declaredLevel === "warning" ? "warn"
		: declaredLevel ? null
		: /\b(script error|error|exception|failed|fatal|could not)\b/i.test(message) ? "error"
		: /\b(warn|warning)\b/i.test(message) ? "warn" : null;
	const sourceSegments = entry.segments ?? [];
	// Redact the whole line before retaining tokens, including credentials split across colors.
	const keepTokens = !command && text === original && sourceSegments.length <= 80
		&& sourceSegments.map((segment) => segment.text).join("") === text;
	const segments = keepTokens ? sourceSegments.map((segment) => ({
		text: segment.text,
		color: segment.color && /^#[\da-f]{6}$/i.test(segment.color) ? segment.color : null,
		emphasis: Boolean(segment.emphasis),
	})) : [{ text }];
	return { id: entry.id, timestamp: validTimestamp(entry.timestamp),
		stream: ["stdout", "stderr", "system", "command", "rcon"].includes(entry.stream) ? entry.stream : "system",
		text, segments, level, resource: resource.slice(0, 160), message,
		signature: normalizeIssueMessage(message) };
}

interface Group {
	id: string;
	key: string;
	level: IssueLevel;
	resource: string;
	message: string;
	search: string;
	occurrences: Map<number, string>;
}

/** Consumes ordered immutable console snapshots. Only unseen IDs are parsed. */
export class ConsoleIssueIndex {
	private scope: string | null = null;
	private lines = new Map<number, IssueLine>();
	private groups = new Map<string, Group>();
	private memberships = new Map<number, string>();
	private sequence = 0;
	private lastId: number | null = null;
	private cached: ConsoleIssue[] | null = null;
	private parsed = 0;
	readonly limit: number;

	constructor(limit = CONSOLE_ISSUE_LIMIT, private parse = parseIssueLine) {
		this.limit = Math.max(1, Math.min(CONSOLE_ISSUE_LIMIT, Math.floor(limit) || CONSOLE_ISSUE_LIMIT));
	}

	get size() { return this.lines.size; }
	get parsedCount() { return this.parsed; }
	get latestId() { return this.lastId; }

	reset() {
		this.lines.clear();
		this.groups.clear();
		this.memberships.clear();
		this.cached = null;
		this.lastId = null;
	}

	update(entries: readonly ConsoleIssueInput[], scope: string, batchSize = this.limit): boolean {
		let changed = false;
		if (scope !== this.scope) { this.reset(); this.scope = scope; changed = true; }
		const start = Math.max(0, entries.length - this.limit);
		const first = entries[start];
		const last = entries.at(-1);
		if (!first || !last) {
			changed ||= this.size > 0;
			this.reset();
			return changed;
		}
		if (this.lastId !== null && (last.id < this.lastId || first.id > this.lastId)) {
			this.reset();
			changed = true;
		}
		for (const id of this.lines.keys()) {
			if (id >= first.id) break;
			this.evict(id);
			changed = true;
		}
		// Binary search skips the retained prefix without reading or reparsing its text.
		let low = start, high = entries.length;
		while (low < high) {
			const middle = (low + high) >>> 1;
			if (this.lastId !== null && entries[middle].id <= this.lastId) low = middle + 1;
			else high = middle;
		}
		const end = Math.min(entries.length, low + Math.max(1, Math.floor(batchSize) || this.limit));
		for (let index = low; index < end; index++) {
			const input = entries[index];
			if (this.lines.has(input.id)) continue;
			const line = this.parse(input);
			this.parsed++;
			this.lines.set(line.id, line);
			if (line.level) {
				const key = JSON.stringify([line.level, line.resource.toLowerCase(), line.signature]);
				let group = this.groups.get(key);
				if (!group) {
					group = { id: `issue-${++this.sequence}`, key, level: line.level, resource: line.resource,
						message: line.message, search: `${line.level} ${line.resource} ${line.message} ${line.signature}`.toLowerCase(), occurrences: new Map() };
					this.groups.set(key, group);
				}
				group.occurrences.set(line.id, line.timestamp);
				this.memberships.set(line.id, key);
			}
			changed = true;
		}
		this.lastId = entries[end - 1]?.id ?? last.id;
		if (changed) this.cached = null;
		return changed;
	}

	private evict(id: number) {
		const key = this.memberships.get(id);
		const group = key ? this.groups.get(key) : undefined;
		if (group) {
			group.occurrences.delete(id);
			if (!group.occurrences.size) this.groups.delete(group.key);
			else {
				const firstId = group.occurrences.keys().next().value!;
				const first = this.lines.get(firstId)!;
				group.message = first.message;
				group.search = `${group.level} ${group.resource} ${first.message} ${first.signature}`.toLowerCase();
			}
		}
		this.memberships.delete(id);
		this.lines.delete(id);
	}

	issues(): readonly ConsoleIssue[] {
		if (!this.cached) {
			this.cached = Array.from(this.groups.values(), (group) => {
				const first = group.occurrences.entries().next().value!;
				const last = Array.from(group.occurrences.entries()).at(-1)!;
				return { id: group.id, level: group.level, resource: group.resource, message: group.message,
					search: group.search, count: group.occurrences.size, first: first[1], last: last[1], firstId: first[0], lastId: last[0] };
			}).sort((a, b) => b.lastId - a.lastId);
		}
		return this.cached;
	}

	context(groupId: string, occurrenceId?: number): IssueContext | null {
		const group = Array.from(this.groups.values()).find((item) => item.id === groupId);
		if (!group) return null;
		const ids = Array.from(group.occurrences.keys());
		const position = occurrenceId === undefined ? ids.length - 1 : ids.indexOf(occurrenceId);
		if (position < 0) return null;
		const anchorId = ids[position];
		const lines = Array.from(this.lines.values());
		const anchor = lines.findIndex((line) => line.id === anchorId);
		return { lines: lines.slice(Math.max(0, anchor - CONSOLE_CONTEXT_RADIUS), anchor + CONSOLE_CONTEXT_RADIUS + 1),
			anchorId, previousId: ids[position - 1] ?? null, nextId: ids[position + 1] ?? null, position: position + 1, count: ids.length };
	}
}

export function filterConsoleIssues(issues: readonly ConsoleIssue[], query: string, level: IssueLevel | "all" = "all"): readonly ConsoleIssue[] {
	const terms = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
	return issues.filter((issue) => (level === "all" || issue.level === level) && terms.every((term) => issue.search.includes(term)));
}

export function issueTime(value: string): string {
	return value ? new Date(value).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", second: "2-digit" }) : "Unknown";
}
