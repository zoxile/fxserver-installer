type RepairResult = {
	value: unknown;
	json: string;
	changes: string[];
};

export function tryParseJson(input: string) {
	return JSON.parse(input) as unknown;
}

export function formatJson(value: unknown, indent = 2) {
	return JSON.stringify(value, null, indent);
}

export function minifyJson(value: unknown) {
	return JSON.stringify(value);
}

export function getJsonErrorMessage(input: string, error: unknown) {
	const message = error instanceof SyntaxError ? error.message : String(error);
	const position = getErrorPosition(message);

	if (position === null) return message;

	const { line, column } = getLineColumn(input, position);
	return `${message} at line ${line}, column ${column}.`;
}

export function repairJson(input: string): RepairResult | null {
	const steps: Array<[(value: string) => string, string]> = [
		[stripByteOrderMark, "Removed byte-order marker"],
		[stripJsonComments, "Removed JavaScript-style comments"],
		[normalizeSmartQuotes, "Normalized smart quotes"],
		[quoteUnquotedObjectKeys, "Quoted unquoted object keys"],
		[replaceSingleQuotedStrings, "Converted single-quoted strings"],
		[removeTrailingCommas, "Removed trailing commas"],
		[closeMissingBrackets, "Added missing closing brackets"],
	];

	let candidate = input;
	const changes: string[] = [];

	for (const [repair, label] of steps) {
		const next = repair(candidate);
		if (next !== candidate) {
			candidate = next;
			changes.push(label);
		}
	}

	if (candidate === input) return null;

	try {
		const value = tryParseJson(candidate);
		return {
			value,
			json: formatJson(value),
			changes,
		};
	} catch {
		return null;
	}
}

function getErrorPosition(message: string) {
	const match = message.match(/position (\d+)/i);
	return match ? Number(match[1]) : null;
}

function getLineColumn(input: string, position: number) {
	const before = input.slice(0, position);
	const lines = before.split(/\r?\n/);
	return {
		line: lines.length,
		column: lines.at(-1)?.length ?? 0,
	};
}

function stripByteOrderMark(input: string) {
	return input.replace(/^\uFEFF/, "");
}

function stripJsonComments(input: string) {
	let output = "";
	let inString = false;
	let quote = "";

	for (let index = 0; index < input.length; index += 1) {
		const current = input[index];
		const next = input[index + 1];

		if (inString) {
			output += current;
			if (current === "\\" && next) {
				output += next;
				index += 1;
			} else if (current === quote) {
				inString = false;
			}
			continue;
		}

		if (current === '"' || current === "'") {
			inString = true;
			quote = current;
			output += current;
			continue;
		}

		if (current === "/" && next === "/") {
			while (index < input.length && !/\r|\n/.test(input[index])) {
				index += 1;
			}
			output += input[index] ?? "";
			continue;
		}

		if (current === "/" && next === "*") {
			index += 2;
			while (index < input.length && !(input[index] === "*" && input[index + 1] === "/")) {
				index += 1;
			}
			index += 1;
			continue;
		}

		output += current;
	}

	return output;
}

function normalizeSmartQuotes(input: string) {
	return input.replace(/[\u201C\u201D]/g, '"').replace(/[\u2018\u2019]/g, "'");
}

function quoteUnquotedObjectKeys(input: string) {
	return input.replace(/([{,]\s*)([A-Za-z_$][\w$-]*)(\s*:)/g, '$1"$2"$3');
}

function replaceSingleQuotedStrings(input: string) {
	return input.replace(/'([^'\\]*(?:\\.[^'\\]*)*)'/g, (_, content: string) => {
		const escaped = content.replace(/"/g, '\\"');
		return `"${escaped}"`;
	});
}

function removeTrailingCommas(input: string) {
	let output = "";
	let inString = false;
	let quote = "";

	for (let index = 0; index < input.length; index += 1) {
		const current = input[index];

		if (inString) {
			output += current;
			if (current === "\\" && input[index + 1]) {
				output += input[index + 1];
				index += 1;
			} else if (current === quote) {
				inString = false;
			}
			continue;
		}

		if (current === '"' || current === "'") {
			inString = true;
			quote = current;
			output += current;
			continue;
		}

		if (current === ",") {
			let cursor = index + 1;
			while (/\s/.test(input[cursor] ?? "")) {
				cursor += 1;
			}
			if (input[cursor] === "}" || input[cursor] === "]") {
				continue;
			}
		}

		output += current;
	}

	return output;
}

function closeMissingBrackets(input: string) {
	const stack: string[] = [];
	let inString = false;
	let quote = "";

	for (let index = 0; index < input.length; index += 1) {
		const current = input[index];

		if (inString) {
			if (current === "\\" && input[index + 1]) {
				index += 1;
			} else if (current === quote) {
				inString = false;
			}
			continue;
		}

		if (current === '"' || current === "'") {
			inString = true;
			quote = current;
			continue;
		}

		if (current === "{") {
			stack.push("}");
			continue;
		}

		if (current === "[") {
			stack.push("]");
			continue;
		}

		if (current === "}" || current === "]") {
			if (stack.pop() !== current) {
				return input;
			}
		}
	}

	if (inString || stack.length === 0) return input;

	return `${input}${stack.reverse().join("")}`;
}
