# JOOAT Resolver Pack

FXServer Installer can hash JOOAT values without downloading anything. The resolver database is optional and only needed when users want offline hash-to-name lookups.

The resolver is designed around small shard reads instead of loading one large database into the Svelte UI. In desktop builds, the Tauri backend stores the installed pack in the app data directory under `jooat-resolver`. Browser preview/dev pages can still use the hasher and dictionary mode, but the full resolver database requires the Tauri runtime.

## How Lookup Works

1. The app normalizes an entered hash to an 8-character lowercase hex value.
2. The first two hex characters select one shard, for example `b7` for `0xB779A091`.
3. Only that shard is read.
4. The backend scans the shard for matching hashes and returns matching names.

This keeps the resolver responsive even when the full pack is large.

## Pack Layout

A resolver pack is a static folder with one manifest and 256 shard files:

```text
manifest.json
shards/
  00.tsv
  01.tsv
  ...
  ff.tsv
```

Every prefix from `00` through `ff` must exist in the manifest. The app marks the database available only when the manifest is complete and every referenced shard has been installed locally.

## Manifest Format

```json
{
  "version": "2026.05.22",
  "source": "https://github.com/zoxile/fxserver-installer/tree/jooat-db",
  "generatedAt": "2026-05-22T00:00:00.000Z",
  "totalHashes": 41604567,
  "totalNames": 41604567,
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

Required fields:

- `version`: Displayed in the app and used to identify the installed pack.
- `shards`: An array containing exactly one entry for every prefix from `00` through `ff`.
- `shards[].prefix`: Two lowercase hex characters.
- `shards[].path`: Relative path to the shard file.

Optional metadata:

- `source`: Where the pack came from.
- `generatedAt`: ISO timestamp for the pack build.
- `totalHashes`, `totalNames`, `sizeBytes`, `hashes`, `bytes`: Used for UI information only.

Path rules:

- Shard paths are resolved relative to the manifest URL.
- Absolute paths are rejected.
- Paths containing `..` are rejected.
- Windows drive-style paths containing `:` are rejected.

## Shard Format

Each shard contains one hash/name pair per line. Empty lines and comment lines beginning with `#` are ignored.

The preferred format is tab-separated TSV:

```text
0xB779A091	adder
0x79FBB0C5	police
0x1B06D571	weapon_pistol
```

The parser is intentionally tolerant and also accepts comma-separated or whitespace-separated lines:

```text
B779A091 adder
hash_79FBB0C5,police
454481265 weapon_pistol
```

Accepted hash formats:

- `0xB779A091`
- `B779A091`
- `hash_B779A091`
- unsigned decimal
- signed decimal

Names are trimmed and must not be empty. The included builder lowercases, deduplicates, and hashes each input name before writing it to a shard, which matches the app's normal hasher behavior.

## Build A Pack

Create a newline-delimited wordlist where each line is a possible native/resource/config name:

```text
adder
police
weapon_pistol
```

Then run:

```bash
node scripts/build-jooat-resolver-pack.mjs complete-wordlist.txt dist/jooat-db 2026.05.22
```

Arguments:

- `complete-wordlist.txt`: Input wordlist.
- `dist/jooat-db`: Output folder.
- `2026.05.22`: Optional version string. If omitted, the script uses the current date.

The output folder will contain `manifest.json` and the `shards/` folder. The generated manifest sets `source` to `null`; edit it after building if you want the app to show a source URL.

## Host A Pack

The manifest URL entered in the app can point at any static host as long as the shard paths resolve beside it. Good options are:

- A raw GitHub branch, for example `https://raw.githubusercontent.com/zoxile/fxserver-installer/jooat-db/manifest.json`.
- A GitHub release asset setup with stable direct URLs.
- Any CDN or static web host that serves JSON and TSV files without authentication.

The app fetches the manifest first, validates that all 256 shards are listed, writes the local manifest, then downloads each shard relative to the manifest URL.

## Updating A Pack

Build a new pack with a new `version`, upload the new manifest and shards, then reinstall from the JOOAT page. Removing the database from the app deletes only the local resolver pack stored in the app data directory.

## Troubleshooting

- `Manifest must include all 256 JOOAT shard prefixes`: One or more prefixes from `00` through `ff` is missing.
- `Shard path is not allowed`: A shard path is absolute, contains `..`, or contains a Windows drive separator.
- Resolver database shows unavailable after install: At least one shard failed to download or the local manifest does not reference the installed files.
- Hash resolves in dictionary mode but not database mode: The installed shard pack does not contain that name/hash pair.
