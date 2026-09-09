# Vendored `leveldb-sys`

The Rust and C++ FFI wrapper around the Mojang variant of LevelDB, vendored here
under Apache-2.0. `scripts/setup-deps.sh` copies this directory over the
`third_party/checkouts/leveldb-sys` clone, so these are the wrapper sources that
actually get compiled and linked into a release binary.

## Why it is vendored

Upstream `bedrock-crustaceans/leveldb-sys` declares no licence. That is an
accident of history rather than a deliberate choice:

| | |
|---|---|
| bedrock-rs has carried Apache-2.0 at its root since | 2024-07-24 |
| leveldb-sys `0601d7e`, *"copied over source files from bedrock-rs"*, adds `build.rs`, `src/lib.rs`, `ffi/ffi.cpp`, `ffi/ffi.h`, `ffi/CMakeLists.txt` | 2026-03-24 16:40 UTC |
| bedrock-rs `d4946739`, *"Move LevelDB FFI to .../leveldb-sys (#206)"*, deletes the same five files there, 33 minutes later | 2026-03-24 17:13 UTC |
| Neither commit carries the LICENSE file across | — |

The one `LICENSE` in the leveldb-sys repository is Google's BSD-3-Clause, at
`ffi/leveldb/LICENSE`; it covers the vendored C++ and says nothing about the
wrapper. (An MIT file briefly existed at that repo's root in 2022, but it
predates the wrapper by four years and was replaced seven minutes later with
Google's text — it was a mislabelled licence for the C++ fork, not a grant over
Rust code that did not yet exist.)

So rather than build from a repository with no terms, this directory takes the
same code from bedrock-rs, where it was published under Apache-2.0, and records
every change in `NOTICE`. Nothing that ships traces to an unlicensed file.

## Relationship to upstream

Taking `src/lib.rs` from bedrock-rs and applying the renames listed in `NOTICE`
reproduces upstream's current `src/lib.rs` byte for byte, apart from one line
inside a commented-out block. `ffi.cpp` and `ffi.h` differ from upstream only in
trailing whitespace and a final newline. The two trees are the same code.

If upstream adds a licence file, this directory can go away and
`scripts/setup-deps.sh` can go back to using the clone unmodified. See
`docs/upstream-leveldb-sys-license-pr.md` for the change to ask for.
