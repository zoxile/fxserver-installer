import { createReadStream, createWriteStream } from "node:fs";
import { mkdir, stat, writeFile } from "node:fs/promises";
import path from "node:path";
import { createInterface } from "node:readline";

const [, , inputFile, outputDir, version = new Date().toISOString().slice(0, 10)] = process.argv;

if (!inputFile || !outputDir) {
	console.error("Usage: node scripts/build-jooat-resolver-pack.mjs <wordlist.txt> <output-dir> [version]");
	process.exit(1);
}

const shardDir = path.join(outputDir, "shards");
await mkdir(shardDir, { recursive: true });

const shardStreams = new Map();
const shardStats = new Map();
const seen = new Set();

for (let index = 0; index <= 0xff; index += 1) {
	const prefix = index.toString(16).padStart(2, "0");
	const filePath = path.join(shardDir, `${prefix}.tsv`);
	shardStreams.set(prefix, createWriteStream(filePath, { encoding: "utf8" }));
	shardStats.set(prefix, { hashes: 0, path: `shards/${prefix}.tsv` });
}

const lines = createInterface({
	input: createReadStream(inputFile, { encoding: "utf8" }),
	crlfDelay: Infinity,
});

for await (const line of lines) {
	const name = line.trim();
	if (!name || name.startsWith("#")) continue;

	const normalized = name.toLowerCase();
	if (seen.has(normalized)) continue;
	seen.add(normalized);

	const hash = jooat(normalized);
	const prefix = hash.toString(16).padStart(8, "0").slice(0, 2);
	shardStreams.get(prefix).write(`0x${hash.toString(16).toUpperCase().padStart(8, "0")}\t${normalized}\n`);
	shardStats.get(prefix).hashes += 1;
}

await Promise.all([...shardStreams.values()].map((stream) => new Promise((resolve) => stream.end(resolve))));

let sizeBytes = 0;
const shards = [];
for (let index = 0; index <= 0xff; index += 1) {
	const prefix = index.toString(16).padStart(2, "0");
	const stats = shardStats.get(prefix);
	const bytes = (await stat(path.join(outputDir, stats.path))).size;
	sizeBytes += bytes;
	shards.push({ prefix, path: stats.path, hashes: stats.hashes, bytes });
}

const manifest = {
	version,
	source: null,
	generatedAt: new Date().toISOString(),
	totalHashes: seen.size,
	totalNames: seen.size,
	sizeBytes,
	shards,
};

await writeFile(path.join(outputDir, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);

console.log(`Built JOOAT resolver pack with ${seen.size.toLocaleString()} names at ${outputDir}`);

function jooat(input) {
	let hash = 0;
	for (let index = 0; index < input.length; index += 1) {
		hash += input.charCodeAt(index);
		hash += hash << 10;
		hash ^= hash >>> 6;
	}
	hash += hash << 3;
	hash ^= hash >>> 11;
	hash += hash << 15;
	return hash >>> 0;
}
