# GLib 0.18.5 backport

- Original archive: <https://static.crates.io/crates/glib/glib-0.18.5.crate>
- SHA256: `233daaf6e83ae6a12a52055f568f9d7cf4671dabb78ff9560ab6da230ce00ee5`
  (verified against the registry checksum in the original application `Cargo.lock`).
- Original source: <https://github.com/gtk-rs/gtk-rs-core/tree/42b9caf98e03ded086362d9653ca58fe94dc8658/glib>
  (from the archive's unchanged `.cargo_vcs_info.json`).
- License: MIT; original `LICENSE` and `COPYRIGHT` are preserved.
- Advisory: <https://rustsec.org/advisories/RUSTSEC-2024-0429.html>
- Upstream fix: <https://github.com/gtk-rs/gtk-rs-core/pull/1343>,
  commit `b5a4071e439bef2b5eea76c3aa25e5ae84839e34`, Fabio Valentini.
- Debian backport reference: <https://sources.debian.org/patches/rust-glib-0.18/0.18.5-7/>.

The only change to original crate files is in `src/variant_iter.rs`:
`let p` becomes `let mut p`, and the variadic GLib out-argument becomes
`&mut p` instead of `&p`. The exact local diff is
[`patches/RUSTSEC-2024-0429.patch`](patches/RUSTSEC-2024-0429.patch).
No Debian dependency changes or other patches are included. The crate version
remains 0.18.5; the application selects this directory with `[patch.crates-io]`.
The scoped `.gitattributes` disables line-ending conversion, preserving archive
bytes even when the Windows checkout has `core.autocrlf=true`.

Verify the official archive checksum, every original file, the exact two-line
change, and the recorded patch (stdlib Python; no extraction or Rust execution):

```sh
python src-tauri/vendor/glib/verify_backport.py
```

Use `--archive /path/to/glib-0.18.5.crate` to verify a cached original archive
without network access. The checksum is still mandatory.

Regression module: `src-tauri/src/platform/glib_regression.rs`. Run on native
Linux with the desktop feature and optimization enabled:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --release --locked --lib platform::glib_regression -- --nocapture
```

The tests cover `next`, `last`, `nth`, `next_back`, `nth_back`, reverse and
mixed iteration, exhaustion, empty arrays, empty strings and UTF-8. The direct
Linux GLib dependency is desktop-only; headless builds do not enable it.

## Audit limitation

On 2026-10-18, `cargo audit` 0.22.2 reported zero vulnerability entries and nine
unmaintained warnings, with no ignores. Advisory DB commit:
`999ec89d5d3f10f20be14750ec8c2d8b44e0ac9b`.
It no longer reports RUSTSEC-2024-0429 because RustSec skips path dependencies;
it does **not** verify local patches. Run the source verifier and optimized
regression separately. This backport does not resolve unrelated maintenance
warnings or certify the rest of the GTK stack as vulnerability-free.
