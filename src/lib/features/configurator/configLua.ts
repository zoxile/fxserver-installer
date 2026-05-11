export type LuaPrimitiveType = "string" | "number" | "boolean" | "nil" | "raw";
export type LuaValueType = LuaPrimitiveType | "vector2" | "vector3" | "vector4" | "table";

export type LuaValue =
	| { type: "string"; value: string }
	| { type: "number"; value: number }
	| { type: "boolean"; value: boolean }
	| { type: "nil" }
	| { type: "raw"; value: string }
	| { type: "vector2" | "vector3" | "vector4"; values: number[] }
	| { type: "table"; entries: LuaTableEntry[] };

export type LuaKey =
	| { type: "identifier"; value: string }
	| { type: "string"; value: string }
	| { type: "number"; value: number; implicit?: boolean };

export interface LuaTableEntry {
	key?: LuaKey;
	value: LuaValue;
}

export interface ConfigSetting {
	id: string;
	path: Array<string | number>;
	label: string;
	type: LuaValueType;
	value: LuaValue;
	editable: boolean;
	comment?: string;
}

export interface ConfigObjectGroup {
	id: string;
	path: Array<string | number>;
	label: string;
	fieldCount: number;
	objectCount: number;
	comment?: string;
}

export interface ParsedConfig {
	root: LuaValue;
	settings: ConfigSetting[];
	output: string;
	warnings: string[];
	commentsByPath: Record<string, string>;
	unassignedComments: string[];
}

type TokenType = "identifier" | "number" | "string" | "symbol" | "eof";

interface Token {
	type: TokenType;
	value: string;
	position: number;
	end: number;
}

const vectorSizes = {
	vector2: 2,
	vector3: 3,
	vector4: 4,
} as const;

export const sampleConfigLua = `Config = {}

-- Enable extra debug output while testing resources.
Config.Debug = false

-- Language key used by your resource translations.
Config.Locale = "en"

-- Maximum amount of players this configuration expects.
Config.MaxPlayers = 64

-- Default player spawn position and heading.
Config.Spawn = vector4(215.76, -810.12, 30.73, 157.5)

Config.Zones = {
    garage = {
        -- Label shown in menus.
        label = "Central Garage",
        -- Interaction point for the garage marker.
        coords = vector3(229.16, -800.12, 30.57),
        radius = 4.5
    },
    shops = {
        -- Shop coordinates can be expanded with more vector3 entries.
        vector3(25.7, -1347.3, 29.5),
        vector3(-48.5, -1757.7, 29.4)
    }
}`;

export function parseConfigLua(source: string): ParsedConfig {
	const parser = new LuaConfigParser(source);
	const root = parser.parse();
	const comments = associateComments(source, parser.valueLines);
	const settings = flattenSettings(root, comments.commentsByPath);
	const warnings = [...parser.warnings, ...validateLuaValue(root)];
	const output = stringifyConfig(root, comments.commentsByPath);

	return {
		root,
		settings,
		output,
		warnings,
		commentsByPath: comments.commentsByPath,
		unassignedComments: comments.unassignedComments,
	};
}

export function stringifyConfig(root: LuaValue, commentsByPath: Record<string, string> = {}) {
	if (root.type !== "table") {
		return `Config = ${stringifyLuaValue(root, 0, [], commentsByPath)}\n`;
	}

	return `Config = ${stringifyLuaValue(root, 0, [], commentsByPath)}\n`;
}

export function getConfigSettings(root: LuaValue, commentsByPath: Record<string, string> = {}) {
	return flattenSettings(root, commentsByPath);
}

export function getConfigWarnings(root: LuaValue) {
	return validateLuaValue(root);
}

export function getConfigObjects(root: LuaValue, commentsByPath: Record<string, string> = {}) {
	const groups: ConfigObjectGroup[] = [];

	function visit(value: LuaValue, path: Array<string | number>) {
		if (value.type !== "table") return;

		groups.push({
			id: pathKey(path),
			path,
			label: path.length ? pathToLabel(path).replace(/^Config\./, "") : "Configuration",
			fieldCount: value.entries.filter((entry) => entry.value.type !== "table").length,
			objectCount: value.entries.filter((entry) => entry.value.type === "table").length,
			comment: commentsByPath[pathKey(path)],
		});

		for (const entry of value.entries) {
			if (entry.key && entry.value.type === "table") visit(entry.value, [...path, entry.key.value]);
		}
	}

	visit(root, []);
	return groups;
}

