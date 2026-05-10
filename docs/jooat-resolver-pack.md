# JOOAT Resolver Pack

The hasher works without extra files. The resolver database is optional so users can keep the app small, or download the larger lookup pack from a GitHub release when they need offline resolving.

## Pack Layout

Publish a release asset folder with a manifest and 256 sharded TSV files:

```text
manifest.json
shards/
  00.tsv
  01.tsv
  ...
  ff.tsv
```

Each shard contains only hashes whose 8-digit hex value starts with that shard prefix. Lines are tab-separated:

```text
0xB779A091	adder
0x79FBB0C5	police
0x1B06D571	weapon_pistol
```

The app reads only the shard needed for the entered hash. It does not load the full resolver database into Svelte.

Build the pack from a complete newline-delimited wordlist:

```bash
node scripts/build-jooat-resolver-pack.mjs complete-wordlist.txt dist/jooat-db 2026.05.10
```

Upload `manifest.json` and the whole `shards/` directory as GitHub release assets. The app validates that every shard prefix from `00` through `ff` is present before it marks the database as installed.

## Manifest

```json
{
  "version": "2026.05.10",
  "source": "https://github.com/zoxile/fxserver-installer/releases/tag/jooat-db",
  "generatedAt": "2026-05-10T00:00:00Z",
  "totalHashes": 41604567,
  "totalNames": 300161,
  "sizeBytes": 789060193,
  "shards": [
    {
      "prefix": "00",
      "path": "shards/00.tsv",
      "hashes": 162300,
      "bytes": 3000000
    }
  ]
}
```

The manifest URL can point at a GitHub release asset. Future users can skip this download and use hasher-only mode, or install/remove the pack from the JOOAT page.
