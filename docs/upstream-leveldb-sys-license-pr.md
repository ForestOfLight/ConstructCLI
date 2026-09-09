# Upstream PR: add the missing licence to `leveldb-sys`

A pull request to open against
[`bedrock-crustaceans/leveldb-sys`](https://github.com/bedrock-crustaceans/leveldb-sys).
It is not a blocker — `third_party/vendor/leveldb-sys/` already lets us ship —
but it is the permanent fix, and it would delete that directory.

## The ask

Add the LICENSE file that did not survive the repository move, and declare it in
the manifest.

**1. `LICENSE`** — a copy of `bedrock-rs`'s own
[`LICENSE`](https://github.com/bedrock-crustaceans/bedrock-rs/blob/main/LICENSE),
the Apache License 2.0, byte for byte (sha256
`c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4`; confirmed
against bedrock-rs `main` on 2026-09-08).

The commit is already prepared in a clone at `../leveldb-sys`, on branch
`add-license`.

**2. `Cargo.toml`** — one line, so the licence is visible to tooling and not
just to a human reading the repository root:

```toml
[package]
name = "leveldb-sys"
version = "0.1.0"
edition = "2024"
build = "build.rs"
license = "Apache-2.0"
```

Leave `ffi/leveldb/LICENSE` exactly where it is. It is Google's BSD-3-Clause for
the vendored C++ and correctly scoped to that subtree.

## Why this is the right licence

The wrapper was written inside bedrock-rs and published under Apache-2.0 there
for twenty months before it moved. The move is a plain file relocation:

| | |
|---|---|
| bedrock-rs has carried Apache-2.0 at its root since | 2024-07-24 |
| leveldb-sys `0601d7e`, *"copied over source files from bedrock-rs"*, adds `build.rs`, `src/lib.rs`, `ffi/ffi.cpp`, `ffi/ffi.h`, `ffi/CMakeLists.txt` | 2026-03-24 16:40 UTC |
| bedrock-rs [`d4946739`](https://github.com/bedrock-crustaceans/bedrock-rs/commit/d4946739826c05e119e6e16d437d9626dc6a70a2), *"Move LevelDB FFI to .../leveldb-sys (#206)"*, deletes the same five files there, 33 minutes later | 2026-03-24 17:13 UTC |
| Neither commit carries `LICENSE` across | — |

The code is the same on both sides. Taking `crates/level/src/mojang/ffi.rs` from
bedrock-rs at `d4946739^` and applying the renames made after the move
(`FfiStatus`/`FfiData`/`FfiResult` → `Status`/`Data`/`Result`, and the
`bedrockrs_` prefix moved from the Rust names to `#[link_name]`) reproduces
today's `src/lib.rs` byte for byte, apart from one line inside a commented-out
block. `ffi.cpp` and `ffi.h` differ only in trailing whitespace and a final
newline.

One thing worth flagging so it does not cause confusion: a `LICENSE` containing
MIT text appears at the repository root in July 2022 (`d2c6319`). It is not
evidence of an MIT grant over this code. It predates the Rust wrapper by four
years, was added when the repository was a fork of the C++ leveldb, and was
replaced seven minutes later (`32125c6`) with Google's BSD-3-Clause text — then
moved to `ffi/leveldb/LICENSE` by the same commit that brought the wrapper in.

## Suggested PR text

> **Title:** Add the LICENSE file, lost in the move from bedrock-rs
>
> **Body:**
>
> This repository currently declares no licence: there is no `LICENSE` at the
> root and no `license` field in `Cargo.toml`. The only licence text in the tree
> is `ffi/leveldb/LICENSE`, Google's BSD-3-Clause, which covers the vendored C++
> and says nothing about the Rust wrapper or the FFI shim.
>
> That looks like an accident of the move rather than a decision. `0601d7e`
> copied `build.rs`, `src/lib.rs`, `ffi/ffi.cpp`, `ffi/ffi.h` and
> `ffi/CMakeLists.txt` in from bedrock-rs, and half an hour later bedrock-rs
> `d4946739` deleted them there — and bedrock-rs has been Apache-2.0 since
> July 2024. The LICENSE file just did not come with them.
>
> This PR adds bedrock-rs's Apache-2.0 `LICENSE` at the root and declares
> `license = "Apache-2.0"` in `Cargo.toml`. `ffi/leveldb/LICENSE` is untouched.
>
> If Apache-2.0 is not what you intend for this repository, say so and I will
> close this — the useful part either way is that downstream users currently
> have no terms to rely on.

## What happens here when it lands

Bump the pinned commit in `scripts/setup-deps.sh`, delete
`third_party/vendor/leveldb-sys/`, drop the overlay step from that script and
the corresponding gate from `scripts/check-release-preconditions.sh`, and fold
the wrapper's two build fixes back into
`third_party/patches/0001-leveldb-sys-macos-and-arm-portability.patch`. The
`leveldb-sys` entry in `about.toml` keeps its `BSD-3-Clause` and `Zlib` file
entries and loses the `Apache-2.0` one, which cargo-about would then read from
the manifest.
