export type HashFormat = {
	value: number;
	hex: string;
	unsigned: string;
	signed: string;
};

export type HashResult = HashFormat & {
	input: string;
	normalized: string;
};

export type ResolveResult = {
	query: string;
	hash?: HashFormat;
	matches: string[];
	error?: string;
};

export function jooat(input: string) {
	let hash = 0;
	const normalized = input.toLowerCase();

	for (let index = 0; index < normalized.length; index += 1) {
		hash += normalized.charCodeAt(index);
		hash += hash << 10;
		hash ^= hash >>> 6;
	}

	hash += hash << 3;
	hash ^= hash >>> 11;
	hash += hash << 15;

	return hash >>> 0;
}

export function createHashResult(input: string): HashResult {
	const normalized = input.trim().toLowerCase();
	const value = jooat(normalized);

	return {
		input: input.trim(),
		normalized,
		...formatHash(value),
	};
}

export function formatHash(value: number): HashFormat {
	const unsigned = value >>> 0;
	const signed = unsigned > 0x7fffffff ? unsigned - 0x100000000 : unsigned;

	return {
		value: unsigned,
		hex: `0x${unsigned.toString(16).toUpperCase().padStart(8, "0")}`,
		unsigned: String(unsigned),
		signed: String(signed),
	};
}

export function parseHash(input: string): HashFormat | null {
	const cleaned = input.trim();
	if (!cleaned) return null;

	if (/^0x[0-9a-f]+$/i.test(cleaned)) {
		const value = Number.parseInt(cleaned.slice(2), 16);
		return Number.isFinite(value) ? formatHash(value) : null;
	}

	if (/^-?\d+$/.test(cleaned)) {
		const value = Number.parseInt(cleaned, 10);
		return Number.isFinite(value) ? formatHash(value) : null;
	}

	if (/^[0-9a-f]{8}$/i.test(cleaned)) {
		const value = Number.parseInt(cleaned, 16);
		return Number.isFinite(value) ? formatHash(value) : null;
	}

	return null;
}

export function parseLines(input: string) {
	return input
		.split(/\r?\n/)
		.map((line) => line.trim())
		.filter(Boolean);
}

export function parseHashQueries(input: string) {
	return input
		.split(/[\s,;]+/)
		.map((line) => line.trim())
		.filter(Boolean);
}

export function uniqueHashResults(input: string) {
	const seen = new Set<string>();
	const results: HashResult[] = [];

	for (const line of parseLines(input)) {
		const normalized = line.toLowerCase();
		if (seen.has(normalized)) continue;
		seen.add(normalized);
		results.push(createHashResult(line));
	}

	return results;
}

export function resolveHashes(hashInput: string, candidateInput: string): ResolveResult[] {
	const candidates = uniqueHashResults(candidateInput);
	const lookup = new Map<number, HashResult[]>();

	for (const candidate of candidates) {
		const matches = lookup.get(candidate.value) ?? [];
		matches.push(candidate);
		lookup.set(candidate.value, matches);
	}

	return parseHashQueries(hashInput).map((query) => {
		const hash = parseHash(query);
		if (!hash) {
			return {
				query,
				matches: [],
				error: "Enter a hex, unsigned, or signed 32-bit hash.",
			};
		}

		return {
			query,
			hash,
			matches: (lookup.get(hash.value) ?? []).map((match) => match.input),
		};
	});
}