export function configPathKey(path: Array<string | number>) {
	return pathKey(path);
}

export function createLuaValue(type: LuaValueType): LuaValue {
	switch (type) {
		case "string":
			return { type: "string", value: "" };
		case "number":
			return { type: "number", value: 0 };
		case "boolean":
			return { type: "boolean", value: false };
		case "nil":
			return { type: "nil" };
		case "vector2":
			return { type: "vector2", values: [0, 0] };
		case "vector3":
			return { type: "vector3", values: [0, 0, 0] };
		case "vector4":
			return { type: "vector4", values: [0, 0, 0, 0] };
		case "table":
			return { type: "table", entries: [] };
		case "raw":
			return { type: "raw", value: "nil" };
	}
}

export function addConfigEntry(root: LuaValue, parentPath: Array<string | number>, key: string, type: LuaValueType) {
	const cloned = cloneLuaValue(root);
	const table = findTableAtPath(cloned, parentPath);
	if (!table) throw new Error("Parent object was not found.");

	const trimmedKey = key.trim();
	if (!trimmedKey) throw new Error("New field name is required.");

	const parsedKey = parseNewKey(trimmedKey);
	if (table.entries.some((entry) => entry.key?.value === parsedKey.value)) {
		throw new Error(`${trimmedKey} already exists in this object.`);
	}

	table.entries.push({
		key: parsedKey,
		value: createLuaValue(type),
	});

	return cloned;
}

export function removeConfigEntry(root: LuaValue, path: Array<string | number>) {
	const cloned = cloneLuaValue(root);
	const parentPath = path.slice(0, -1);
	const key = path[path.length - 1];
	const table = findTableAtPath(cloned, parentPath);
	if (!table) return cloned;

	table.entries = table.entries.filter((entry) => entry.key?.value !== key);
	return cloned;
}

export function stringifyLuaValue(value: LuaValue, indent = 0, path: Array<string | number> = [], commentsByPath: Record<string, string> = {}): string {
	switch (value.type) {
		case "string":
			return `"${escapeLuaString(value.value)}"`;
		case "number":
			return formatNumber(value.value);
		case "boolean":
			return value.value ? "true" : "false";
		case "nil":
			return "nil";
		case "raw":
			return value.value;
		case "vector2":
		case "vector3":
		case "vector4":
			return `${value.type}(${value.values.map(formatNumber).join(", ")})`;
		case "table":
			return stringifyTable(value, indent, path, commentsByPath);
	}
}

export function updateConfigValue(root: LuaValue, path: Array<string | number>, nextValue: LuaValue) {
	const cloned = cloneLuaValue(root);
	setValueAtPath(cloned, path, nextValue);
	return cloned;
}

export function cloneLuaValue<T extends LuaValue>(value: T): T {
	return JSON.parse(JSON.stringify(value)) as T;
}

export function valueSummary(value: LuaValue) {
	switch (value.type) {
		case "string":
			return value.value || '""';
		case "number":
			return formatNumber(value.value);
		case "boolean":
			return value.value ? "true" : "false";
		case "nil":
			return "nil";
		case "raw":
			return value.value;
		case "vector2":
		case "vector3":
		case "vector4":
			return `${value.type}(${value.values.map(formatNumber).join(", ")})`;
		case "table":
			return `${value.entries.length} entries`;
	}
}

function stringifyTable(value: Extract<LuaValue, { type: "table" }>, indent: number, path: Array<string | number>, commentsByPath: Record<string, string>) {
	if (value.entries.length === 0) return "{}";

	const currentIndent = "\t".repeat(indent);
	const childIndent = "\t".repeat(indent + 1);
	const lines = value.entries.map((entry) => {
		const entryPath = entry.key ? [...path, entry.key.value] : path;
		const key = entry.key && !(entry.key.type === "number" && entry.key.implicit) ? `${formatKey(entry.key)} = ` : "";
		const comment = commentsByPath[pathKey(entryPath)];
		const commentLines = comment ? `${comment.split("\n").map((line) => `${childIndent}-- ${line}`).join("\n")}\n` : "";
		return `${commentLines}${childIndent}${key}${stringifyLuaValue(entry.value, indent + 1, entryPath, commentsByPath)}`;
	});

	return `{\n${lines.join(",\n")}\n${currentIndent}}`;
}

