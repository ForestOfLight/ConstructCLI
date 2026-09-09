# Patched dependencies

Three upstreams are patched and vendored here: `bedrock_level` (from `bedrock-rs`) and its
`leveldb-sys` backend, the database dependencies, plus `nbtx`, which is not a database
dependency at all — it is the NBT codec `.mcstructure` and `level.dat` are read and written
through.

## `bedrock_level` and `leveldb-sys`

These do not compile as published:

- `leveldb-sys/build.rs` links `stdc++` unconditionally on unix. Apple ships `libc++`,
  so every macOS target fails at the link step. (This one is fixed in the replacement
  `build.rs` under `vendor/leveldb-sys/` rather than by a patch — see below.)
- `bedrock-rs/crates/level/src/greedy.rs` uses `is_x86_feature_detected!`,
  `#[target_feature(enable = "avx2")]`, and `std::arch::x86_64` with no architecture
  gate. This is a *compile* failure on any non-x86_64 target, not a runtime one.
- The vendored `port_posix_sse.cc` defines `LEVELDB_PLATFORM_POSIX_SSE` unconditionally
  and then reaches x86 intrinsics under an `#elif defined(__GNUC__)` that Apple clang
  satisfies on arm64.
- The vendored zlib's `zutil.h` treats `TARGET_OS_MAC` as classic Mac OS and redefines
  `fdopen()` to `NULL`, clobbering the modern SDK's declaration.

## `nbtx`

`nbtx` 3.0.1 cannot serialize an empty list: it writes a sequence's element type and length
lazily, on the first element, so an empty one omits both — five bytes short — producing NBT
that `nbtx` itself cannot parse back. Of 13 real `.mcstructure` files measured, 12 contained at
least one empty list (one, `creaking.mcstructure`, has none), so this blocked encoding for the
overwhelming majority of files rather than being a rare edge case. The patch arms a flag when a
sequence opens and writes `TAG_End` with length 0 from `end()` if no element ever arrived.

## Applying the patches

`patches/` holds the fixes. `scripts/setup-deps.sh` clones each upstream repo at its pinned
commit and applies them into `checkouts/`, which is git-ignored.

`bedrock-rs` and `nbtx` are Apache-2.0, which permits this. The fixes are intended to go
upstream; when they land, or when the patched branches are published as forks, this directory
is deleted and the workspace depends on a URL again.

## `leveldb-sys` and its missing licence

`leveldb-sys` declares no licence of its own — no top-level `LICENSE` file, no `license` field
in its `Cargo.toml`. The only licence text in its checkout is Google's BSD-3-Clause for the
vendored C++ under `ffi/leveldb/`, which says nothing about the Rust wrapper.

That is an accident rather than a decision. The wrapper was written inside `bedrock-rs`, which
has been Apache-2.0 since 2024-07-24, and bedrock-rs commit `d4946739` moved it out to the new
repository on 2026-03-24 without carrying the LICENSE file across. `vendor/leveldb-sys/` holds
our own Apache-2.0 copies of those files, taken from bedrock-rs at the commit before the move;
`setup-deps.sh` copies them over the clone, so nothing that gets compiled and shipped traces to
an unlicensed file. Only `ffi/leveldb/` is used as cloned, and it carries its own terms.

`vendor/leveldb-sys/NOTICE` records every change we made to that code, and
`vendor/leveldb-sys/README.md` records the provenance in full. Edit the wrapper there, not in
the checkout — `scripts/check-release-preconditions.sh` refuses to release if the two disagree.
The vendoring goes away if upstream adds a licence file.

Run `scripts/setup-deps.sh` once after cloning.
