export const SLOW_QUERY_LIMITS = Object.freeze({ bytes: 4 * 1024 * 1024, records: 20_000, groups: 2_000, queryCharacters: 65_536, tokens: 4_096, fingerprintCharacters: 16_384, pageSize: 25 });

export interface SlowQueryGroup {
	id: number;
	database: string | null;
	fingerprint: string;
	count: number;
	totalSeconds: number;
	averageSeconds: number;
	maxSeconds: number;
	rowsExamined: number;
	averageRowsExamined: number;
	maxRowsExamined: number;
	draft: string | null;
	draftReason: string;
}

export interface SlowQueryReport {
	groups: SlowQueryGroup[];
	accepted: number;
	skipped: number;
	truncated: boolean;
	totalSeconds: number;
	rowsExamined: number;
}

export type SlowQuerySort = "totalSeconds" | "count" | "averageSeconds" | "maxSeconds" | "rowsExamined";

const keywords = new Set(("SELECT FROM WHERE JOIN LEFT RIGHT INNER OUTER CROSS ON AS AND OR NOT IN IS NULL TRUE FALSE BETWEEN LIKE REGEXP RLIKE EXISTS DISTINCT ALL UNION ORDER BY ASC DESC GROUP HAVING LIMIT OFFSET WITH RECURSIVE CASE WHEN THEN ELSE END COLLATE BINARY USING USE FORCE IGNORE INDEX KEY PARTITION SQL_NO_CACHE SQL_CALC_FOUND_ROWS INSERT UPDATE DELETE REPLACE CREATE ALTER DROP TRUNCATE SET CALL DO INTO OUTFILE DUMPFILE FOR LOCK SHARE MODE PROCEDURE ANALYZE EXPLAIN RETURNING OVER WINDOW INTERVAL").split(" "));
const word = /[\p{L}\p{N}_$]+/uy;
const number = /(?:0[xX][\da-fA-F]+|0[bB][01]+|(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?)/y;
const controls = /[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f-\u009f\u202a-\u202e\u2066-\u2069]/;

// This lexer makes comparison fingerprints, not executable SQL or a SQL validity claim.
// Literal contents are never retained, including across log lines resembling headers.
class FingerprintLexer {
	tokens: string[] = [];
	mode: "code" | "'" | '"' | "`" | "comment" = "code";
	invalid = false;
	ambiguousQuotes = false;
	private identifier = "";
	private characters = 0;
	private outputCharacters = 0;

	private add(token: string) {
		this.outputCharacters += token.length + 1;
		if (this.tokens.length >= SLOW_QUERY_LIMITS.tokens || this.outputCharacters > SLOW_QUERY_LIMITS.fingerprintCharacters) this.invalid = true;
		if (!this.invalid) this.tokens.push(token);
	}

	feed(line: string) {
		this.characters += line.length;
		if (this.characters > SLOW_QUERY_LIMITS.queryCharacters || controls.test(line)) this.invalid = true;
		for (let i = 0; i < line.length;) {
			const char = line[i];
			const next = line[i + 1];
			if (this.mode === "comment") {
				if (char === "*" && next === "/") { this.mode = "code"; i += 2; } else i++;
				continue;
			}
			if (this.mode !== "code") {
				if (char === "\\") {
					this.ambiguousQuotes = true;
					if (this.mode === "`") this.invalid = true;
					i += 2;
					continue;
				}
				if (char === this.mode) {
					if (next === char) {
						if (this.mode === "`" && !this.invalid) this.identifier += "``";
						i += 2;
						continue;
					}
					if (this.mode === "`") this.add(`${this.identifier}\``);
					this.identifier = "";
					this.mode = "code";
				} else if (this.mode === "`" && !this.invalid) {
					this.identifier += char;
					if (this.identifier.length > 256 || /\s/.test(char)) this.invalid = true;
				}
				i++;
				continue;
			}
			if (/\s/.test(char)) { i++; continue; }
			if (char === "#" || (char === "-" && next === "-" && /\s/.test(line[i + 2] ?? "\n"))) break;
			if (char === "/" && next === "*") {
				if (line[i + 2] === "!" || /^M!/i.test(line.slice(i + 2, i + 4))) this.invalid = true;
				this.mode = "comment";
				i += 2;
				continue;
			}
			if (char === "'" || char === '"' || char === "`") {
				this.mode = char;
				if (char === "`") this.identifier = "`";
				else { this.add("?"); if (char === '"') this.ambiguousQuotes = true; }
				i++;
				continue;
			}
			number.lastIndex = i;
			const numeric = number.exec(line);
			if (numeric) {
				this.add("?");
				i = number.lastIndex;
				if (/[\p{L}\p{N}_$]/u.test(line[i] ?? "")) this.invalid = true;
				continue;
			}
			word.lastIndex = i;
			const identifier = word.exec(line);
			if (identifier) {
				i = word.lastIndex;
				const value = identifier[0];
				const upper = value.toUpperCase();
				if (!(line[i] === "'" && /^(?:_[a-z0-9]+|n|x|b)$/i.test(value))) {
					this.add(upper === "TRUE" || upper === "FALSE" ? "?" : keywords.has(upper) ? upper : value);
				}
				continue;
			}
			if (!"(),.;?+-*/%=<>!|&^~:@".includes(char)) this.invalid = true;
			const operator = line.slice(i, i + 3) === "<=>" ? "<=>" : ["<=", ">=", "<>", "!=", ":=", "||", "&&", "<<", ">>"].includes(char + next) ? char + next : char;
			this.add(operator);
			i += operator.length;
		}
	}

	finish() {
		if (this.invalid || this.mode !== "code" || this.tokens.at(-1) !== ";") return null;
		const tokens = this.tokens.slice(0, -1);
		if (!tokens.length || tokens.includes(";")) return null;
		return { fingerprint: tokens.join(" "), ...explainDraft(tokens, this.ambiguousQuotes) };
	}
}

function explainDraft(tokens: string[], ambiguousQuotes: boolean) {
	const unavailable = (draftReason: string) => ({ draft: null, draftReason });
	if (tokens[0] !== "SELECT") return unavailable("Drafts are limited to single SELECT statements; writes, CTEs and administrative statements are excluded.");
	if (ambiguousQuotes) return unavailable("Quote interpretation depends on SQL mode. Review the original query separately.");
	const blocked = new Set("INTO OUTFILE DUMPFILE FOR LOCK PROCEDURE ANALYZE EXPLAIN INSERT UPDATE DELETE REPLACE CREATE ALTER DROP TRUNCATE SET CALL DO RETURNING".split(" "));
	if (tokens.some((token) => blocked.has(token.toUpperCase()) || ["@", ":", ":="].includes(token))) return unavailable("Output, locking, variables or other side-effect syntax prevents a draft.");
	const groupingWords = new Set(["SELECT", "WHERE", "ON", "AND", "OR", "NOT", "IN", "EXISTS", "HAVING"]);
	for (let i = 0; i < tokens.length - 1; i++) {
		if (tokens[i + 1] === "(" && /^[\p{L}_$`]/u.test(tokens[i]) && !groupingWords.has(tokens[i])) return unavailable("Function calls are excluded, including stored functions and UDFs. Review them separately.");
	}
	return { draft: `-- REVIEW ONLY: confirm database, identifiers and SQL mode.\n-- Replace each ? with a reviewed value; this is not executable as-is.\n-- Inspect referenced views and functions before any manual execution.\nEXPLAIN\n${tokens.join(" ")};`, draftReason: "" };
}

export function normalizeSlowQuery(query: string) {
	if (query.length > SLOW_QUERY_LIMITS.queryCharacters) return null;
	const lexer = new FingerprintLexer();
	for (const line of query.split(/\r?\n/)) lexer.feed(`${line}\n`);
	return lexer.finish();
}

function metric(value: string, integer: boolean) {
	if (!(integer ? /^\d+$/ : /^\d+(?:\.\d+)?$/).test(value)) return null;
	const parsed = Number(value);
	return Number.isFinite(parsed) && parsed <= Number.MAX_SAFE_INTEGER && (!integer || Number.isSafeInteger(parsed)) ? parsed : null;
}

function schemaName(value: string): string | null {
	const name = value.trim();
	if (!name || name.length > 256 || controls.test(name)) return null;
	if (/^`(?:[^`]|``)+`$/.test(name)) return name.slice(1, -1).replaceAll("``", "`");
	return /^[\p{L}\p{N}_$-]+$/u.test(name) ? name : null;
}

export function parseSlowQueryLog(content: string): SlowQueryReport {
	if (content.length > SLOW_QUERY_LIMITS.bytes || new TextEncoder().encode(content).length > SLOW_QUERY_LIMITS.bytes) throw new Error("Slow query log exceeds the 4 MiB limit.");
	if (controls.test(content)) throw new Error("Slow query log contains unsupported control characters.");
	const report: SlowQueryReport = { groups: [], accepted: 0, skipped: 0, truncated: false, totalSeconds: 0, rowsExamined: 0 };
	const groups = new Map<string, SlowQueryGroup>();
	let current: { seconds: number; rows: number; valid: boolean; database: string | null; lexer: FingerprintLexer } | null = null;
	let database: string | null = null;
	let records = 0;

	function finish() {
		if (!current) return;
		const entry = current;
		current = null;
		const normalized = entry.valid ? entry.lexer.finish() : null;
		if (!normalized) { report.skipped++; return; }
		const key = JSON.stringify([entry.database, normalized.fingerprint]);
		let group = groups.get(key);
		if (!group && groups.size >= SLOW_QUERY_LIMITS.groups) { report.skipped++; report.truncated = true; return; }
		if (!Number.isSafeInteger(report.rowsExamined + entry.rows) || report.totalSeconds + entry.seconds > Number.MAX_SAFE_INTEGER) { report.skipped++; return; }
		if (!group) {
			group = { id: groups.size + 1, database: entry.database, ...normalized, count: 0, totalSeconds: 0, averageSeconds: 0, maxSeconds: 0, rowsExamined: 0, averageRowsExamined: 0, maxRowsExamined: 0 };
			groups.set(key, group);
		}
		// Any ambiguous member makes the whole group's draft unavailable.
		if (!normalized.draft) { group.draft = null; group.draftReason = normalized.draftReason; }
		group.count++;
		group.totalSeconds += entry.seconds;
		group.averageSeconds = group.totalSeconds / group.count;
		group.maxSeconds = Math.max(group.maxSeconds, entry.seconds);
		group.rowsExamined += entry.rows;
		group.averageRowsExamined = group.rowsExamined / group.count;
		group.maxRowsExamined = Math.max(group.maxRowsExamined, entry.rows);
		report.accepted++;
		report.totalSeconds += entry.seconds;
		report.rowsExamined += entry.rows;
	}

	for (const raw of content.replace(/^\uFEFF/, "").split("\n")) {
		const line = raw.replace(/\r$/, "");
		const outside = !current || current.lexer.mode === "code";
		if (outside && /^# (?:Time:|User@Host:)/.test(line)) { finish(); continue; }
		if (outside && /^# Thread_id:/.test(line)) {
			finish();
			database = schemaName(line.match(/\bSchema:\s*(.*?)\s+(?:QC_hit:|$)/)?.[1] ?? "");
			continue;
		}
		if (outside && /^# Query_time:/.test(line)) {
			finish();
			if (records++ >= SLOW_QUERY_LIMITS.records) { report.truncated = true; break; }
			const values = line.match(/^# Query_time:\s*(\S+)\s+Lock_time:\s*(\S+)\s+Rows_sent:\s*(\S+)\s+Rows_examined:\s*(\S+)(?:\s.*)?$/);
			const seconds = values ? metric(values[1], false) : null;
			const lock = values ? metric(values[2], false) : null;
			const sent = values ? metric(values[3], true) : null;
			const rows = values ? metric(values[4], true) : null;
			current = { seconds: seconds ?? 0, rows: rows ?? 0, valid: seconds !== null && lock !== null && sent !== null && rows !== null, database, lexer: new FingerprintLexer() };
			continue;
		}
		if (!current) continue;
		if (outside && !current.lexer.tokens.length) {
			const use = line.match(/^use\s+(.+);\s*$/i);
			if (use) { database = schemaName(use[1]); current.database = database; if (!database) current.valid = false; continue; }
			if (/^SET timestamp=\d+(?:\.\d+)?;\s*$/i.test(line)) continue;
		}
		current.lexer.feed(`${line}\n`);
	}
	finish();
	report.groups = [...groups.values()];
	return report;
}

export function slowQueryPage(groups: SlowQueryGroup[], search: string, sort: SlowQuerySort, descending: boolean, requestedPage: number) {
	const needle = search.slice(0, 256).trim().toLocaleLowerCase();
	const matching = groups.filter((group) => !needle || `${group.database ?? ""} ${group.fingerprint}`.toLocaleLowerCase().includes(needle));
	matching.sort((a, b) => (descending ? b[sort] - a[sort] : a[sort] - b[sort]) || a.id - b.id);
	const pages = Math.max(1, Math.ceil(matching.length / SLOW_QUERY_LIMITS.pageSize));
	const page = Math.min(Math.max(0, Math.trunc(requestedPage) || 0), pages - 1);
	return { groups: matching.slice(page * SLOW_QUERY_LIMITS.pageSize, (page + 1) * SLOW_QUERY_LIMITS.pageSize), page, pages, total: matching.length };
}

export function slowQueryIndexAdvice(group: SlowQueryGroup): string[] {
	const advice = ["Compare existing index columns and their order with this query's predicates and joins. A slow-log entry alone does not prove an index is missing."];
	if (group.averageRowsExamined >= 1_000) advice.push("High examined-row counts warrant reviewing selectivity and the chosen access path. A scan may still be appropriate for small tables or broad results.");
	if (/\b(?:WHERE|JOIN|ON)\b/.test(group.fingerprint)) advice.push("Review a composite index for equality and join columns, followed by range or ordering columns where appropriate. Validate column order against the plan and real data distribution.");
	if (/\b(?:ORDER BY|GROUP BY)\b/.test(group.fingerprint)) advice.push("Check whether an existing index can support grouping or ordering before proposing another index. Sorting alone does not establish that a new index will help.");
	advice.push("Review write overhead, index size and duplicate prefixes. Test any proposed index on a representative backup and obtain a migration review; no schema changes are generated or applied.");
	return advice;
}
