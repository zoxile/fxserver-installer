export type ParameterType = "text" | "number" | "boolean" | "null";
export interface QueryParameter { name: string; type: ParameterType }
interface Occurrence extends QueryParameter { start: number; end: number }
export const MAX_TEMPLATE_LENGTH = 16_384;

// Templates deliberately exclude comments and backslash escapes: SQL modes change
// their interpretation. Parameters become scalar literals/expressions, never SQL names.
function occurrences(sql: string): Occurrence[] {
	if (sql.length > MAX_TEMPLATE_LENGTH) throw new Error("Query templates are limited to 16,384 characters.");
	if (!sql.includes("{{")) return [];
	if (/\\|--|#|\/\*|\*\//.test(sql)) throw new Error("Parameterized drafts cannot contain comments or backslash escapes.");
	const found: Occurrence[] = [];
	let quote = "";
	for (let i = 0; i < sql.length; i++) {
		const char = sql[i];
		if (quote) {
			if (sql.startsWith("{{", i)) throw new Error("Parameters must be outside quoted strings and identifiers.");
			if (char === quote) {
				if (sql[i + 1] === quote) i++;
				else quote = "";
			}
			continue;
		}
		if (["'", '"', "`"].includes(char)) { quote = char; continue; }
		if (!sql.startsWith("{{", i)) continue;
		const match = /^\{\{([A-Za-z][A-Za-z0-9_]{0,39}):(text|number|boolean|null)\}\}/.exec(sql.slice(i));
		if (!match) throw new Error("Use typed value parameters such as {{player_id:number}} or {{name:text}}.");
		const end = i + match[0].length;
		if (/[\w.$@`]/.test(sql[i - 1] ?? "") || /[\w.$@`]/.test(sql[end] ?? "")) throw new Error("Parameters must be standalone values, not identifiers or parts of tokens.");
		found.push({ name: match[1], type: match[2] as ParameterType, start: i, end });
		i = end - 1;
	}
	if (quote) throw new Error("Close quoted strings and identifiers before preparing a draft.");
	return found;
}

export function queryParameters(sql: string): QueryParameter[] {
	const unique = new Map<string, QueryParameter>();
	for (const { name, type } of occurrences(sql)) {
		if (unique.has(name) && unique.get(name)!.type !== type) throw new Error("Repeated parameters must have the same type.");
		unique.set(name, { name, type });
	}
	if (unique.size > 16) throw new Error("A query can have at most 16 parameters.");
	return [...unique.values()];
}

function literal(parameter: QueryParameter, value: unknown): string {
	if (parameter.type === "null") return "(NULL)";
	if (typeof value !== "string") throw new Error(`Enter a value for ${parameter.name}.`);
	if (value.length > 4096 || /[\u0000-\u0008\u000b\u000c\u000e-\u001f\ud800-\udfff]/u.test(value)) throw new Error("Parameter values must be valid text, at most 4,096 characters.");
	if (parameter.type === "number") {
		if (!/^-?(?:0|[1-9]\d*)(?:\.\d+)?$/.test(value) || value.length > 65 || !Number.isFinite(Number(value))) throw new Error(`Enter a plain finite number for ${parameter.name}.`);
		// LIMIT/OFFSET need literal integers, not parenthesized expressions. Padding
		// also prevents a preceding minus and a negative value from forming "--".
		return ` ${value} `;
	}
	if (parameter.type === "boolean") {
		if (value !== "true" && value !== "false") throw new Error(`Choose true or false for ${parameter.name}.`);
		return value === "true" ? "(TRUE)" : "(FALSE)";
	}
	const hex = [...new TextEncoder().encode(value)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
	return `(CONVERT(X'${hex}' USING utf8mb4))`;
}

export function prepareQueryDraft(sql: string, values: Record<string, string>): string {
	queryParameters(sql);
	const parts = occurrences(sql);
	let result = "";
	let position = 0;
	for (const parameter of parts) {
		result += sql.slice(position, parameter.start) + literal(parameter, Object.hasOwn(values, parameter.name) ? values[parameter.name] : undefined);
		if (result.length > 65_536) throw new Error("Prepared SQL exceeds the 65,536-character draft limit.");
		position = parameter.end;
	}
	result += sql.slice(position);
	if (result.length > 65_536) throw new Error("Prepared SQL exceeds the 65,536-character draft limit.");
	return result;
}