function formatKey(key: LuaKey) {
	if (key.type === "identifier" && isIdentifier(key.value)) return key.value;
	if (key.type === "number") return `[${formatNumber(key.value)}]`;
	return `["${escapeLuaString(String(key.value))}"]`;
}

function formatNumber(value: number) {
	if (!Number.isFinite(value)) return "0";
	return Number.isInteger(value) ? String(value) : String(Number(value.toFixed(6)));
}

function escapeLuaString(value: string) {
	return value.replace(/\\/g, "\\\\").replace(/"/g, '\\"').replace(/\n/g, "\\n").replace(/\r/g, "\\r").replace(/\t/g, "\\t");
}

function isIdentifier(value: string) {
	return /^[A-Za-z_][A-Za-z0-9_]*$/.test(value);
}

function flattenSettings(root: LuaValue, commentsByPath: Record<string, string>) {
	const settings: ConfigSetting[] = [];

	function visit(value: LuaValue, path: Array<string | number>) {
		if (value.type === "table") {
			if (value.entries.length === 0 && path.length > 0) {
				settings.push(createSetting(path, value, commentsByPath));
			}

			for (const entry of value.entries) {
				if (!entry.key) continue;
				visit(entry.value, [...path, entry.key.value]);
			}
			return;
		}

		settings.push(createSetting(path, value, commentsByPath));
	}

	visit(root, []);
	return settings;
}

function createSetting(path: Array<string | number>, value: LuaValue, commentsByPath: Record<string, string>): ConfigSetting {
	const label = pathToLabel(path);

	return {
		id: label,
		path,
		label,
		type: value.type,
		value,
		editable: value.type !== "raw" && value.type !== "table",
		comment: commentsByPath[pathKey(path)],
	};
}

function pathToLabel(path: Array<string | number>) {
	if (!path.length) return "Config";

	return `Config.${path
		.map((part) => {
			if (typeof part === "number") return `[${part}]`;
			return isIdentifier(part) ? part : `["${part}"]`;
		})
		.join(".")
		.replace(/\.\[/g, "[")}`;
}

function pathKey(path: Array<string | number>) {
	return JSON.stringify(path);
}

function validateLuaValue(value: LuaValue, path: Array<string | number> = []) {
	const warnings: string[] = [];
	const label = pathToLabel(path);

	if (value.type === "number" && !Number.isFinite(value.value)) {
		warnings.push(`${label} is not a finite number.`);
	}

	if (value.type === "vector2" || value.type === "vector3" || value.type === "vector4") {
		const expected = vectorSizes[value.type];
		if (value.values.length !== expected) {
			warnings.push(`${label} must contain exactly ${expected} number values.`);
		}
		if (value.values.some((entry) => !Number.isFinite(entry))) {
			warnings.push(`${label} contains a non-numeric vector component.`);
		}
	}

	if (value.type === "raw") {
		warnings.push(`${label} is a raw Lua expression and cannot be safely edited through typed controls.`);
	}

	if (value.type === "table") {
		for (const entry of value.entries) {
			if (entry.key) warnings.push(...validateLuaValue(entry.value, [...path, entry.key.value]));
		}
	}

	return warnings;
}

function setValueAtPath(root: LuaValue, path: Array<string | number>, nextValue: LuaValue) {
	if (path.length === 0) return;
	if (root.type !== "table") return;

	let current = root;
	for (let index = 0; index < path.length; index += 1) {
		const segment = path[index];
		const entry = findEntry(current, segment);
		if (!entry) return;

		if (index === path.length - 1) {
			entry.value = nextValue;
			return;
		}

		if (entry.value.type !== "table") return;
		current = entry.value;
	}
}

function findEntry(table: Extract<LuaValue, { type: "table" }>, key: string | number) {
	return table.entries.find((entry) => entry.key?.value === key);
}

function findTableAtPath(root: LuaValue, path: Array<string | number>) {
	if (root.type !== "table") return null;
	let current = root;

	for (const segment of path) {
		const entry = findEntry(current, segment);
		if (!entry || entry.value.type !== "table") return null;
		current = entry.value;
	}

	return current;
}

function parseNewKey(value: string): LuaKey {
	if (/^\d+$/.test(value)) {
		return { type: "number", value: Number(value), implicit: true };
	}

	return {
		type: isIdentifier(value) ? "identifier" : "string",
		value,
	};
}

interface LuaComment {
	line: number;
	text: string;
	inline: boolean;
}

function associateComments(source: string, valueLines: Map<string, number>) {
	const comments = collectComments(source);
	const consumedLines = new Set<number>();
	const commentsByPath: Record<string, string> = {};
	const sortedEntries = [...valueLines.entries()].sort((left, right) => left[1] - right[1]);

	for (const [key, line] of sortedEntries) {
		const nearby = comments.filter((comment) => {
			if (consumedLines.has(comment.line)) return false;
			if (comment.inline) return comment.line === line;
			return comment.line < line && isOnlyWhitespaceOrCommentsBetween(source, comment.line, line);
		});

		if (!nearby.length) continue;

		for (const comment of nearby) consumedLines.add(comment.line);
		commentsByPath[key] = nearby.map((comment) => comment.text).join("\n");
	}

	return {
		commentsByPath,
		unassignedComments: comments.filter((comment) => !consumedLines.has(comment.line)).map((comment) => comment.text),
	};
}

function collectComments(source: string) {
	const lines = source.split(/\r\n|\r|\n/);
	const comments: LuaComment[] = [];

	for (let index = 0; index < lines.length; index += 1) {
		const line = lines[index];
		const commentStart = findLineCommentStart(line);
		if (commentStart === -1) continue;

		const prefix = line.slice(0, commentStart);
		const marker = line.slice(commentStart);

		if (marker.startsWith("--[[")) {
			const blockLines = [marker.slice(4)];
			let endLine = index;

			while (endLine < lines.length) {
				const endIndex = blockLines[blockLines.length - 1].indexOf("]]");
				if (endIndex !== -1) {
					blockLines[blockLines.length - 1] = blockLines[blockLines.length - 1].slice(0, endIndex);
					break;
				}
				endLine += 1;
				if (endLine < lines.length) blockLines.push(lines[endLine]);
			}

			comments.push({
				line: index + 1,
				text: blockLines.map((entry) => entry.trim()).filter(Boolean).join("\n"),
				inline: prefix.trim().length > 0,
			});
			index = endLine;
			continue;
		}

		comments.push({
			line: index + 1,
			text: marker.slice(2).trim(),
			inline: prefix.trim().length > 0,
		});
	}

	return comments.filter((comment) => comment.text);
}

function findLineCommentStart(line: string) {
	let quote: string | null = null;
	let escaped = false;

	for (let index = 0; index < line.length - 1; index += 1) {
		const char = line[index];
		const next = line[index + 1];

		if (quote) {
			if (escaped) {
				escaped = false;
				continue;
			}
			if (char === "\\") {
				escaped = true;
				continue;
			}
			if (char === quote) quote = null;
			continue;
		}

		if (char === '"' || char === "'") {
			quote = char;
			continue;
		}

		if (char === "-" && next === "-") return index;
	}

	return -1;
}

function isOnlyWhitespaceOrCommentsBetween(source: string, fromLine: number, toLine: number) {
	const lines = source.split(/\r\n|\r|\n/).slice(fromLine, toLine - 1);
	return lines.every((line) => {
		const trimmed = line.trim();
		return !trimmed || trimmed.startsWith("--");
	});
}

class LuaConfigParser {
	private tokens: Token[];
	private cursor = 0;
	warnings: string[] = [];
	valueLines = new Map<string, number>();

	constructor(private source: string) {
		this.tokens = tokenize(source);
	}

	parse(): LuaValue {
		let root: LuaValue = { type: "table", entries: [] };

		while (!this.is("eof")) {
			if (this.matchIdentifier("Config")) {
				const path = this.parseConfigPath();
				if (this.matchSymbol("=")) {
					const valueLine = this.lineAt(this.peek().position);
					const value = this.parseValue(path);
					this.valueLines.set(pathKey(path), valueLine);
					if (path.length === 0) {
						root = value.type === "table" ? value : { type: "table", entries: [{ key: { type: "identifier", value: "value" }, value }] };
					} else {
						if (root.type !== "table") root = { type: "table", entries: [] };
						setConfigPath(root, path, value);
					}
				}
				continue;
			}

			if (this.matchIdentifier("return")) {
				root = this.parseValue([]);
				continue;
			}

			this.advance();
		}

		return root;
	}

	private parseConfigPath() {
		const path: Array<string | number> = [];

		while (true) {
			if (this.matchSymbol(".")) {
				const token = this.consume("identifier", "Expected a property name after Config.");
				path.push(token.value);
				continue;
			}

			if (this.matchSymbol("[")) {
				const key = this.parseBracketKey();
				this.consumeSymbol("]");
				path.push(key.value);
				continue;
			}

			break;
		}

		return path;
	}

	private parseValue(path: Array<string | number>): LuaValue {
		const token = this.peek();

		if (this.matchSymbol("{")) return this.parseTable(path);
		if (token.type === "string") {
			this.advance();
			return { type: "string", value: token.value };
		}
		if (token.type === "number") {
			this.advance();
			return { type: "number", value: Number(token.value) };
		}
		if (this.matchIdentifier("true")) return { type: "boolean", value: true };
		if (this.matchIdentifier("false")) return { type: "boolean", value: false };
		if (this.matchIdentifier("nil")) return { type: "nil" };

		if (token.type === "identifier" && isVectorType(token.value) && this.peek(1).value === "(") {
			return this.parseVector(token.value as "vector2" | "vector3" | "vector4");
		}

		return this.parseRawValue();
	}

	private parseTable(path: Array<string | number>): LuaValue {
		const entries: LuaTableEntry[] = [];
		let implicitIndex = 1;

		while (!this.is("eof") && !this.matchSymbol("}")) {
			if (this.matchSymbol(",") || this.matchSymbol(";")) continue;

			let key: LuaKey | undefined;
			let value: LuaValue;
			let nextPath: Array<string | number>;
			const valueLine = this.lineAt(this.peek().position);

			if (this.peek().type === "identifier" && this.peek(1).value === "=") {
				key = { type: "identifier", value: this.advance().value };
				this.consumeSymbol("=");
				nextPath = [...path, key.value];
				value = this.parseValue(nextPath);
			} else if (this.matchSymbol("[")) {
				key = this.parseBracketKey();
				this.consumeSymbol("]");
				this.consumeSymbol("=");
				nextPath = [...path, key.value];
				value = this.parseValue(nextPath);
			} else {
				key = { type: "number", value: implicitIndex, implicit: true };
				nextPath = [...path, key.value];
				value = this.parseValue(nextPath);
				implicitIndex += 1;
			}

			this.valueLines.set(pathKey(nextPath), valueLine);
			entries.push({ key, value });
			this.matchSymbol(",");
			this.matchSymbol(";");
		}

		return { type: "table", entries };
	}

	private parseBracketKey(): LuaKey {
		const token = this.peek();
		if (token.type === "string") {
			this.advance();
			return { type: "string", value: token.value };
		}
		if (token.type === "number") {
			this.advance();
			return { type: "number", value: Number(token.value) };
		}
		if (token.type === "identifier") {
			this.advance();
			return { type: "string", value: token.value };
		}

		throw new Error(`Expected a table key near ${this.describeToken(token)}.`);
	}

	private parseVector(type: "vector2" | "vector3" | "vector4"): LuaValue {
		this.consume("identifier", `Expected ${type}.`);
		this.consumeSymbol("(");

		const values: number[] = [];
		while (!this.is("eof") && !this.matchSymbol(")")) {
			const numberToken = this.consume("number", `${type} only accepts numeric components.`);
			values.push(Number(numberToken.value));
			this.matchSymbol(",");
		}

		const expected = vectorSizes[type];
		if (values.length !== expected) {
			throw new Error(`${type} expects ${expected} numeric values, received ${values.length}.`);
		}

		return { type, values };
	}

	private parseRawValue(): LuaValue {
		const start = this.peek().position;
		let depth = 0;

		while (!this.is("eof")) {
			const token = this.peek();
			if (depth === 0 && (token.value === "," || token.value === "}" || token.value === ";")) break;
			if (token.value === "(" || token.value === "{" || token.value === "[") depth += 1;
			if (token.value === ")" || token.value === "}" || token.value === "]") depth -= 1;
			this.advance();
		}

		const end = this.peek().position;
		const value = this.source.slice(start, end).trim();
		this.warnings.push(`Unsupported raw Lua expression preserved: ${value || "empty expression"}`);
		return { type: "raw", value: value || "nil" };
	}

	private matchIdentifier(value: string) {
		if (this.peek().type === "identifier" && this.peek().value === value) {
			this.cursor += 1;
			return true;
		}
		return false;
	}

	private matchSymbol(value: string) {
		if (this.peek().type === "symbol" && this.peek().value === value) {
			this.cursor += 1;
			return true;
		}
		return false;
	}

	private consume(type: TokenType, message: string) {
		const token = this.peek();
		if (token.type !== type) throw new Error(`${message} Found ${this.describeToken(token)}.`);
		this.cursor += 1;
		return token;
	}

	private consumeSymbol(value: string) {
		if (!this.matchSymbol(value)) throw new Error(`Expected "${value}" near ${this.describeToken(this.peek())}.`);
	}

	private is(type: TokenType) {
		return this.peek().type === type;
	}

	private peek(offset = 0) {
		return this.tokens[this.cursor + offset] ?? this.tokens[this.tokens.length - 1];
	}

	private advance() {
		const token = this.peek();
		this.cursor += 1;
		return token;
	}

	private describeToken(token: Token) {
		return token.type === "eof" ? "end of file" : `"${token.value}"`;
	}

	private lineAt(position: number) {
		return this.source.slice(0, position).split(/\r\n|\r|\n/).length;
	}
}

function setConfigPath(root: LuaValue, path: Array<string | number>, value: LuaValue) {
	if (root.type !== "table") return;

	let current = root;
	for (let index = 0; index < path.length; index += 1) {
		const segment = path[index];
		const final = index === path.length - 1;
		let entry = findEntry(current, segment);

		if (!entry) {
			entry = {
				key: typeof segment === "number" ? { type: "number", value: segment } : { type: isIdentifier(segment) ? "identifier" : "string", value: segment },
				value: final ? value : { type: "table", entries: [] },
			};
			current.entries.push(entry);
		}

		if (final) {
			entry.value = value;
		} else {
			if (entry.value.type !== "table") entry.value = { type: "table", entries: [] };
			current = entry.value;
		}
	}
}

function isVectorType(value: string) {
	return value === "vector2" || value === "vector3" || value === "vector4";
}

function tokenize(source: string) {
	const tokens: Token[] = [];
	let cursor = 0;

	while (cursor < source.length) {
		const char = source[cursor];

		if (/\s/.test(char)) {
			cursor += 1;
			continue;
		}

		if (char === "-" && source[cursor + 1] === "-") {
			cursor = skipComment(source, cursor + 2);
			continue;
		}

		if (char === '"' || char === "'") {
			const token = readString(source, cursor, char);
			tokens.push(token);
			cursor = token.end;
			continue;
		}

		if (/[0-9.-]/.test(char) && /^(?:-?\d|\.\d)/.test(source.slice(cursor))) {
			const token = readNumber(source, cursor);
			tokens.push(token);
			cursor = token.end;
			continue;
		}

		if (/[A-Za-z_]/.test(char)) {
			const start = cursor;
			cursor += 1;
			while (/[A-Za-z0-9_]/.test(source[cursor] ?? "")) cursor += 1;
			tokens.push({ type: "identifier", value: source.slice(start, cursor), position: start, end: cursor });
			continue;
		}

		if ("{}[]=.,();".includes(char)) {
			tokens.push({ type: "symbol", value: char, position: cursor, end: cursor + 1 });
			cursor += 1;
			continue;
		}

		tokens.push({ type: "symbol", value: char, position: cursor, end: cursor + 1 });
		cursor += 1;
	}

	tokens.push({ type: "eof", value: "", position: source.length, end: source.length });
	return tokens;
}

function skipComment(source: string, cursor: number) {
	if (source[cursor] === "[" && source[cursor + 1] === "[") {
		const end = source.indexOf("]]", cursor + 2);
		return end === -1 ? source.length : end + 2;
	}

	const end = source.indexOf("\n", cursor);
	return end === -1 ? source.length : end + 1;
}

function readString(source: string, start: number, quote: string): Token {
	let cursor = start + 1;
	let value = "";

	while (cursor < source.length) {
		const char = source[cursor];
		if (char === quote) {
			return { type: "string", value, position: start, end: cursor + 1 };
		}

		if (char === "\\") {
			const next = source[cursor + 1];
			value += decodeEscape(next);
			cursor += 2;
			continue;
		}

		value += char;
		cursor += 1;
	}

	throw new Error("Unterminated string literal.");
}

function decodeEscape(value: string) {
	return {
		n: "\n",
		r: "\r",
		t: "\t",
		"\\": "\\",
		'"': '"',
		"'": "'",
	}[value] ?? value;
}

function readNumber(source: string, start: number): Token {
	const match = /^-?(?:\d+\.?\d*|\.\d+)(?:e[+-]?\d+)?/i.exec(source.slice(start));
	if (!match) throw new Error(`Invalid number near ${source.slice(start, start + 12)}.`);
	const value = match[0];
	return { type: "number", value, position: start, end: start + value.length };
}
