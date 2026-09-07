# Release Publishing Pipeline

**Date:** 2026-09-06
**Status:** Approved, pending implementation

## Context

ConstructCLI has no release mechanism. `README.md` states "No binaries are
published yet; building from source is currently the only way to get the tool,"
and the repository carries no tags. CI (`.github/workflows/ci.yml`) runs fmt,
clippy, and tests on three platforms, but produces no artifacts.

Building from source is unusually demanding here. The leveldb backend is
Mojang's own C++ implementation, so users need CMake and a C++ compiler on top
of a Rust toolchain. They must also run `scripts/setup-deps.sh` first, which
clones three upstream repositories at pinned commits and applies the patches in
`third_party/patches/`. That is a high barrier for an audience of Minecraft
players.

## Goal

Ship prebuilt binaries from a git tag, so that installing ConstructCLI requires
downloading one file. Structure the work so that adding crates.io publishing
later is an addition rather than a rewrite.

## Non-goals

- Publishing to crates.io. This is blocked on upstream work in other people's
  repositories; the blocker chain is documented below but not addressed here.
- Install scripts (`curl | sh`), a Homebrew tap, Scoop or WinGet manifests.
  All of these consume the artifacts this pipeline produces and can be added
  later without changing it.
- A hand-maintained `CHANGELOG.md`.
- Intel macOS (`x86_64-apple-darwin`) binaries. Explicitly dropped. Note that
  Apple Silicon binaries cannot run on Intel Macs — Rosetta translates x86 to
  ARM, not the reverse — so Intel Mac users build from source.

## Precondition: the leveldb-sys licence question

`third_party/README.md` already records that bedrock-crustaceans' `leveldb-sys`
declares no licence of its own: no top-level `LICENSE` file, no `license` field
in its `Cargo.toml`. It concludes that "its terms should be confirmed with its
authors before this project is published."

A GitHub Release is publishing. Attaching a binary that statically links that
code is redistribution in exactly the sense the missing licence leaves
unanswered. The vendored C++ under `ffi/leveldb/` is Google's BSD-3-Clause and
is not the problem; the gap is bedrock-crustaceans' own Rust wrapper —
`build.rs` and `src/`.

**This gates the first tag, not the pipeline.** Build and test the workflow
freely. Before pushing a real `v0.1.0`, resolve the question by one of:

1. Open an issue on bedrock-crustaceans/leveldb-sys asking for a licence file.
   Cheapest, and likely a one-line fix upstream.
2. Write our own bindings over the vendored BSD-3-Clause C++.
3. Move to the `rusty-leveldb` backend (see crates.io readiness below).

## Design

### Trigger and versioning

Releases are cut manually. The maintainer bumps `workspace.package.version` in
the root `Cargo.toml`, commits, tags `vX.Y.Z`, and pushes the tag. A new
workflow, `.github/workflows/release.yml`, fires on `push` of tags matching
`v*`.

Cargo has no built-in release command and nothing verifies that a tag matches
the version in `Cargo.toml`. The pipeline enforces that itself.

### Job graph

Three stages in one workflow file.

**`verify`** — runs on ubuntu, roughly 30 seconds, and exists to fail fast
before three platform builds are spent on a bad tag.

- Read `workspace.package.version` by parsing the root `Cargo.toml` directly,
  not via `cargo metadata`. `cargo metadata` has to resolve the
  `third_party/checkouts/` path dependencies, so it only works once
  `scripts/setup-deps.sh` has cloned three upstream repositories. Parsing the
  manifest keeps this gate genuinely cheap.
- Assert it equals `github.ref_name` with a leading `v` stripped. Fail with a
  message naming both values.
- Assert every patched dependency carries a licence, per the precondition
  above. This gate fails today, by design, and clears on its own once the
  question is answered.

An earlier draft of this spec also ran `cargo package --workspace --no-verify`
here, on the assumption it would pass today. It does not — both workspace
members carry versionless `path` dependencies, so it fails with "all
dependencies must have a version requirement specified when packaging."
Packaging cannot succeed until the crates.io blocker chain below is resolved,
so the check is omitted rather than left to break every release.

**`build`** — a matrix job, `needs: verify`. Each runner performs:

1. `actions/checkout@v4`
2. `dtolnay/rust-toolchain@stable`
3. `Swatinem/rust-cache@v2`
4. `./scripts/setup-deps.sh` (bash shell on Windows)
5. `cargo test --workspace --release`
6. `cargo build --release -p construct-cli`
7. Assemble the staging directory and archive it
8. `actions/upload-artifact@v4`

Symbols are stripped by `[profile.release] strip = true` in the root
`Cargo.toml` rather than by a workflow step. `strip` is not available on the
Windows MSVC toolchain and behaves differently between GNU and Apple binutils,
so doing it in Cargo keeps one behaviour across all three runners and makes
local `cargo build --release` output match what ships.

Tests run in `--release` rather than debug deliberately: one compilation serves
both the test run and the shipped binary. A debug test pass would roughly double
wall-clock time on the slowest leg, and the C++ leveldb build is already the
dominant cost.

**`publish`** — runs on ubuntu, `needs: build`, with `permissions: contents: write`.

- Download all artifacts.
- Generate a single `SHA256SUMS` file covering every archive.
- `gh release create` with all archives plus `SHA256SUMS`, and `--generate-notes`.
- Pass `--prerelease` when the tag contains a `-`, so `v0.1.0-rc.1` is marked
  correctly.

### Matrix

| Runner | Target | Archive |
|---|---|---|
| `ubuntu-22.04` | `x86_64-unknown-linux-gnu` | `.tar.gz` |
| `macos-14` | `aarch64-apple-darwin` | `.tar.gz` |
| `windows-latest` | `x86_64-pc-windows-msvc` | `.zip` |

Archives are named `construct-<version>-<target>.<ext>`, where `<version>` is
the tag with `v` stripped.

Cross-compilation is not used. leveldb is real C++ built through CMake, so each
target needs a native runner.

`ubuntu-22.04` is chosen over a current runner image to set the glibc floor at
2.35, which covers Ubuntu 22.04 and later plus Debian 12. Binaries built on
newer images fail on those systems with an unhelpful symbol error. The
alternative — building inside an `ubuntu:22.04` container on a current runner —
is immune to runner-image retirement but requires installing cmake, g++, git and
curl into the container. Should the `ubuntu-22.04` image be retired, switching to
a container is a change confined to one matrix entry.

### Archive contents

Each archive contains a single top-level directory holding:

- the `construct` binary (`construct.exe` on Windows)
- `LICENSE`
- `README.md`
- `THIRD-PARTY-NOTICES.md`

Shell completions are deliberately absent. ConstructCLI's completions are
dynamic: `construct completions bash` emits a registration stub that calls the
binary at runtime, so there are no static scripts to ship.

### Third-party notices

The binaries statically link Google's BSD-3-Clause leveldb, Apache-2.0 code from
bedrock-rs and nbtx, and the wider Rust dependency tree. All of those licences
require attribution on redistribution of compiled artifacts, so a notices file
is an obligation rather than a nicety.

`cargo-about` generates it during the `build` job from the real dependency
graph, so it cannot drift from what is actually linked. Configuration lives in
`about.toml` at the repository root.

Two consequences are expected and wanted:

- `cargo-about` will flag `leveldb-sys` as having no licence. This is the
  precondition above surfacing automatically, and the workflow should not
  suppress it. Once upstream resolves the licence, the flag clears on its own.
- The vendored C++ leveldb and zlib sit below Cargo's visibility and need
  hand-authored clarification entries in `about.toml`.

### Release notes

`gh release create --generate-notes` builds notes from commits and pull requests
since the previous tag. The repository already uses conventional commits
(`feat:`, `docs:`, `fix:`), so generated notes read well without a curated
changelog. A hand-maintained `CHANGELOG.md` can be added later if curated notes
become worth the upkeep.

### README changes

The Install section currently opens "No binaries are published yet; building
from source is currently the only way to get the tool." It is replaced by:

- A download table: platform, architecture, archive name.
- Instructions for verifying a download against `SHA256SUMS`.
- Instructions for placing the binary on `PATH`, per platform.
- The existing build-from-source instructions, demoted to a subsection aimed at
  contributors and at users on platforms with no published binary (notably Intel
  macOS).

## crates.io readiness

No `cargo publish` job is added. Recording why, so the constraint is documented
rather than rediscovered:

1. **The library name is taken.** `construct-core` exists on crates.io as an
   unrelated crate — a "hardware-agnostic agent runtime for the SuperInstance
   Construct API," v0.1.4, last published 2026-07-13. Names are global and
   first-come, so this workspace's library crate must be renamed to publish.
   `construct-cli` is free. The `construct` package name is taken by an
   abandoned 2015 macro crate, but this does not matter: `[[bin]] name =
   "construct"` is independent of the package name, so `cargo install
   construct-cli` would still install a binary called `construct`.

2. **Patch tables do not reach consumers.** The root `[patch.crates-io]` and
   `[patch."https://github.com/bedrock-crustaceans/leveldb-sys"]` sections apply
   only to the top-level workspace being built. Anyone running `cargo install`
   would get unpatched upstream `nbtx` and `leveldb-sys` — broken on macOS,
   broken on non-x86_64, and unable to encode any `.mcstructure` containing an
   empty list.

3. **Nothing under `bedrock_level` is published.** The chain is `construct-core`
   → `bedrock_level` → `bedrock_shared` → `bedrock_protocol_core` and
   `bedrock_macros`. None of those four exist on crates.io; bedrock-rs publishes
   no crates at all. `bedrock_level` also depends on `leveldb-sys` through a git
   URL, which crates.io rejects outright, so upstream would need to switch to a
   registry dependency as well.

4. **`leveldb-sys` is both occupied and unlicensed.** The `leveldb-sys` on
   crates.io is skade's unrelated crate (v2.0.9, 2021); bedrock-crustaceans' is
   a different codebase needing a different name. And it carries no licence, per
   the precondition above.

5. **`nbtx` is the clean one.** Published, Apache-2.0. It needs only the two
   local patches (empty-list serialization, recursion depth limit) landed
   upstream, or a renamed fork.

**The `rusty-leveldb` escape hatch.** `bedrock_level` carries a commented-out
`rusty-leveldb` feature, and `rusty-leveldb` is a live, actively maintained
crates.io crate. A pure-Rust backend would eliminate `leveldb-sys`, the CMake
and C++ toolchain requirement, the licensing blocker, and most cross-compilation
difficulty at once. It is commented out upstream, so it presumably does not work
as written, and replacing the storage engine beneath a tool that writes to real
Minecraft worlds is not a small change. It is nonetheless the only path that
makes crates.io simple rather than merely possible, and is worth a spike before
any of items 1 through 4 are attempted.

## Testing

Release workflows resist local testing: `act` cannot help, because the matrix
needs real macOS and Windows runners.

The workflow therefore carries a `workflow_dispatch` trigger alongside the tag
trigger. A manual run builds all three platforms and uploads the archives as
workflow artifacts, but skips the `publish` job entirely, because `publish` is
gated on the push event. There is deliberately no input to override this: a
control that cannot publish should not offer the choice. Nothing reaches the public, which matters: publishing a throwaway
`v0.0.1-test` release would raise exactly the same redistribution question as a
real one, and would need an exception carved into the licence gate to get past
it. A dry run needs no exception.

What a dry run leaves untested is the final `gh release create` call itself.
That is one well-documented command, and the first real tag exercises it.

The precondition gate is exercised separately and locally, by running
`scripts/check-release-preconditions.sh` with a mismatched tag, with a matching
tag, and with the licence present and absent.

## Files touched

- `.github/workflows/release.yml` — new
- `scripts/check-release-preconditions.sh` — new, the `verify` job's two gates
- `scripts/package-release.sh` — new, assembles one platform's archive
- `about.toml` — new, `cargo-about` configuration and clarification entries
- `about.hbs` — new, the notices template
- `Cargo.toml` — add `[profile.release] strip = true`
- `.gitignore` — ignore `/dist` and the generated `THIRD-PARTY-NOTICES.md`
- `README.md` — Install section rewritten
- `docs/manual-verification.md` — add a clean-machine check for the archives
- `.github/workflows/ci.yml` — unchanged

Logic lives in `scripts/` rather than in workflow YAML so that it can be run and
tested locally. The workflow only sequences it.

## To verify during implementation

- `Swatinem/rust-cache` behaviour across the `verify` and `build` jobs, so the
  C++ leveldb build is not recompiled unnecessarily.
- That `scripts/setup-deps.sh` runs correctly under the release workflow's
  Windows shell, as it does in CI today.
