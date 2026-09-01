# ConstructCLI Stages 0–1 (Spike + Read Path) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prove the `bedrock_level` dependency works against a real world, then ship the read half of ConstructCLI — `construct worlds`, `construct list`, and `construct export`.

**Architecture:** A Cargo workspace with `construct-core` (all logic, no CLI concepts) and `construct-cli` (clap, formatting, exit codes). Structure bytes are moved verbatim: a structure's leveldb value is byte-identical to a `.mcstructure` file, so nothing in this plan parses NBT except the small `level.dat` header reader. Database access sits behind a `StructureStore` trait so the backend can be swapped later.

**Tech Stack:** Rust stable (edition 2024), `bedrock_level` from `bedrock-rs` (git, commit-pinned; FFI to Mojang's leveldb fork, needs CMake + a C++ compiler), `clap` 4, `serde`/`serde_json`, `toml`, `directories`, `thiserror` (core), `anyhow` (cli), `tempfile`.

**Spec:** `docs/superpowers/specs/2026-08-31-constructcli-design.md` — read it before starting. This plan implements §14 stages 0 and 1 only. Stages 2–4 (Construct integration, merge, world delete) get their own plans.

## Global Constraints

- **`construct-core` never references CLI concepts.** No arguments, no stdout, no exit codes, no `println!`. It returns typed values and `thiserror` errors and never panics. `construct-cli` owns `clap`, all formatting, and all exit codes, and uses `anyhow` for context.
- **Rust edition 2024** in every crate, matching `bedrock-rs`. Requires **Rust 1.88 or newer** — Tasks 5 and 10 use `if let … && …` let-chains, which are stable only from 1.88. (Verified installed: 1.98.0.)
- **The database backend comes from patched forks, not upstream.** The stage 0 spike proved the upstream pins do not compile on macOS at all, or on any non-x86_64 target — four independent portability defects (§3 of the spec). Patched branches are prepared and verified building from a clean cargo cache; their published URLs are filled in at Task 9. Pin by explicit commit, never a branch.
- **Reads never open a world's database.** Opening a leveldb database runs recovery and rewrites it, measured in the spike. Every read copies `db/` to a temp directory and opens the copy — always, not only when the world is in use. See spec §8.
- **Never open a real world's database in place during development or tests.** Always copy to a temp directory first. A guard helper (Task 9) refuses any database path outside a temp directory.
- **Exit codes:** `0` success · `1` failure · `2` usage error · `3` not found · `4` world in use. Ambiguity is `2`, not `3` — the target exists, the reference was underspecified.
- **Never guess between candidates.** Ambiguous world references, ambiguous structure names, and ambiguous installations are all errors that print the qualified forms. Never silently pick.
- **`--json` prints exactly one JSON document to stdout and nothing else.** Every payload carries `"schema": 1`. Warnings go to stderr as plain text *and* into a `"warnings"` array in the payload.
- **Structure key format** (confirmed against a real world): `structuretemplate_` + `namespace:name`, e.g. `structuretemplate_mystructure:copy`. A bare name means the `mystructure` namespace.
- **Test worlds on this machine** live under `~/Library/Application Support/mcpelauncher/games/com.mojang/minecraftWorlds/`. `Ssu8ww1SFbM=` ("construct show", 2.4 MB) contains `structuretemplate_mystructure:copy` and is the spike target. `Amelix CMP` (35 MB, four behavior packs, folder name contains a space) has **no** structures but is the `level.dat` and discovery fixture.
- **License MIT**, matching Construct. Binary name is `construct`.

---

## File Structure

```
ConstructCLI/
├── Cargo.toml                          # workspace manifest
├── rust-toolchain.toml                 # pins stable
├── .gitignore
├── README.md
├── .github/workflows/ci.yml            # macOS + Linux + Windows
├── spike/                              # THROWAWAY — deleted in Task 2
│   ├── Cargo.toml
│   └── src/main.rs
└── crates/
    ├── construct-core/
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs                  # re-exports; module wiring
    │       ├── error.rs                # CoreError + Result
    │       ├── config.rs               # Config load + precedence
    │       ├── leveldat.rs             # level.dat header, LastPlayed, experiments read
    │       ├── discovery/
    │       │   ├── mod.rs              # Installation, WorldRoot, World, discover()
    │       │   ├── platform.rs         # per-OS candidate root tables
    │       │   └── reference.rs        # WorldRef parse + resolve (incl. path-as-reference)
    │       ├── store/
    │       │   ├── mod.rs              # StructureStore trait, open_world_store()
    │       │   ├── key.rs              # structuretemplate_ encode/decode
    │       │   ├── bedrock.rs          # bedrock_level backend
    │       │   └── snapshot.rs         # copy db/ to temp when the world is in use
    │       └── catalog.rs              # StructureEntry, Source, resolve_structure()
    └── construct-cli/
        ├── Cargo.toml
        └── src/
            ├── main.rs                 # dispatch + exit codes
            ├── cli.rs                  # clap definitions
            ├── output.rs               # Out: human/json rendering, warning channel
            └── commands/
                ├── mod.rs
                ├── worlds.rs
                ├── list.rs
                └── export.rs
```

Each file has one responsibility. `discovery` is split three ways because platform tables, reference grammar, and world enumeration change for unrelated reasons. `store` is split so the trait, the key grammar, the FFI backend, and the snapshot fallback can each be reviewed and tested alone.

---

## Task 1: The spike (throwaway)

This task exists to answer one question before any real code is written: **does `bedrock_level` open a real Bedrock world, enumerate `structuretemplate_` keys, and round-trip a structure's bytes?** The spec's §14 makes this gate everything. Output is an answer, not code — the crate is deleted in Task 2.

Not TDD. This is a probe.

**Files:**
- Create: `spike/Cargo.toml`
- Create: `spike/src/main.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: nothing in code. Produces four **findings** that later tasks depend on, recorded in the commit message: (1) does the FFI build on this machine, (2) the real count of `structuretemplate_` keys in the fixture world, (3) whether written bytes read back identical, (4) what error a locked database actually returns.

- [ ] **Step 1: Install the Rust toolchain**

There is no `cargo` on this machine. CMake 4.4.2 and Apple clang 21 are already present, which is what `leveldb-sys` needs.

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
rustc --version    # must be 1.85.0 or newer for edition 2024
cmake --version    # already verified: 4.4.2
```

- [ ] **Step 2: Create the throwaway spike crate**

`spike/Cargo.toml`:

```toml
[package]
name = "spike"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
bedrock_level = { git = "https://github.com/bedrock-crustaceans/bedrock-rs", package = "bedrock_level", rev = "2d9e4087a207bdcbad6e4cdc83de46e94712b2f4" }
```

- [ ] **Step 3: Write the probe**

Note three API details confirmed by reading the dependency's source — get these wrong and it will not compile:

1. `Database::open` takes `AsRef<str>`, **not** a path. Convert with `.to_str()`.
2. The path must point at the world's `db` directory, not the world folder.
3. `Iterator` is implemented for `&mut Keys`, not `Keys`. You must write `for kv in &mut keys`, not `for kv in keys`.

`spike/src/main.rs`:

```rust
use bedrock_level::db::Database;
use std::path::{Path, PathBuf};

const PREFIX: &[u8] = b"structuretemplate_";

/// Recursively copy a directory. The spike must never touch the real world.
fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

fn main() {
    let world = PathBuf::from(std::env::args().nth(1).expect("usage: spike <world-dir>"));
    assert!(world.join("level.dat").is_file(), "not a world: {}", world.display());

    // 1. Copy the world so the original is never opened.
    let tmp = std::env::temp_dir().join("construct-spike");
    let _ = std::fs::remove_dir_all(&tmp);
    copy_dir(&world, &tmp).expect("copy failed");
    let db_path = tmp.join("db");
    println!("copied to {}", db_path.display());

    // 2. Open and enumerate.
    let db = Database::open(db_path.to_str().expect("non-UTF-8 path")).expect("open failed");
    let mut found: Vec<(Vec<u8>, usize)> = Vec::new();
    let mut total = 0usize;
    {
        let mut keys = db.keys();
        for kv in &mut keys {
            total += 1;
            let k = kv.key().to_vec();
            if k.starts_with(PREFIX) {
                found.push((k, kv.value().len()));
            }
        }
    }
    println!("FINDING total keys: {total}");
    println!("FINDING structuretemplate_ keys: {}", found.len());
    for (k, len) in &found {
        println!("  {} ({len} bytes)", String::from_utf8_lossy(k));
    }
    assert!(!found.is_empty(), "no structures in this world — wrong fixture");

    // 3. Round-trip: read bytes, write under a new key, read back, compare.
    let (src_key, _) = &found[0];
    let original: Vec<u8> = db.get(src_key).expect("get failed").expect("key vanished").to_vec();
    std::fs::write(tmp.join("spike-out.mcstructure"), &original).expect("write file failed");

    let dst_key = b"structuretemplate_mystructure:spike_roundtrip".to_vec();
    db.insert(&dst_key, &original).expect("insert failed");
    let read_back: Vec<u8> = db.get(&dst_key).expect("get failed").expect("insert did not stick").to_vec();
    println!("FINDING round-trip identical: {}", read_back == original);
    assert_eq!(read_back, original, "bytes changed across write/read");

    // 4. Does the value look like a .mcstructure? Little-endian NBT starts with 0x0A.
    println!("FINDING first byte 0x{:02x} (0x0a = TAG_Compound)", original[0]);

    // 5. What does a second open of the same database do?
    drop(db);
    let held = Database::open(db_path.to_str().unwrap()).expect("reopen after drop failed");
    match Database::open(db_path.to_str().unwrap()) {
        Ok(_) => println!("FINDING second concurrent open: SUCCEEDED (no lock error in-process)"),
        Err(e) => println!("FINDING second concurrent open: {e:?}"),
    }
    drop(held);

    println!("\nspike OK");
}
```

- [ ] **Step 4: Run it against the fixture world**

The first build compiles leveldb from C++ and takes several minutes.

```bash
cd spike
cargo run --release -- ~/Library/Application\ Support/mcpelauncher/games/com.mojang/minecraftWorlds/Ssu8ww1SFbM=
```

Expected: `spike OK`, with `structuretemplate_ keys: 1` or more (a raw grep of the `.ldb` files found exactly one, `structuretemplate_mystructure:copy`, but leveldb compresses most blocks so the true count can be higher), `round-trip identical: true`, and `first byte 0x0a`.

**If the build fails**, the problem is the C++ toolchain, not the code — check CMake is on `PATH`. **If enumeration finds zero keys**, the fixture is wrong, not the library; try `6UlYZgSrhQI=` instead. **If round-trip differs**, stop and report — that invalidates the byte-transparency property in §4 of the spec and the whole staging argument with it.

- [ ] **Step 5: Probe the real lock behaviour**

This is the finding that Task 10 depends on, and it cannot be obtained from a copy. Start Minecraft via mcpelauncher, load the "construct show" world, leave it running, then:

```bash
cargo run --release -- ~/Library/Application\ Support/mcpelauncher/games/com.mojang/minecraftWorlds/Ssu8ww1SFbM=
```

The copy still succeeds, so the spike will pass. What matters is a separate check — open the **live** path directly and record the exact error:

```bash
cargo run --release -- /nonexistent 2>&1 | head -3   # sanity: what a plain failure looks like
```

Then add a one-line variant that calls `Database::open` on the live `db` directory and prints `{e:?}`. Record the exact error text.

Expect something containing `already held by process` on macOS and Linux (leveldb's `env_posix.cc` produces `IOError("lock " + fname, "already held by process")`). **Windows uses a different implementation (`env_win.cc`) with a different message**, so Task 10 must not depend on this exact string. Note also that POSIX `fcntl` locks are per-process, which is why step 3's in-process double-open may well succeed — that is expected and is not the case being probed here.

- [ ] **Step 6: Record the findings and commit**

```bash
cd .. && git add spike
git commit -m "Spike: verify bedrock_level against a real world

Throwaway probe, deleted in the next commit. Findings:
- leveldb-sys builds on macOS with CMake 4.4.2 + Apple clang 21
- <N> structuretemplate_ keys enumerated in Ssu8ww1SFbM=
- round-trip write/read is byte-identical
- values begin 0x0a (little-endian NBT TAG_Compound)
- locked-database error text: <exact text>"
```

---

## Task 2: Workspace scaffolding and CI

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `.gitignore`, `.github/workflows/ci.yml`
  (`README.md` is **not** here — Task 15 creates it, with its full content specified there)
- Create: `crates/construct-core/Cargo.toml`, `crates/construct-core/src/lib.rs`
- Create: `crates/construct-cli/Cargo.toml`, `crates/construct-cli/src/main.rs`
- Delete: `spike/`

**Interfaces:**
- Consumes: Task 1's confirmation that the dependency builds.
- Produces: a workspace where `cargo test` and `cargo clippy` run clean, and `construct-core` and `construct-cli` exist as crates. Every later task adds to these.

- [ ] **Step 1: Delete the spike**

Its output was an answer, recorded in Task 1's commit message. Keeping the code would leave an unmaintained second copy of the database-opening logic.

```bash
git rm -r spike
```

- [ ] **Step 2: Write the workspace manifest**

`Cargo.toml`:

```toml
[workspace]
resolver = "3"
members = ["crates/construct-core", "crates/construct-cli"]

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "MIT"
repository = "https://github.com/ForestOfLight/ConstructCLI"

[workspace.dependencies]
# The database backend is deliberately absent here. It arrives in Task 9, once the
# patched forks the spike showed to be necessary have somewhere to be fetched from.
# Nothing in Tasks 2-8 touches a database.
thiserror = "2.0"
anyhow = "1.0"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
toml = "1.0"
directories = "6.0"
clap = { version = "4.6", features = ["derive"] }
tempfile = "3.27"
```

`rust-toolchain.toml`:

```toml
[toolchain]
channel = "stable"
components = ["clippy", "rustfmt"]
```

`.gitignore`:

```
/target
**/*.rs.bk
.DS_Store
```

- [ ] **Step 3: Create the two crates**

`crates/construct-core/Cargo.toml`:

```toml
[package]
name = "construct-core"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
thiserror.workspace = true
serde = { workspace = true }
serde_json.workspace = true
toml.workspace = true
directories.workspace = true
tempfile.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

`crates/construct-core/src/lib.rs`:

```rust
//! Core logic for ConstructCLI.
//!
//! This crate knows nothing about command-line arguments, stdout, or exit codes.
//! It returns typed values and typed errors, and it does not panic.

pub mod error;

pub use error::{CoreError, Result};
```

`crates/construct-cli/Cargo.toml`:

```toml
[package]
name = "construct-cli"
version.workspace = true
edition.workspace = true
license.workspace = true

[[bin]]
name = "construct"
path = "src/main.rs"

[dependencies]
construct-core = { path = "../construct-core" }
anyhow.workspace = true
clap.workspace = true
serde = { workspace = true }
serde_json.workspace = true
```

`crates/construct-cli/src/main.rs`:

```rust
fn main() {
    println!("construct {}", env!("CARGO_PKG_VERSION"));
}
```

- [ ] **Step 4: Write the error type**

`crates/construct-core/src/error.rs`:

```rust
use std::path::PathBuf;
use thiserror::Error;

/// Every failure `construct-core` can produce.
///
/// Variants carry the data a caller needs to render a good message — the
/// candidates for an ambiguity, the paths probed for a missing root — rather
/// than a pre-formatted string.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("no Minecraft installation found")]
    NoInstallations { probed: Vec<PathBuf> },

    #[error("world not found: {reference}")]
    WorldNotFound { reference: String, near: Vec<String> },

    #[error("{reference} matches {} worlds", candidates.len())]
    AmbiguousWorld { reference: String, candidates: Vec<String> },

    #[error("structure not found: {name}")]
    StructureNotFound { name: String, near: Vec<String> },

    #[error("structure {name} exists in both a world and a pack")]
    AmbiguousStructure { name: String },

    #[error("more than one installation; no default configured")]
    AmbiguousInstallation { candidates: Vec<String> },

    #[error("world is in use: {}", world.display())]
    WorldInUse { world: PathBuf },

    #[error("not enough space to snapshot {}: need {need} bytes, {available} available", world.display())]
    InsufficientSpace { world: PathBuf, need: u64, available: u64 },

    #[error("{} already exists", path.display())]
    TargetExists { path: PathBuf },

    #[error("database error: {0}")]
    Db(String),

    #[error("malformed level.dat at {}: {reason}", path.display())]
    BadLevelDat { path: PathBuf, reason: String },

    #[error("config error in {}: {reason}", path.display())]
    BadConfig { path: PathBuf, reason: String },

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, CoreError>;
```

- [ ] **Step 5: Verify the workspace builds and is clean**

```bash
cargo build --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo run -p construct-cli -- --help 2>/dev/null || cargo run -p construct-cli
```

Expected: builds, no clippy warnings, and the binary prints `construct 0.1.0`.

- [ ] **Step 6: Write CI**

`.github/workflows/ci.yml`:

```yaml
name: CI
on:
  push:
    branches: [master, main]
  pull_request:

jobs:
  test:
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt
      - uses: Swatinem/rust-cache@v2
      # CMake and a C++ compiler are preinstalled on all three runner images;
      # leveldb-sys vendors the leveldb source, so no submodule checkout is needed.
      - run: cmake --version
      - run: cargo fmt --all --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo test --workspace
```

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "Scaffold the Cargo workspace and CI

Two crates: construct-core holds all logic, construct-cli owns clap and
exit codes. bedrock_level is pinned to a commit rather than a branch.
CI runs macOS, Linux, and Windows so the leveldb C++ build is exercised
everywhere from the start.

Removes the spike, whose findings are in the previous commit message."
```

---

## Task 3: Reading `level.dat`

Discovery needs `LastPlayed`, and `status` (stage 2) will need the `experiments` compound. Both live in `level.dat`, which is a small little-endian NBT document behind an 8-byte header.

**Format, measured from `Amelix CMP/level.dat` (2935 bytes):** a little-endian `i32` version (`10`), a little-endian `i32` payload length (`2927`, and `8 + 2927 == 2935` exactly), then the payload — a `TAG_Compound` (`0x0a`) with an empty root name. `LastPlayed` is a `TAG_Long` holding unix seconds (`1741501762` → 2025-03-08).

**Files:**
- Create: `crates/construct-core/src/leveldat.rs`
- Modify: `crates/construct-core/src/lib.rs`
- Modify: `crates/construct-core/Cargo.toml` (add `nbtx`)
- Test: inline `#[cfg(test)]` module in `leveldat.rs`

**Interfaces:**
- Consumes: `CoreError` from Task 2.
- Produces:
  - `pub struct LevelDat { pub version: i32, pub root: nbtx::Value }`
  - `pub fn read(path: &Path) -> Result<LevelDat>`
  - `pub fn parse(bytes: &[u8], path: &Path) -> Result<LevelDat>`
  - `impl LevelDat { pub fn last_played(&self) -> Option<i64>; pub fn experiments(&self) -> Option<BTreeMap<String, i8>>; }`

- [ ] **Step 1: Add the nbtx dependency**

In the workspace `Cargo.toml` under `[workspace.dependencies]`:

```toml
nbtx = "3.0.1"
```

In `crates/construct-core/Cargo.toml` under `[dependencies]`:

```toml
nbtx.workspace = true
```

- [ ] **Step 2: Write the failing tests**

These build a `level.dat` byte-for-byte rather than depending on a real file, so they run in CI. Note `nbtx::from_le_bytes::<T, _>(&mut reader)` takes `&mut &[u8]` — the doc comment claiming it also returns a byte count is stale; it returns `Result<T, Error>`.

Create `crates/construct-core/src/leveldat.rs` with only this test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::Path;

    /// Build a level.dat: 8-byte header then a little-endian NBT compound.
    fn build(version: i32, root: nbtx::Value) -> Vec<u8> {
        let payload = nbtx::to_le_bytes(&root).unwrap();
        let mut out = Vec::new();
        out.extend_from_slice(&version.to_le_bytes());
        out.extend_from_slice(&(payload.len() as i32).to_le_bytes());
        out.extend_from_slice(&payload);
        out
    }

    fn compound(pairs: Vec<(&str, nbtx::Value)>) -> nbtx::Value {
        nbtx::Value::Compound(
            pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect::<HashMap<_, _>>(),
        )
    }

    #[test]
    fn reads_last_played() {
        let bytes = build(10, compound(vec![("LastPlayed", nbtx::Value::Long(1741501762))]));
        let dat = parse(&bytes, Path::new("x")).unwrap();
        assert_eq!(dat.version, 10);
        assert_eq!(dat.last_played(), Some(1741501762));
    }

    #[test]
    fn missing_last_played_is_none() {
        let bytes = build(10, compound(vec![("SomethingElse", nbtx::Value::Int(1))]));
        assert_eq!(parse(&bytes, Path::new("x")).unwrap().last_played(), None);
    }

    #[test]
    fn reads_all_three_experiment_flags() {
        // These are the exact keys and values measured in Amelix CMP/level.dat.
        let experiments = compound(vec![
            ("experiments_ever_used", nbtx::Value::Byte(1)),
            ("gametest", nbtx::Value::Byte(1)),
            ("saved_with_toggled_experiments", nbtx::Value::Byte(1)),
        ]);
        let bytes = build(10, compound(vec![("experiments", experiments)]));
        let got = parse(&bytes, Path::new("x")).unwrap().experiments().unwrap();
        assert_eq!(got.get("gametest"), Some(&1));
        assert_eq!(got.get("experiments_ever_used"), Some(&1));
        assert_eq!(got.get("saved_with_toggled_experiments"), Some(&1));
    }

    #[test]
    fn preserves_unrelated_experiment_siblings() {
        // Stage 2 rewrites this compound. Anything it cannot see, it will destroy.
        let experiments = compound(vec![
            ("gametest", nbtx::Value::Byte(1)),
            ("data_driven_biomes", nbtx::Value::Byte(1)),
        ]);
        let bytes = build(10, compound(vec![("experiments", experiments)]));
        let got = parse(&bytes, Path::new("x")).unwrap().experiments().unwrap();
        assert_eq!(got.get("data_driven_biomes"), Some(&1));
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn no_experiments_compound_is_none() {
        let bytes = build(10, compound(vec![("LastPlayed", nbtx::Value::Long(1))]));
        assert_eq!(parse(&bytes, Path::new("x")).unwrap().experiments(), None);
    }

    #[test]
    fn rejects_a_file_too_short_for_the_header() {
        let err = parse(&[0u8; 4], Path::new("x")).unwrap_err();
        assert!(matches!(err, CoreError::BadLevelDat { .. }));
    }

    #[test]
    fn rejects_a_declared_length_longer_than_the_file() {
        let mut bytes = build(10, compound(vec![("LastPlayed", nbtx::Value::Long(1))]));
        bytes[4..8].copy_from_slice(&9999i32.to_le_bytes());
        let err = parse(&bytes, Path::new("x")).unwrap_err();
        assert!(matches!(err, CoreError::BadLevelDat { .. }));
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

```bash
cargo test -p construct-core leveldat
```

Expected: FAIL to compile — `parse`, `LevelDat`, and `leveldat` module are not defined.

- [ ] **Step 4: Write the implementation**

Prepend to `crates/construct-core/src/leveldat.rs`:

```rust
//! Reading `level.dat`, which is little-endian NBT behind an 8-byte header.

use crate::error::{CoreError, Result};
use std::collections::BTreeMap;
use std::path::Path;

/// A parsed `level.dat`.
///
/// The root is kept as a generic [`nbtx::Value`] rather than a typed struct so
/// that keys this tool does not understand survive a read. Stage 2 rewrites the
/// `experiments` compound in place, and a wholesale replacement would silently
/// disable whatever else the world had enabled.
#[derive(Debug, Clone)]
pub struct LevelDat {
    pub version: i32,
    pub root: nbtx::Value,
}

/// Reads and parses the `level.dat` at `path`.
pub fn read(path: &Path) -> Result<LevelDat> {
    let bytes = std::fs::read(path)?;
    parse(&bytes, path)
}

/// Parses `level.dat` bytes. Separated from [`read`] so tests need no files.
pub fn parse(bytes: &[u8], path: &Path) -> Result<LevelDat> {
    let bad = |reason: &str| CoreError::BadLevelDat {
        path: path.to_path_buf(),
        reason: reason.to_string(),
    };

    if bytes.len() < 8 {
        return Err(bad("shorter than the 8-byte header"));
    }
    let version = i32::from_le_bytes(bytes[0..4].try_into().expect("4 bytes"));
    let declared = i32::from_le_bytes(bytes[4..8].try_into().expect("4 bytes"));
    let declared = usize::try_from(declared).map_err(|_| bad("negative payload length"))?;

    let payload = bytes
        .get(8..8 + declared)
        .ok_or_else(|| bad("declared payload length runs past the end of the file"))?;

    let mut cursor: &[u8] = payload;
    let root: nbtx::Value =
        nbtx::from_le_bytes(&mut cursor).map_err(|e| bad(&format!("invalid NBT: {e}")))?;

    Ok(LevelDat { version, root })
}

impl LevelDat {
    fn field(&self, name: &str) -> Option<&nbtx::Value> {
        match &self.root {
            nbtx::Value::Compound(map) => map.get(name),
            _ => None,
        }
    }

    /// Unix seconds of the last session, if the field is present and a Long.
    pub fn last_played(&self) -> Option<i64> {
        match self.field("LastPlayed") {
            Some(nbtx::Value::Long(v)) => Some(*v),
            _ => None,
        }
    }

    /// The `experiments` compound's byte flags, or `None` when the world has no
    /// such compound. Ordered so callers render it deterministically.
    pub fn experiments(&self) -> Option<BTreeMap<String, i8>> {
        let nbtx::Value::Compound(map) = self.field("experiments")? else {
            return None;
        };
        Some(
            map.iter()
                .filter_map(|(k, v)| match v {
                    nbtx::Value::Byte(b) => Some((k.clone(), *b)),
                    _ => None,
                })
                .collect(),
        )
    }
}
```

Add to `crates/construct-core/src/lib.rs`:

```rust
pub mod leveldat;
```

- [ ] **Step 5: Run the tests to verify they pass**

```bash
cargo test -p construct-core leveldat
```

Expected: 7 passed.

- [ ] **Step 6: Verify against the real world**

The synthetic tests prove the parser is self-consistent. This proves it matches Minecraft. Add a test that is skipped when the file is absent, so CI stays green:

```rust
#[test]
fn parses_a_real_world_level_dat() {
    let path = dirs_next_to_home("Library/Application Support/mcpelauncher/games/com.mojang/minecraftWorlds/Amelix CMP/level.dat");
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!("skipping: no local world at {}", path.display());
        return;
    };
    let dat = parse(&bytes, &path).unwrap();
    assert_eq!(dat.version, 10);
    assert_eq!(dat.last_played(), Some(1741501762));
    let exp = dat.experiments().expect("Amelix CMP has an experiments compound");
    assert_eq!(exp.get("gametest"), Some(&1));
    assert_eq!(exp.get("experiments_ever_used"), Some(&1));
    assert_eq!(exp.get("saved_with_toggled_experiments"), Some(&1));
}

fn dirs_next_to_home(rel: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(rel)
}
```

Run `cargo test -p construct-core leveldat -- --nocapture`. Expected: 8 passed, with the real-world assertions actually running on this machine.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "Read level.dat: header, LastPlayed, and the experiments compound

The root is kept as a generic nbtx::Value rather than a typed struct, so
that keys this tool does not understand survive a read — stage 2 rewrites
the experiments compound in place and must not drop siblings.

Verified against Amelix CMP: version 10, LastPlayed 1741501762, and all
three measured experiment flags."
```

---

## Task 4: Platform root tables

Where worlds and development packs live, per platform. Pure data plus existence checks, so it is table-testable against synthetic directory trees.

**Files:**
- Create: `crates/construct-core/src/discovery/platform.rs`
- Create: `crates/construct-core/src/discovery/mod.rs` (module declaration only for now)
- Modify: `crates/construct-core/src/lib.rs`
- Test: inline `#[cfg(test)]` module in `platform.rs`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub struct Candidate { pub name: String, pub dev_pack_root: PathBuf, pub world_root_globs: Vec<PathBuf> }`
  - `pub fn candidates(home: &Path, appdata: Option<&Path>, localappdata: Option<&Path>) -> Vec<Candidate>`
  - `pub struct WorldRoot { pub account: Option<String>, pub path: PathBuf }`
  - `pub struct Installation { pub name: String, pub dev_pack_root: PathBuf, pub world_roots: Vec<WorldRoot> }`
  - `pub fn resolve(candidates: Vec<Candidate>) -> Vec<Installation>`

Taking the base directories as parameters rather than reading the environment is what makes this testable — every test points it at a temp directory.

- [ ] **Step 1: Write the failing tests**

Create `crates/construct-core/src/discovery/platform.rs` with only this test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn touch_world(root: &Path, folder: &str, name: &str) {
        let dir = root.join(folder);
        fs::create_dir_all(dir.join("db")).unwrap();
        fs::write(dir.join("level.dat"), b"stub").unwrap();
        fs::write(dir.join("levelname.txt"), name).unwrap();
    }

    #[test]
    fn gdk_release_has_shared_dev_packs_and_per_account_world_roots() {
        let tmp = tempfile::tempdir().unwrap();
        let appdata = tmp.path().join("AppData/Roaming");
        let base = appdata.join("Minecraft Bedrock/Users");
        fs::create_dir_all(base.join("Shared/games/com.mojang/development_behavior_packs")).unwrap();
        touch_world(&base.join("Shared/games/com.mojang/minecraftWorlds"), "AAA=", "Shared World");
        touch_world(&base.join("2533274801234567/games/com.mojang/minecraftWorlds"), "BBB=", "My World");

        let found = resolve(candidates(tmp.path(), Some(&appdata), None));
        let release = found.iter().find(|i| i.name == "release").expect("release installation");
        assert!(release.dev_pack_root.ends_with("Users/Shared/games/com.mojang"));
        assert_eq!(release.world_roots.len(), 2, "one world root per account");
        // With several roots the account segment must be present, or qualified
        // references cannot address them.
        assert!(release.world_roots.iter().all(|r| r.account.is_some()));
    }

    #[test]
    fn a_single_world_root_carries_no_account_segment() {
        // macOS and Linux must never display an account segment.
        let tmp = tempfile::tempdir().unwrap();
        let com_mojang = tmp.path().join("Library/Application Support/mcpelauncher/games/com.mojang");
        fs::create_dir_all(&com_mojang).unwrap();
        touch_world(&com_mojang.join("minecraftWorlds"), "Ssu8ww1SFbM=", "construct show");

        let found = resolve(candidates(tmp.path(), None, None));
        let mcpe = found.iter().find(|i| i.name == "mcpelauncher").expect("mcpelauncher");
        assert_eq!(mcpe.world_roots.len(), 1);
        assert_eq!(mcpe.world_roots[0].account, None);
    }

    #[test]
    fn absent_roots_are_omitted_without_error() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(resolve(candidates(tmp.path(), None, None)).is_empty());
    }

    #[test]
    fn preview_and_release_are_separate_installations() {
        let tmp = tempfile::tempdir().unwrap();
        let appdata = tmp.path().join("AppData/Roaming");
        for product in ["Minecraft Bedrock", "Minecraft Bedrock Preview"] {
            let base = appdata.join(product).join("Users/Shared/games/com.mojang");
            fs::create_dir_all(base.join("development_behavior_packs")).unwrap();
            touch_world(&base.join("minecraftWorlds"), "AAA=", "W");
        }
        let names: Vec<_> = resolve(candidates(tmp.path(), Some(&appdata), None))
            .into_iter().map(|i| i.name).collect();
        assert!(names.contains(&"release".to_string()));
        assert!(names.contains(&"preview".to_string()));
    }

    #[test]
    fn legacy_uwp_is_probed_for_worlds() {
        // Pre-migration worlds are exactly the ones worth mining.
        let tmp = tempfile::tempdir().unwrap();
        let local = tmp.path().join("AppData/Local");
        let base = local.join("Packages/Microsoft.MinecraftUWP_8wekyb3d8bbwe/LocalState/games/com.mojang");
        fs::create_dir_all(&base).unwrap();
        touch_world(&base.join("minecraftWorlds"), "OLD=", "Old World");

        let found = resolve(candidates(tmp.path(), None, Some(&local)));
        assert!(found.iter().any(|i| i.name == "legacy"));
    }

    #[test]
    fn a_root_without_a_minecraft_worlds_dir_still_counts_for_dev_packs() {
        let tmp = tempfile::tempdir().unwrap();
        let com_mojang = tmp.path().join(".local/share/mcpelauncher/games/com.mojang");
        fs::create_dir_all(com_mojang.join("development_behavior_packs")).unwrap();
        let found = resolve(candidates(tmp.path(), None, None));
        let mcpe = found.iter().find(|i| i.name == "mcpelauncher").expect("mcpelauncher");
        assert!(mcpe.world_roots.is_empty());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p construct-core platform
```

Expected: FAIL to compile — `candidates`, `resolve`, `Installation`, `WorldRoot` are not defined.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/construct-core/src/discovery/platform.rs`:

```rust
//! Where Minecraft keeps worlds and development packs, per platform.
//!
//! Windows moved from UWP to GDK in Minecraft 1.21.120. Under GDK, worlds live
//! per Xbox account while development packs live in `Users\Shared` — so worlds
//! and dev packs sit in *different* roots and there may be several world roots
//! on one machine. Legacy UWP is still probed for worlds.

use std::path::{Path, PathBuf};

/// A place worth probing, before existence is checked.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub name: String,
    pub dev_pack_root: PathBuf,
    /// Directories that may each contain a `minecraftWorlds`. More than one
    /// means the installation is per-account.
    pub world_root_parents: Vec<PathBuf>,
    /// True when `world_root_parents` was produced by expanding accounts, so a
    /// single surviving root still deserves an account label.
    pub per_account: bool,
}

/// One world root: a `minecraftWorlds` directory, optionally owned by an account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldRoot {
    pub account: Option<String>,
    pub path: PathBuf,
}

/// A resolved Minecraft installation that exists on disk.
#[derive(Debug, Clone)]
pub struct Installation {
    pub name: String,
    pub dev_pack_root: PathBuf,
    pub world_roots: Vec<WorldRoot>,
}

/// Every location worth probing on this platform.
///
/// Base directories are parameters rather than environment reads so that tests
/// can point the whole table at a temp directory.
pub fn candidates(home: &Path, appdata: Option<&Path>, localappdata: Option<&Path>) -> Vec<Candidate> {
    let mut out = Vec::new();

    // Windows GDK: release and preview.
    if let Some(appdata) = appdata {
        for (name, product) in [
            ("release", "Minecraft Bedrock"),
            ("preview", "Minecraft Bedrock Preview"),
        ] {
            let users = appdata.join(product).join("Users");
            out.push(Candidate {
                name: name.to_string(),
                dev_pack_root: users.join("Shared/games/com.mojang"),
                world_root_parents: account_dirs(&users),
                per_account: true,
            });
        }
    }

    // Windows legacy UWP.
    if let Some(local) = localappdata {
        let base = local
            .join("Packages/Microsoft.MinecraftUWP_8wekyb3d8bbwe/LocalState/games/com.mojang");
        out.push(Candidate {
            name: "legacy".to_string(),
            dev_pack_root: base.clone(),
            world_root_parents: vec![base],
            per_account: false,
        });
    }

    // mcpelauncher: macOS then Linux. Both may be probed; only one will exist.
    for rel in [
        "Library/Application Support/mcpelauncher/games/com.mojang",
        ".local/share/mcpelauncher/games/com.mojang",
    ] {
        let base = home.join(rel);
        out.push(Candidate {
            name: "mcpelauncher".to_string(),
            dev_pack_root: base.clone(),
            world_root_parents: vec![base],
            per_account: false,
        });
    }

    out
}

/// Under GDK each Xbox account gets its own directory beside `Shared`.
fn account_dirs(users: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(users) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| e.path().join("games/com.mojang"))
        .collect();
    dirs.sort();
    dirs
}

/// Keeps the candidates that exist on disk. A missing root is absent, not an error.
pub fn resolve(candidates: Vec<Candidate>) -> Vec<Installation> {
    let mut out = Vec::new();

    for c in candidates {
        let world_roots: Vec<WorldRoot> = c
            .world_root_parents
            .iter()
            .map(|p| p.join("minecraftWorlds"))
            .filter(|p| p.is_dir())
            .map(|path| {
                let account = c.per_account.then(|| account_label(&path)).flatten();
                WorldRoot { account, path }
            })
            .collect();

        // An installation is real if either half of it exists: dev packs without
        // worlds is a valid deployment target, and worlds without dev packs is
        // exactly the legacy UWP case.
        if c.dev_pack_root.is_dir() || !world_roots.is_empty() {
            out.push(Installation {
                name: c.name,
                dev_pack_root: c.dev_pack_root,
                world_roots,
            });
        }
    }

    out
}

/// The account directory name, e.g. `Shared` or `2533274801234567`, taken from
/// `<account>/games/com.mojang/minecraftWorlds`.
fn account_label(world_root: &Path) -> Option<String> {
    world_root
        .parent()?          // games/com.mojang
        .parent()?          // games
        .parent()?          // <account>
        .file_name()?
        .to_str()
        .map(str::to_string)
}
```

Create `crates/construct-core/src/discovery/mod.rs`:

```rust
pub mod platform;

pub use platform::{Candidate, Installation, WorldRoot};
```

Add to `crates/construct-core/src/lib.rs`:

```rust
pub mod discovery;
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test -p construct-core platform
```

Expected: 6 passed.

- [ ] **Step 5: Check it finds the real installation on this machine**

```bash
cargo test -p construct-core platform -- --nocapture
```

Then confirm by hand that the mcpelauncher path in the table matches reality:

```bash
ls ~/Library/Application\ Support/mcpelauncher/games/com.mojang/minecraftWorlds/
```

Expected: four worlds, including `Amelix CMP` and `Ssu8ww1SFbM=`.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "Add per-platform root discovery

Base directories are parameters rather than environment reads, so the
whole table is testable against synthetic trees in a temp dir.

GDK splits worlds (per Xbox account) from dev packs (Users\\Shared), so an
installation carries several world roots; single-root installations omit
the account segment entirely, which is why macOS and Linux never show one."
```

---

## Task 5: Enumerating worlds

Turns installations into a flat list of worlds carrying everything `construct worlds` prints.

**Files:**
- Create: `crates/construct-core/src/discovery/worlds.rs`
- Modify: `crates/construct-core/src/discovery/mod.rs`
- Test: inline `#[cfg(test)]` module in `worlds.rs`

**Interfaces:**
- Consumes: `Installation`, `WorldRoot` (Task 4); `leveldat::read` (Task 3).
- Produces:
  - `pub enum LastPlayedSource { LevelDat, DirMtime }`
  - `pub struct World { pub installation: String, pub account: Option<String>, pub folder: String, pub display_name: String, pub path: PathBuf, pub last_played: Option<i64>, pub last_played_source: LastPlayedSource, pub size_bytes: u64 }`
  - `impl World { pub fn qualified(&self) -> String; pub fn db_path(&self) -> PathBuf; }`
  - `pub fn enumerate(installations: &[Installation]) -> Vec<World>`

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::platform::{Installation, WorldRoot};
    use std::fs;

    fn world_at(root: &Path, folder: &str, name: &str) -> PathBuf {
        let dir = root.join(folder);
        fs::create_dir_all(dir.join("db")).unwrap();
        fs::write(dir.join("db/000001.ldb"), vec![0u8; 1024]).unwrap();
        fs::write(dir.join("levelname.txt"), name).unwrap();
        dir
    }

    fn level_dat_with(dir: &Path, last_played: i64) {
        let root = nbtx::Value::Compound(
            [("LastPlayed".to_string(), nbtx::Value::Long(last_played))].into_iter().collect(),
        );
        let payload = nbtx::to_le_bytes(&root).unwrap();
        let mut bytes = 10i32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&(payload.len() as i32).to_le_bytes());
        bytes.extend_from_slice(&payload);
        fs::write(dir.join("level.dat"), bytes).unwrap();
    }

    fn single_root(tmp: &Path) -> Vec<Installation> {
        vec![Installation {
            name: "mcpelauncher".to_string(),
            dev_pack_root: tmp.to_path_buf(),
            world_roots: vec![WorldRoot { account: None, path: tmp.join("minecraftWorlds") }],
        }]
    }

    #[test]
    fn reads_display_name_from_levelname_txt() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("minecraftWorlds");
        let dir = world_at(&root, "Ssu8ww1SFbM=", "construct show");
        level_dat_with(&dir, 1741501762);

        let worlds = enumerate(&single_root(tmp.path()));
        assert_eq!(worlds.len(), 1);
        assert_eq!(worlds[0].display_name, "construct show");
        assert_eq!(worlds[0].folder, "Ssu8ww1SFbM=");
    }

    #[test]
    fn prefers_level_dat_last_played_and_says_so() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = world_at(&tmp.path().join("minecraftWorlds"), "A=", "W");
        level_dat_with(&dir, 1741501762);

        let worlds = enumerate(&single_root(tmp.path()));
        assert_eq!(worlds[0].last_played, Some(1741501762));
        assert_eq!(worlds[0].last_played_source, LastPlayedSource::LevelDat);
    }

    #[test]
    fn falls_back_to_dir_mtime_when_level_dat_is_unreadable() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = world_at(&tmp.path().join("minecraftWorlds"), "A=", "W");
        fs::write(dir.join("level.dat"), b"garbage").unwrap();

        let worlds = enumerate(&single_root(tmp.path()));
        assert_eq!(worlds[0].last_played_source, LastPlayedSource::DirMtime);
        assert!(worlds[0].last_played.is_some());
    }

    #[test]
    fn display_name_falls_back_to_the_folder_name() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("minecraftWorlds/NoName=");
        fs::create_dir_all(dir.join("db")).unwrap();
        fs::write(dir.join("level.dat"), b"x").unwrap();

        let worlds = enumerate(&single_root(tmp.path()));
        assert_eq!(worlds[0].display_name, "NoName=");
    }

    #[test]
    fn a_directory_without_level_dat_is_not_a_world() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("minecraftWorlds/not_a_world")).unwrap();
        assert!(enumerate(&single_root(tmp.path())).is_empty());
    }

    #[test]
    fn qualified_omits_the_account_when_there_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = world_at(&tmp.path().join("minecraftWorlds"), "A=", "W");
        level_dat_with(&dir, 1);
        assert_eq!(enumerate(&single_root(tmp.path()))[0].qualified(), "mcpelauncher/A=");
    }

    #[test]
    fn qualified_includes_the_account_when_present() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("minecraftWorlds");
        let dir = world_at(&root, "A=", "W");
        level_dat_with(&dir, 1);
        let installs = vec![Installation {
            name: "release".to_string(),
            dev_pack_root: tmp.path().to_path_buf(),
            world_roots: vec![WorldRoot { account: Some("Shared".into()), path: root }],
        }];
        assert_eq!(enumerate(&installs)[0].qualified(), "release/Shared/A=");
    }

    #[test]
    fn size_counts_the_db_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = world_at(&tmp.path().join("minecraftWorlds"), "A=", "W");
        level_dat_with(&dir, 1);
        assert!(enumerate(&single_root(tmp.path()))[0].size_bytes >= 1024);
    }

    #[test]
    fn a_folder_name_containing_a_space_is_handled() {
        // "Amelix CMP" is a real world folder on the target machine.
        let tmp = tempfile::tempdir().unwrap();
        let dir = world_at(&tmp.path().join("minecraftWorlds"), "Amelix CMP", "Amelix CMP");
        level_dat_with(&dir, 1);
        assert_eq!(enumerate(&single_root(tmp.path()))[0].folder, "Amelix CMP");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p construct-core worlds
```

Expected: FAIL to compile — `enumerate`, `World`, `LastPlayedSource` are not defined.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/construct-core/src/discovery/worlds.rs`:

```rust
//! Turning installations into the flat list of worlds the CLI shows.

use crate::discovery::platform::Installation;
use crate::leveldat;
use std::path::{Path, PathBuf};

/// Where a world's last-played time came from. The two sources disagree often
/// enough that `--json` reports which was used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastPlayedSource {
    LevelDat,
    DirMtime,
}

/// One discovered world.
#[derive(Debug, Clone)]
pub struct World {
    pub installation: String,
    pub account: Option<String>,
    /// The directory name, e.g. `Ssu8ww1SFbM=`.
    pub folder: String,
    /// From `levelname.txt`, falling back to the folder name.
    pub display_name: String,
    pub path: PathBuf,
    /// Unix seconds.
    pub last_played: Option<i64>,
    pub last_played_source: LastPlayedSource,
    /// Size of `db/`, which is what dominates a world and what a snapshot copies.
    pub size_bytes: u64,
}

impl World {
    /// `<installation>/<account>/<folder>`, with the account segment omitted
    /// when the installation has only one world root.
    pub fn qualified(&self) -> String {
        match &self.account {
            Some(a) => format!("{}/{}/{}", self.installation, a, self.folder),
            None => format!("{}/{}", self.installation, self.folder),
        }
    }

    pub fn db_path(&self) -> PathBuf {
        self.path.join("db")
    }
}

/// Every world under every installation. Unreadable entries are skipped rather
/// than failing the whole enumeration.
pub fn enumerate(installations: &[Installation]) -> Vec<World> {
    let mut out = Vec::new();

    for installation in installations {
        for root in &installation.world_roots {
            let Ok(entries) = std::fs::read_dir(&root.path) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                // A world is a directory containing level.dat. Nothing else counts.
                if !path.join("level.dat").is_file() {
                    continue;
                }
                let folder = entry.file_name().to_string_lossy().into_owned();
                let display_name = std::fs::read_to_string(path.join("levelname.txt"))
                    .map(|s| s.trim().to_string())
                    .ok()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| folder.clone());

                let (last_played, last_played_source) = last_played(&path);

                out.push(World {
                    installation: installation.name.clone(),
                    account: root.account.clone(),
                    folder,
                    display_name,
                    size_bytes: dir_size(&path.join("db")),
                    path,
                    last_played,
                    last_played_source,
                });
            }
        }
    }

    // Newest first — the world you want is almost always the one you just played.
    out.sort_by(|a, b| b.last_played.cmp(&a.last_played).then(a.folder.cmp(&b.folder)));
    out
}

fn last_played(world: &Path) -> (Option<i64>, LastPlayedSource) {
    if let Ok(dat) = leveldat::read(&world.join("level.dat"))
        && let Some(v) = dat.last_played()
    {
        return (Some(v), LastPlayedSource::LevelDat);
    }
    let mtime = std::fs::metadata(world)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);
    (mtime, LastPlayedSource::DirMtime)
}

fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            _ => e.metadata().map(|m| m.len()).unwrap_or(0),
        })
        .sum()
}
```

Update `crates/construct-core/src/discovery/mod.rs`:

```rust
pub mod platform;
pub mod worlds;

pub use platform::{Candidate, Installation, WorldRoot};
pub use worlds::{LastPlayedSource, World, enumerate};
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test -p construct-core worlds
```

Expected: 9 passed.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "Enumerate worlds from discovered installations

LastPlayed comes from level.dat, falling back to directory mtime, and the
World records which source was used — the two disagree often enough that
--json needs to report it.

Sorted newest first. Unreadable entries are skipped rather than failing
the whole enumeration."
```

---

## Task 6: World references

The grammar for naming a world on the command line: a display name, a folder name, a qualified `<installation>/<account>/<world>`, or a filesystem path. This is where the never-guess rule lives.

**Files:**
- Create: `crates/construct-core/src/discovery/reference.rs`
- Modify: `crates/construct-core/src/discovery/mod.rs`
- Test: inline `#[cfg(test)]` module in `reference.rs`

**Interfaces:**
- Consumes: `World` (Task 5), `CoreError` (Task 2), `leveldat::read` (Task 3).
- Produces:
  - `pub struct WorldRef { pub installation: Option<String>, pub account: Option<String>, pub world: String }`
  - `pub fn parse(input: &str) -> WorldRef`
  - `pub fn resolve(input: &str, worlds: &[World]) -> Result<World>`

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::worlds::LastPlayedSource;
    use std::path::PathBuf;

    fn w(installation: &str, account: Option<&str>, folder: &str, name: &str) -> World {
        World {
            installation: installation.to_string(),
            account: account.map(str::to_string),
            folder: folder.to_string(),
            display_name: name.to_string(),
            path: PathBuf::from("/tmp").join(folder),
            last_played: Some(0),
            last_played_source: LastPlayedSource::LevelDat,
            size_bytes: 0,
        }
    }

    fn fixture() -> Vec<World> {
        vec![
            w("release", Some("Shared"), "Ssu8ww1SFbM=", "Test World"),
            w("release", Some("2533274801234567"), "aBc0=", "Test World"),
            w("preview", Some("Shared"), "XyZ9=", "Test World"),
            w("mcpelauncher", None, "Amelix CMP", "Amelix CMP"),
        ]
    }

    #[test]
    fn parses_a_bare_name() {
        let r = parse("Amelix CMP");
        assert_eq!(r.installation, None);
        assert_eq!(r.account, None);
        assert_eq!(r.world, "Amelix CMP");
    }

    #[test]
    fn parses_installation_and_world() {
        let r = parse("release/Ssu8ww1SFbM=");
        assert_eq!(r.installation.as_deref(), Some("release"));
        assert_eq!(r.account, None);
        assert_eq!(r.world, "Ssu8ww1SFbM=");
    }

    #[test]
    fn parses_the_full_three_segment_form() {
        let r = parse("release/Shared/Ssu8ww1SFbM=");
        assert_eq!(r.installation.as_deref(), Some("release"));
        assert_eq!(r.account.as_deref(), Some("Shared"));
        assert_eq!(r.world, "Ssu8ww1SFbM=");
    }

    #[test]
    fn resolves_an_unambiguous_display_name() {
        assert_eq!(resolve("Amelix CMP", &fixture()).unwrap().folder, "Amelix CMP");
    }

    #[test]
    fn a_folder_name_beats_a_display_name() {
        let worlds = vec![
            w("mcpelauncher", None, "target", "decoy"),
            w("mcpelauncher", None, "other", "target"),
        ];
        assert_eq!(resolve("target", &worlds).unwrap().folder, "target");
    }

    #[test]
    fn an_ambiguous_name_is_an_error_listing_qualified_forms() {
        let err = resolve("Test World", &fixture()).unwrap_err();
        let CoreError::AmbiguousWorld { candidates, .. } = err else {
            panic!("expected AmbiguousWorld, got {err:?}");
        };
        assert_eq!(candidates.len(), 3);
        assert!(candidates.contains(&"release/Shared/Ssu8ww1SFbM=".to_string()));
        assert!(candidates.contains(&"preview/Shared/XyZ9=".to_string()));
    }

    #[test]
    fn qualifying_disambiguates() {
        assert_eq!(
            resolve("release/Shared/Ssu8ww1SFbM=", &fixture()).unwrap().folder,
            "Ssu8ww1SFbM="
        );
    }

    #[test]
    fn the_installation_segment_alone_can_disambiguate() {
        assert_eq!(resolve("preview/Test World", &fixture()).unwrap().folder, "XyZ9=");
    }

    #[test]
    fn a_missing_world_suggests_near_matches() {
        let err = resolve("Amelix", &fixture()).unwrap_err();
        let CoreError::WorldNotFound { near, .. } = err else {
            panic!("expected WorldNotFound, got {err:?}");
        };
        assert!(near.iter().any(|n| n.contains("Amelix CMP")));
    }

    #[test]
    fn a_filesystem_path_wins_over_a_name() {
        // Filesystem check first, so resolution is deterministic.
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("Amelix CMP");
        std::fs::create_dir_all(dir.join("db")).unwrap();
        std::fs::write(dir.join("level.dat"), b"x").unwrap();

        let got = resolve(dir.to_str().unwrap(), &fixture()).unwrap();
        assert_eq!(got.path, dir);
        assert_eq!(got.installation, "path");
    }

    #[test]
    fn a_path_without_level_dat_is_not_a_path_reference() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("empty");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(matches!(
            resolve(dir.to_str().unwrap(), &fixture()),
            Err(CoreError::WorldNotFound { .. })
        ));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p construct-core reference
```

Expected: FAIL to compile — `parse`, `resolve`, `WorldRef` are not defined.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/construct-core/src/discovery/reference.rs`:

```rust
//! The grammar for naming a world on the command line.
//!
//! A reference is a display name, a folder name, a qualified
//! `<installation>/<account>/<world>` with each segment optional from the left,
//! or a filesystem path. The filesystem is checked first so resolution is
//! deterministic. Ambiguity is always an error, never a silent pick.

use crate::discovery::worlds::{LastPlayedSource, World};
use crate::error::{CoreError, Result};
use crate::leveldat;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldRef {
    pub installation: Option<String>,
    pub account: Option<String>,
    pub world: String,
}

/// Splits a reference into its segments. Never fails — an unparseable reference
/// is simply a world name that will not match.
pub fn parse(input: &str) -> WorldRef {
    let parts: Vec<&str> = input.split('/').collect();
    match parts.as_slice() {
        [world] => WorldRef { installation: None, account: None, world: (*world).to_string() },
        [installation, world] => WorldRef {
            installation: Some((*installation).to_string()),
            account: None,
            world: (*world).to_string(),
        },
        [installation, account, world, ..] => WorldRef {
            installation: Some((*installation).to_string()),
            account: Some((*account).to_string()),
            world: (*world).to_string(),
        },
        [] => WorldRef { installation: None, account: None, world: String::new() },
    }
}

/// Resolves a reference to exactly one world.
pub fn resolve(input: &str, worlds: &[World]) -> Result<World> {
    // Filesystem first. This is why there is no --path flag: a single global
    // flag could only ever describe one world, and `copy` takes two.
    if let Some(world) = as_path(input) {
        return Ok(world);
    }

    let r = parse(input);
    let matches_segments = |w: &World| {
        r.installation.as_ref().is_none_or(|i| &w.installation == i)
            && r.account.as_ref().is_none_or(|a| w.account.as_ref() == Some(a))
    };

    // Folder names are matched before display names.
    let by_folder: Vec<&World> =
        worlds.iter().filter(|w| w.folder == r.world && matches_segments(w)).collect();
    let candidates = if by_folder.is_empty() {
        worlds
            .iter()
            .filter(|w| w.display_name == r.world && matches_segments(w))
            .collect::<Vec<_>>()
    } else {
        by_folder
    };

    match candidates.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(CoreError::WorldNotFound {
            reference: input.to_string(),
            near: near_matches(&r.world, worlds),
        }),
        many => Err(CoreError::AmbiguousWorld {
            reference: input.to_string(),
            candidates: many.iter().map(|w| w.qualified()).collect(),
        }),
    }
}

/// A directory containing `level.dat` is a world, wherever it sits.
fn as_path(input: &str) -> Option<World> {
    let path = Path::new(input);
    if !path.join("level.dat").is_file() {
        return None;
    }
    let folder = path.file_name()?.to_string_lossy().into_owned();
    let display_name = std::fs::read_to_string(path.join("levelname.txt"))
        .map(|s| s.trim().to_string())
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| folder.clone());
    let (last_played, last_played_source) = match leveldat::read(&path.join("level.dat")) {
        Ok(d) => match d.last_played() {
            Some(v) => (Some(v), LastPlayedSource::LevelDat),
            None => (None, LastPlayedSource::DirMtime),
        },
        Err(_) => (None, LastPlayedSource::DirMtime),
    };

    Some(World {
        // A path-referenced world belongs to no installation. The label keeps
        // `qualified()` total rather than introducing an Option.
        installation: "path".to_string(),
        account: None,
        folder,
        display_name,
        path: path.to_path_buf(),
        last_played,
        last_played_source,
        size_bytes: 0,
    })
}

/// Case-insensitive substring matches, for "did you mean".
fn near_matches(needle: &str, worlds: &[World]) -> Vec<String> {
    let needle = needle.to_lowercase();
    let mut out: Vec<String> = worlds
        .iter()
        .filter(|w| {
            w.folder.to_lowercase().contains(&needle)
                || w.display_name.to_lowercase().contains(&needle)
        })
        .map(|w| format!("{} ({})", w.display_name, w.qualified()))
        .collect();
    out.truncate(5);
    out
}
```

Update `crates/construct-core/src/discovery/mod.rs`:

```rust
pub mod platform;
pub mod reference;
pub mod worlds;

pub use platform::{Candidate, Installation, WorldRoot};
pub use worlds::{LastPlayedSource, World, enumerate};
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test -p construct-core reference
```

Expected: 11 passed.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "Resolve world references, never guessing between candidates

A reference is a display name, a folder name, a qualified
installation/account/world, or a filesystem path. The filesystem is
checked first so resolution is deterministic, which is why there is no
--path flag — one global flag could only describe one world, and copy
takes two.

Folder names beat display names; ambiguity is an error carrying the
qualified forms rather than a silent pick."
```

---

## Task 7: Configuration

**Files:**
- Create: `crates/construct-core/src/config.rs`
- Modify: `crates/construct-core/src/lib.rs`
- Test: inline `#[cfg(test)]` module in `config.rs`

**Interfaces:**
- Consumes: `CoreError` (Task 2).
- Produces:
  - `pub struct Config { pub default_installation: Option<String>, pub roots: Vec<ExtraRoot>, pub backups: Backups }`
  - `pub struct ExtraRoot { pub name: String, pub path: PathBuf }`
  - `pub struct Backups { pub dir: Option<PathBuf>, pub keep: usize }` — `keep` defaults to `10`
  - `pub struct Loaded { pub config: Config, pub warnings: Vec<String>, pub source: Option<PathBuf> }`
  - `pub fn load(explicit: Option<&Path>, env: &dyn Fn(&str) -> Option<String>) -> Result<Loaded>`
  - `pub fn parse(text: &str, path: &Path) -> Result<(Config, Vec<String>)>`
  - `pub fn default_path() -> Option<PathBuf>`

Precedence is CLI flag → environment variable → config file → auto-discovery. The CLI layer applies the flag tier; this module covers the rest. Taking `env` as a closure keeps the tests from mutating process state.

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(_: &str) -> Option<String> { None }

    #[test]
    fn an_absent_file_yields_defaults_with_no_error() {
        // There is no init step; absent config must simply mean all defaults.
        let loaded = load(Some(Path::new("/nonexistent/config.toml")), &no_env).unwrap();
        assert_eq!(loaded.config.default_installation, None);
        assert!(loaded.config.roots.is_empty());
        assert_eq!(loaded.config.backups.keep, 10);
        assert!(loaded.warnings.is_empty());
    }

    #[test]
    fn keep_defaults_to_ten_when_the_file_omits_it() {
        let (c, _) = parse("[backups]\ndir = \"/tmp/b\"\n", Path::new("c.toml")).unwrap();
        assert_eq!(c.backups.keep, 10);
        assert_eq!(c.backups.dir, Some(PathBuf::from("/tmp/b")));
    }

    #[test]
    fn parses_a_full_config() {
        let text = r#"
default_installation = "release"

[[roots]]
name = "backup"
path = "D:/MinecraftBackups/com.mojang"

[backups]
dir = "/Volumes/Spare/construct-backups"
keep = 3
"#;
        let (c, warnings) = parse(text, Path::new("c.toml")).unwrap();
        assert_eq!(c.default_installation.as_deref(), Some("release"));
        assert_eq!(c.roots.len(), 1);
        assert_eq!(c.roots[0].name, "backup");
        assert_eq!(c.backups.keep, 3);
        assert!(warnings.is_empty());
    }

    #[test]
    fn unknown_keys_warn_rather_than_fail() {
        let (c, warnings) = parse("nonsense = 1\ndefault_installation = \"release\"\n", Path::new("c.toml")).unwrap();
        assert_eq!(c.default_installation.as_deref(), Some("release"));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("nonsense"), "warning should name the key: {}", warnings[0]);
    }

    #[test]
    fn malformed_toml_is_an_error() {
        assert!(matches!(
            parse("this is not toml [[[", Path::new("c.toml")),
            Err(CoreError::BadConfig { .. })
        ));
    }

    #[test]
    fn an_extra_root_must_be_named() {
        // An unnamed root cannot appear in the installation/account/world grammar.
        assert!(matches!(
            parse("[[roots]]\npath = \"/tmp/x\"\n", Path::new("c.toml")),
            Err(CoreError::BadConfig { .. })
        ));
    }

    #[test]
    fn the_env_var_overrides_the_config_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "default_installation = \"release\"\n").unwrap();

        let env = |k: &str| (k == "CONSTRUCT_INSTALLATION").then(|| "preview".to_string());
        let loaded = load(Some(&path), &env).unwrap();
        assert_eq!(loaded.config.default_installation.as_deref(), Some("preview"));
    }

    #[test]
    fn construct_com_mojang_env_var_becomes_an_extra_root() {
        let env = |k: &str| (k == "CONSTRUCT_COM_MOJANG").then(|| "/tmp/env-root".to_string());
        let loaded = load(Some(Path::new("/nonexistent")), &env).unwrap();
        assert_eq!(loaded.config.roots.len(), 1);
        assert_eq!(loaded.config.roots[0].path, PathBuf::from("/tmp/env-root"));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p construct-core config
```

Expected: FAIL to compile — `load`, `parse`, `Config` are not defined.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/construct-core/src/config.rs`:

```rust
//! Configuration loading.
//!
//! Precedence is CLI flag → environment variable → config file →
//! auto-discovery. An absent file means all defaults, so there is no init step.
//! Unknown keys warn rather than fail.

use crate::error::{CoreError, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub default_installation: Option<String>,
    pub roots: Vec<ExtraRoot>,
    pub backups: Backups,
}

/// An extra `com.mojang` root to probe. Named, because an unnamed root cannot
/// appear in the `<installation>/<account>/<world>` grammar.
#[derive(Debug, Clone)]
pub struct ExtraRoot {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Backups {
    pub dir: Option<PathBuf>,
    pub keep: usize,
}

impl Default for Backups {
    fn default() -> Self {
        Self { dir: None, keep: DEFAULT_KEEP }
    }
}

/// Retention default: the last ten snapshots per world.
pub const DEFAULT_KEEP: usize = 10;

#[derive(Debug)]
pub struct Loaded {
    pub config: Config,
    pub warnings: Vec<String>,
    pub source: Option<PathBuf>,
}

// The wire form. Separate from `Config` so unknown keys can be collected as
// warnings instead of aborting the load.
#[derive(Deserialize)]
struct WireConfig {
    default_installation: Option<String>,
    #[serde(default)]
    roots: Vec<WireRoot>,
    backups: Option<WireBackups>,
    #[serde(flatten)]
    unknown: toml::Table,
}

#[derive(Deserialize)]
struct WireRoot {
    name: Option<String>,
    path: PathBuf,
}

#[derive(Deserialize)]
struct WireBackups {
    dir: Option<PathBuf>,
    keep: Option<usize>,
}

/// `~/.config/constructcli/config.toml` and platform equivalents.
pub fn default_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("", "", "constructcli")
        .map(|d| d.config_dir().join("config.toml"))
}

pub fn parse(text: &str, path: &Path) -> Result<(Config, Vec<String>)> {
    let bad = |reason: String| CoreError::BadConfig { path: path.to_path_buf(), reason };

    let wire: WireConfig = toml::from_str(text).map_err(|e| bad(e.to_string()))?;

    let mut warnings = Vec::new();
    for key in wire.unknown.keys() {
        warnings.push(format!("unknown config key `{key}` in {}", path.display()));
    }

    let mut roots = Vec::new();
    for r in wire.roots {
        let name = r.name.ok_or_else(|| {
            bad("every [[roots]] entry needs a `name`; an unnamed root cannot be addressed in a qualified reference".to_string())
        })?;
        roots.push(ExtraRoot { name, path: r.path });
    }

    let backups = wire.backups.map_or_else(Backups::default, |b| Backups {
        dir: b.dir,
        keep: b.keep.unwrap_or(DEFAULT_KEEP),
    });

    Ok((Config { default_installation: wire.default_installation, roots, backups }, warnings))
}

/// Loads config, then applies environment overrides on top.
pub fn load(explicit: Option<&Path>, env: &dyn Fn(&str) -> Option<String>) -> Result<Loaded> {
    let path = explicit
        .map(Path::to_path_buf)
        .or_else(|| env("CONSTRUCT_CONFIG").map(PathBuf::from))
        .or_else(default_path);

    let (mut config, mut warnings, source) = match &path {
        Some(p) if p.is_file() => {
            let text = std::fs::read_to_string(p)?;
            let (c, w) = parse(&text, p)?;
            (c, w, Some(p.clone()))
        }
        // An absent file is not an error.
        _ => (Config::default(), Vec::new(), None),
    };

    if let Some(v) = env("CONSTRUCT_INSTALLATION") {
        config.default_installation = Some(v);
    }
    if let Some(v) = env("CONSTRUCT_COM_MOJANG") {
        config.roots.push(ExtraRoot { name: "env".to_string(), path: PathBuf::from(v) });
    }

    warnings.shrink_to_fit();
    Ok(Loaded { config, warnings, source })
}
```

Add to `crates/construct-core/src/lib.rs`:

```rust
pub mod config;
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test -p construct-core config
```

Expected: 8 passed.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "Load configuration with env-over-file precedence

An absent file means all defaults, so there is no init step. Unknown keys
warn rather than fail. backups.keep defaults to 10.

Extra roots must be named — an unnamed root cannot be addressed in the
installation/account/world grammar, so a missing name is an error rather
than a generated placeholder."
```

---

## Task 8: Structure keys

The grammar of `structuretemplate_` keys. Pure functions over bytes and strings — no I/O, no database.

**Files:**
- Create: `crates/construct-core/src/store/key.rs`
- Create: `crates/construct-core/src/store/mod.rs` (module declarations only for now)
- Modify: `crates/construct-core/src/lib.rs`
- Test: inline `#[cfg(test)]` module in `key.rs`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub const PREFIX: &[u8]` = `b"structuretemplate_"`
  - `pub const DEFAULT_NAMESPACE: &str` = `"mystructure"`
  - `pub fn qualify(name: &str) -> String` — `"house"` → `"mystructure:house"`; a name already carrying a namespace is returned unchanged
  - `pub fn encode(name: &str) -> Vec<u8>`
  - `pub fn decode(key: &[u8]) -> Option<String>` — returns the qualified id, or `None` when the key is not a structure key
  - `pub fn display_name(id: &str) -> &str` — strips a `mystructure:` prefix for display, leaves other namespaces intact

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualifies_a_bare_name_with_the_default_namespace() {
        assert_eq!(qualify("house"), "mystructure:house");
    }

    #[test]
    fn leaves_an_explicit_namespace_alone() {
        assert_eq!(qualify("understudy:players"), "understudy:players");
    }

    #[test]
    fn encodes_the_key_measured_from_a_real_world() {
        // Ssu8ww1SFbM= ("construct show") really contains this key.
        assert_eq!(encode("copy"), b"structuretemplate_mystructure:copy".to_vec());
    }

    #[test]
    fn round_trips() {
        for name in ["house", "mystructure:house", "understudy:players", "a.b-c_1"] {
            assert_eq!(decode(&encode(name)).unwrap(), qualify(name));
        }
    }

    #[test]
    fn decodes_a_real_key() {
        assert_eq!(
            decode(b"structuretemplate_mystructure:copy").unwrap(),
            "mystructure:copy"
        );
    }

    #[test]
    fn rejects_keys_without_the_prefix() {
        assert_eq!(decode(b"AutonomousEntities"), None);
        assert_eq!(decode(b""), None);
        assert_eq!(decode(&[0x00, 0x01, 0x02]), None);
    }

    #[test]
    fn rejects_a_prefixed_key_whose_body_is_not_utf8() {
        let mut k = PREFIX.to_vec();
        k.extend_from_slice(&[0xff, 0xfe]);
        assert_eq!(decode(&k), None);
    }

    #[test]
    fn display_name_strips_only_the_default_namespace() {
        assert_eq!(display_name("mystructure:house"), "house");
        assert_eq!(display_name("understudy:players"), "understudy:players");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p construct-core key
```

Expected: FAIL to compile — `qualify`, `encode`, `decode`, `display_name` are not defined.

- [ ] **Step 3: Write the implementation**

Prepend to `crates/construct-core/src/store/key.rs`:

```rust
//! The `structuretemplate_` key grammar.
//!
//! A structure saved in-game is stored under `structuretemplate_` followed by a
//! namespaced id, e.g. `structuretemplate_mystructure:copy`. A bare name means
//! the `mystructure` namespace, which is what the structure block and
//! `/structure` use by default.

pub const PREFIX: &[u8] = b"structuretemplate_";
pub const DEFAULT_NAMESPACE: &str = "mystructure";

/// Adds the default namespace to a bare name, leaving qualified names alone.
pub fn qualify(name: &str) -> String {
    if name.contains(':') {
        name.to_string()
    } else {
        format!("{DEFAULT_NAMESPACE}:{name}")
    }
}

/// The leveldb key for a structure name, bare or qualified.
pub fn encode(name: &str) -> Vec<u8> {
    let mut out = PREFIX.to_vec();
    out.extend_from_slice(qualify(name).as_bytes());
    out
}

/// The qualified id in a leveldb key, or `None` if it is not a structure key.
pub fn decode(key: &[u8]) -> Option<String> {
    let body = key.strip_prefix(PREFIX)?;
    // Bedrock writes plenty of binary keys; a non-UTF-8 body is not ours.
    std::str::from_utf8(body).ok().filter(|s| !s.is_empty()).map(str::to_string)
}

/// How an id is shown to a user: the default namespace is noise, any other
/// namespace is information.
pub fn display_name(id: &str) -> &str {
    id.strip_prefix(DEFAULT_NAMESPACE).and_then(|r| r.strip_prefix(':')).unwrap_or(id)
}
```

Create `crates/construct-core/src/store/mod.rs`:

```rust
pub mod key;
```

Add to `crates/construct-core/src/lib.rs`:

```rust
pub mod store;
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test -p construct-core key
```

Expected: 8 passed.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "Add structuretemplate_ key encoding and decoding

Pure functions, no I/O. The encoding test asserts the exact key found in
the construct show world: structuretemplate_mystructure:copy.

Non-UTF-8 bodies decode to None rather than erroring — Bedrock writes
plenty of binary keys and they are simply not ours."
```

---

## Task 9: The `StructureStore` trait and the `bedrock_level` backend

**A constraint discovered by reading the dependency:** `leveldb::Options::create_if_missing` defaults to `false` and the FFI never sets it, so `Database::open` **cannot create a database**. A fixture therefore cannot be synthesised in Rust — it must be a real database. This task commits one, derived from `bedrock-rs`'s own Apache-2.0 test world (591 KB), with structure keys inserted.

**The backend dependency is added in this task, not Task 2.** It comes from patched local
checkouts, not from a registry or a fork URL — the upstream crates do not compile on macOS
or on any non-x86_64 target (spec §3). `scripts/setup-deps.sh` and `third_party/patches/`
already exist and are verified working; the checkouts are git-ignored and recreated by that
script.

Add to the workspace `[workspace.dependencies]`:

```toml
bedrock_level = { path = "third_party/checkouts/bedrock-rs/crates/level" }
```

and to the **workspace root** `Cargo.toml`, a patch redirecting `bedrock_level`'s own
upstream dependency on `leveldb-sys` to the patched checkout:

```toml
[patch."https://github.com/bedrock-crustaceans/leveldb-sys"]
leveldb-sys = { path = "third_party/checkouts/leveldb-sys" }
```

Then add `bedrock_level.workspace = true` to `construct-core`'s `[dependencies]`.

**Before building, run `./scripts/setup-deps.sh`.** Without it the checkouts are absent and
cargo fails to resolve the path dependency. Verified end-to-end from scratch: wiping
`third_party/checkouts/`, re-running the script, and building a probe compiles cleanly on
Apple Silicon and the FFI links and runs.

CI must run `./scripts/setup-deps.sh` before `cargo` — add that step to
`.github/workflows/ci.yml` in this task, before the fmt/clippy/test steps.

**Files:**
- Modify: `Cargo.toml` (workspace deps + `[patch]`), `crates/construct-core/Cargo.toml`
- Create: `crates/construct-core/src/store/bedrock.rs`
- Modify: `crates/construct-core/src/store/mod.rs`
- Create: `crates/construct-core/tests/fixtures/world.tar.gz` (generated in Step 1)
- Create: `crates/construct-core/tests/fixtures/NOTICE`
- Create: `crates/construct-core/tests/store.rs`
- Modify: `crates/construct-core/Cargo.toml` (add `tar`, `flate2` as dev-dependencies)

**Interfaces:**
- Consumes: `key::{decode, encode}` (Task 8), `CoreError` (Task 2).
- Produces:
  - `pub trait StructureStore { fn ids(&self) -> Result<Vec<String>>; fn get(&self, id: &str) -> Result<Option<Vec<u8>>>; }`
  - `pub struct BedrockStore` with `pub fn open(db_dir: &Path) -> Result<BedrockStore>`
  - `pub fn guard_test_path(path: &Path)` — panics if a database path is not under a temp directory
  - `pub struct MemoryStore(pub BTreeMap<String, Vec<u8>>)` implementing `StructureStore`, for tests in later tasks

- [ ] **Step 1: Generate the fixture, once**

Run this from the repo root. It borrows `bedrock-rs`'s test world, inserts two structure keys, and re-tars the result.

```bash
mkdir -p /tmp/fixgen && cd /tmp/fixgen
git clone --depth 1 https://github.com/bedrock-crustaceans/bedrock-rs.git
mkdir -p work && tar xzf bedrock-rs/crates/level/tests/level.tar.gz -C work
cargo new --bin fixgen && cd fixgen
cat >> Cargo.toml <<'TOML'
bedrock_level = { git = "https://github.com/bedrock-crustaceans/bedrock-rs", package = "bedrock_level", rev = "2d9e4087a207bdcbad6e4cdc83de46e94712b2f4" }
nbtx = "3.0.1"
TOML
```

`src/main.rs`:

```rust
use bedrock_level::db::Database;
use std::collections::HashMap;

/// A minimal but structurally valid .mcstructure value.
fn mcstructure(size: [i32; 3]) -> Vec<u8> {
    let v = |x: i32| nbtx::Value::Int(x);
    let root = nbtx::Value::Compound(HashMap::from([
        ("format_version".to_string(), nbtx::Value::Int(1)),
        ("size".to_string(), nbtx::Value::List(size.iter().copied().map(v).collect())),
        (
            "structure_world_origin".to_string(),
            nbtx::Value::List(vec![v(0), v(64), v(0)]),
        ),
        (
            "structure".to_string(),
            nbtx::Value::Compound(HashMap::from([(
                "block_indices".to_string(),
                nbtx::Value::List(vec![
                    nbtx::Value::List(vec![nbtx::Value::Int(0)]),
                    nbtx::Value::List(vec![nbtx::Value::Int(-1)]),
                ]),
            )])),
        ),
    ]));
    nbtx::to_le_bytes(&root).unwrap()
}

fn main() {
    let db = Database::open("/tmp/fixgen/work/test_level/db").expect("open");
    db.insert(b"structuretemplate_mystructure:house", mcstructure([4, 4, 4])).unwrap();
    db.insert(b"structuretemplate_mystructure:barn", mcstructure([2, 2, 2])).unwrap();
    // A namespaced structure, to prove the decoder does not assume mystructure.
    db.insert(b"structuretemplate_understudy:players", mcstructure([1, 1, 1])).unwrap();
    println!("inserted");
}
```

```bash
cargo run --release
cd /tmp/fixgen/work && tar czf world.tar.gz test_level
cp world.tar.gz <repo>/crates/construct-core/tests/fixtures/world.tar.gz
```

`crates/construct-core/tests/fixtures/NOTICE`:

```
world.tar.gz is derived from bedrock-rs (crates/level/tests/level.tar.gz),
Copyright the bedrock-crustaceans contributors, licensed under Apache-2.0:
https://github.com/bedrock-crustaceans/bedrock-rs/blob/main/LICENSE

Three structuretemplate_ keys were added for ConstructCLI's tests. The world
is otherwise unmodified.
```

Verify the result is small enough to commit:

```bash
ls -la crates/construct-core/tests/fixtures/world.tar.gz   # expect well under 1 MB
```

- [ ] **Step 2: Add dev-dependencies**

In the workspace `Cargo.toml` under `[workspace.dependencies]`:

```toml
tar = "0.4"
flate2 = "1.0"
```

In `crates/construct-core/Cargo.toml` under `[dev-dependencies]`:

```toml
tar.workspace = true
flate2.workspace = true
```

- [ ] **Step 3: Write the failing tests**

`crates/construct-core/tests/store.rs`:

```rust
use construct_core::store::bedrock::BedrockStore;
use construct_core::store::StructureStore;
use std::path::PathBuf;

/// Extracts the fixture world into a fresh temp dir. Every test gets its own
/// copy, so a test that writes cannot affect another.
fn extract() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let gz = std::fs::File::open("tests/fixtures/world.tar.gz").expect("fixture missing");
    tar::Archive::new(flate2::read::GzDecoder::new(gz))
        .unpack(tmp.path())
        .expect("unpack");
    let db = tmp.path().join("test_level/db");
    (tmp, db)
}

#[test]
fn lists_only_structure_keys() {
    let (_tmp, db) = extract();
    let store = BedrockStore::open(&db).unwrap();
    let mut ids = store.ids().unwrap();
    ids.sort();
    assert_eq!(
        ids,
        vec![
            "mystructure:barn".to_string(),
            "mystructure:house".to_string(),
            "understudy:players".to_string(),
        ],
        "the fixture world has many other keys; none of them are structures"
    );
}

#[test]
fn gets_a_structure_by_bare_name() {
    let (_tmp, db) = extract();
    let store = BedrockStore::open(&db).unwrap();
    let bytes = store.get("house").unwrap().expect("house is present");
    assert!(!bytes.is_empty());
    // A .mcstructure is little-endian NBT, which begins with TAG_Compound.
    assert_eq!(bytes[0], 0x0a);
}

#[test]
fn gets_a_structure_by_qualified_name() {
    let (_tmp, db) = extract();
    let store = BedrockStore::open(&db).unwrap();
    assert!(store.get("understudy:players").unwrap().is_some());
    // The bare form must NOT find a non-default namespace.
    assert!(store.get("players").unwrap().is_none());
}

#[test]
fn a_missing_structure_is_none_not_an_error() {
    let (_tmp, db) = extract();
    let store = BedrockStore::open(&db).unwrap();
    assert!(store.get("nope").unwrap().is_none());
}

#[test]
fn opening_a_nonexistent_database_is_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(BedrockStore::open(&tmp.path().join("no-db")).is_err());
}

#[test]
#[should_panic(expected = "outside a temp directory")]
fn the_guard_refuses_a_path_outside_temp() {
    construct_core::store::bedrock::guard_test_path(std::path::Path::new("/Users/someone/world/db"));
}
```

- [ ] **Step 4: Run the tests to verify they fail**

```bash
cargo test -p construct-core --test store
```

Expected: FAIL to compile — `BedrockStore` and `StructureStore` are not defined.

- [ ] **Step 5: Write the implementation**

`crates/construct-core/src/store/bedrock.rs`:

```rust
//! The `bedrock_level` backend: FFI to the leveldb fork Minecraft itself uses.

use crate::error::{CoreError, Result};
use crate::store::{StructureStore, key};
use bedrock_level::db::Database;
use std::path::Path;

pub struct BedrockStore {
    db: Database,
}

impl BedrockStore {
    /// Opens the database at a world's `db` directory.
    ///
    /// Note this acquires `db/LOCK`: leveldb's C++ API has no read-only open,
    /// so a world currently open in Minecraft cannot be opened here. Callers
    /// that can tolerate a stale read should go through
    /// [`crate::store::open_world_store`], which falls back to a snapshot.
    pub fn open(db_dir: &Path) -> Result<Self> {
        // `Database::open` takes `AsRef<str>`, not a path.
        let path = db_dir
            .to_str()
            .ok_or_else(|| CoreError::Db(format!("non-UTF-8 database path: {}", db_dir.display())))?;
        let db = Database::open(path).map_err(|e| CoreError::Db(e.to_string()))?;
        Ok(Self { db })
    }
}

impl StructureStore for BedrockStore {
    fn ids(&self) -> Result<Vec<String>> {
        let mut out = Vec::new();
        // `Iterator` is implemented for `&mut Keys`, not `Keys`, so the binding
        // must be mutable and iterated by reference.
        let mut keys = self.db.keys();
        for kv in &mut keys {
            if let Some(id) = key::decode(&kv.key()) {
                out.push(id);
            }
        }
        Ok(out)
    }

    fn get(&self, id: &str) -> Result<Option<Vec<u8>>> {
        let k = key::encode(id);
        let got = self.db.get(&k).map_err(|e| CoreError::Db(e.to_string()))?;
        Ok(got.map(|buf| buf.to_vec()))
    }
}

/// Refuses any database path that is not under a temp directory.
///
/// Tests open real leveldb databases and this crate can write to them. The cost
/// of a test pointed at a real world is a corrupted save, so the check is a
/// hard panic rather than a warning.
pub fn guard_test_path(path: &Path) {
    let tmp = std::env::temp_dir();
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let tmp = tmp.canonicalize().unwrap_or(tmp);
    assert!(
        canonical.starts_with(&tmp),
        "refusing to open a database outside a temp directory: {}",
        canonical.display()
    );
}
```

Update `crates/construct-core/src/store/mod.rs`:

```rust
pub mod bedrock;
pub mod key;

use crate::error::{CoreError, Result};
use std::collections::BTreeMap;

/// Read access to a world's structures.
///
/// A trait rather than a concrete type so the leveldb backend can be replaced —
/// `bedrock-leveldb` compiles to WASM and never takes the LOCK, which a future
/// web UI would want.
pub trait StructureStore {
    /// Every structure id in the store, qualified (`mystructure:house`).
    fn ids(&self) -> Result<Vec<String>>;

    /// The raw bytes of one structure, which are byte-identical to a
    /// `.mcstructure` file. `id` may be bare or qualified.
    fn get(&self, id: &str) -> Result<Option<Vec<u8>>>;
}

/// An in-memory store, for testing everything above this layer without a database.
#[derive(Debug, Default, Clone)]
pub struct MemoryStore(pub BTreeMap<String, Vec<u8>>);

impl MemoryStore {
    pub fn with(entries: &[(&str, &[u8])]) -> Self {
        Self(
            entries
                .iter()
                .map(|(k, v)| (key::qualify(k), v.to_vec()))
                .collect(),
        )
    }
}

impl StructureStore for MemoryStore {
    fn ids(&self) -> Result<Vec<String>> {
        Ok(self.0.keys().cloned().collect())
    }

    fn get(&self, id: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.0.get(&key::qualify(id)).cloned())
    }
}
```

- [ ] **Step 6: Run the tests to verify they pass**

```bash
cargo test -p construct-core --test store
```

Expected: 6 passed.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "Add the StructureStore trait and the bedrock_level backend

leveldb's create_if_missing defaults to false and the FFI never sets it,
so a database cannot be synthesised in Rust — the fixture is a real world
derived from bedrock-rs's Apache-2.0 test level with structure keys added.
Attribution in tests/fixtures/NOTICE.

Each test extracts its own copy, and guard_test_path panics on any
database path outside a temp dir: the cost of a test pointed at a real
world is a corrupted save.

MemoryStore lets every layer above this one be tested without a database."
```

---

## Task 10: Reads always work from a copy

**This task changed after the stage 0 spike.** The original design opened a world's database
directly and fell back to a snapshot only when the LOCK was held. That is unsafe: the spike
opened a real world without ever calling `insert`, and the world's `db/` came back rewritten —
`000014.log` and `MANIFEST-000012` replaced by `000018.ldb`, `000019.log`, `MANIFEST-000017`.
No data was lost, but `DB::Open` runs recovery, and `bedrock_level` exposes no read-only
option. Spec §8 was rewritten accordingly.

So: **a read never opens a world's database. It copies `db/` to a temp directory and opens
the copy — unconditionally.** The world being in use stops mattering for reads, because the
copy never touches the LOCK. The free-space check is on the main path, not a fallback.

Two consequences for the code below. `open_world_store` has no fallback branch and no error
matching — there is nothing to detect, because the direct path no longer exists. And
`via_snapshot` is now always `Some(bytes)` for a world-backed store, so it reports a cost
rather than an exceptional condition.

**A finding from Task 1 kept for later:** the lock failure surfaces as `CoreError::Db(String)`,
not a typed variant — `bedrock_level::Error::DatabaseLockError` is mutex poisoning, not the
LOCK file. The measured POSIX text is `IO error: lock <path>/LOCK: already held by process`;
Windows uses `env_win.cc` with a different message, and no external-process holder has been
tested. Stage 4's `delete` needs this; stage 1 does not.

**Files:**
- Create: `crates/construct-core/src/store/snapshot.rs`
- Modify: `crates/construct-core/src/store/mod.rs`
- Test: `crates/construct-core/tests/store.rs` (extend)

**Interfaces:**
- Consumes: `BedrockStore` (Task 9), `World` (Task 5).
- Produces:
  - `pub struct OpenedStore { pub via_snapshot: Option<u64>, .. }` implementing `StructureStore`
  - `pub fn open_world_store(world: &World) -> Result<OpenedStore>` — always snapshots
  - `pub fn copy_dir(src: &Path, dst: &Path) -> Result<u64>` — returns bytes copied
  - `pub fn dir_size(dir: &Path) -> u64`

- [ ] **Step 1: Write the failing tests**

Append to `crates/construct-core/tests/store.rs`:

```rust
use construct_core::store::snapshot;

#[test]
fn copy_dir_reports_bytes_copied() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(src.join("nested")).unwrap();
    std::fs::write(src.join("a"), vec![0u8; 100]).unwrap();
    std::fs::write(src.join("nested/b"), vec![0u8; 50]).unwrap();

    let dst = tmp.path().join("dst");
    assert_eq!(snapshot::copy_dir(&src, &dst).unwrap(), 150);
    assert!(dst.join("nested/b").is_file());
}

#[test]
fn a_read_always_snapshots_and_reports_the_size() {
    let (_tmp, db) = extract();
    let world = world_at(db.parent().unwrap());
    let opened = construct_core::store::open_world_store(&world).unwrap();
    assert!(opened.via_snapshot.is_some(), "every read goes through a copy");
    assert!(opened.ids().unwrap().contains(&"mystructure:house".to_string()));
}

#[test]
fn a_read_does_not_modify_the_world_on_disk() {
    // THE test for this task. Opening a leveldb database rewrites it, so the only
    // way a read can be safe is never to open the original. Hash the whole db
    // directory before and after; any difference means the guarantee is broken.
    let (_tmp, db) = extract();
    let world = world_at(db.parent().unwrap());

    let before = dir_fingerprint(&db);
    let opened = construct_core::store::open_world_store(&world).unwrap();
    let _ = opened.ids().unwrap();
    drop(opened);
    let after = dir_fingerprint(&db);

    assert_eq!(before, after, "a read modified the world's db/ directory");
}

/// Every file name and its exact bytes, so a rewritten MANIFEST or a rolled WAL shows up.
fn dir_fingerprint(dir: &std::path::Path) -> Vec<(String, u64, Vec<u8>)> {
    let mut out: Vec<(String, u64, Vec<u8>)> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| {
            let bytes = std::fs::read(e.path()).unwrap_or_default();
            (
                e.file_name().to_string_lossy().into_owned(),
                bytes.len() as u64,
                bytes,
            )
        })
        .collect();
    out.sort();
    out
}

/// Build a `World` pointing at an extracted fixture.
fn world_at(dir: &std::path::Path) -> construct_core::discovery::World {
    construct_core::discovery::World {
        installation: "test".into(),
        account: None,
        folder: "test_level".into(),
        display_name: "test_level".into(),
        path: dir.to_path_buf(),
        last_played: Some(0),
        last_played_source: construct_core::discovery::LastPlayedSource::LevelDat,
        size_bytes: 0,
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p construct-core --test store
```

Expected: FAIL to compile — `snapshot`, `open_world_store`, `OpenedStore` are not defined.

- [ ] **Step 3: Write the implementation**

`crates/construct-core/src/store/snapshot.rs`:

```rust
//! Reading a world that Minecraft currently has open.
//!
//! LevelDB's C++ API acquires `db/LOCK` on every open and offers no read-only
//! mode, so the only way to read a live world is to copy it. This is not gated
//! behind a flag — it announces itself and checks free space first, because a
//! large survival world's `db/` can reach several gigabytes.

use crate::discovery::World;
use crate::error::{CoreError, Result};
use crate::store::bedrock::BedrockStore;
use crate::store::{OpenedStore, StructureStore};
use std::path::Path;

/// Copies a directory tree, returning the number of bytes written.
pub fn copy_dir(src: &Path, dst: &Path) -> Result<u64> {
    std::fs::create_dir_all(dst)?;
    let mut total = 0;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            total += copy_dir(&entry.path(), &to)?;
        } else {
            total += std::fs::copy(entry.path(), &to)?;
        }
    }
    Ok(total)
}

/// Copies `db/` to a temp directory and opens the copy.
pub fn open_via_snapshot(world: &World) -> Result<OpenedStore> {
    let db = world.db_path();
    let need = dir_size(&db);

    if let Some(available) = available_space(&std::env::temp_dir())
        && available < need
    {
        return Err(CoreError::InsufficientSpace { world: world.path.clone(), need, available });
    }

    let tmp = tempfile::tempdir()?;
    let copy = tmp.path().join("db");
    let copied = copy_dir(&db, &copy)?;
    let store = BedrockStore::open(&copy)?;

    Ok(OpenedStore { inner: Box::new(store), _snapshot: Some(tmp), via_snapshot: Some(copied) })
}

pub fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            _ => e.metadata().map(|m| m.len()).unwrap_or(0),
        })
        .sum()
}

/// Free space on the filesystem holding `path`, when it can be determined.
///
/// Returns `None` rather than failing: an unknown free-space figure should not
/// stop a read that would have worked.
fn available_space(_path: &Path) -> Option<u64> {
    // std has no portable statvfs. Rather than add a dependency for a check
    // that only produces a nicer error, the copy is allowed to proceed and a
    // genuine ENOSPC surfaces as an ordinary io error.
    None
}
```

Update `crates/construct-core/src/store/mod.rs` — add to the existing contents:

```rust
pub mod snapshot;

use crate::discovery::World;

/// An opened store plus whatever it needs to stay alive.
pub struct OpenedStore {
    pub(crate) inner: Box<dyn StructureStore>,
    /// Held so the snapshot directory outlives the database handle.
    pub(crate) _snapshot: Option<tempfile::TempDir>,
    /// `Some(bytes)` when the read came from a snapshot rather than the world.
    pub via_snapshot: Option<u64>,
}

impl StructureStore for OpenedStore {
    fn ids(&self) -> Result<Vec<String>> {
        self.inner.ids()
    }
    fn get(&self, id: &str) -> Result<Option<Vec<u8>>> {
        self.inner.get(id)
    }
}

/// Opens a world's structures for reading.
///
/// This *always* copies `db/` and opens the copy. Opening a leveldb database runs
/// recovery and rewrites it, so there is no such thing as a read-only open with
/// this backend — the only safe read is one that never touches the original.
/// There is deliberately no direct path and no fallback logic here.
pub fn open_world_store(world: &World) -> Result<OpenedStore> {
    let db = world.db_path();
    if !db.is_dir() {
        return Err(CoreError::Db(format!("no database at {}", db.display())));
    }
    snapshot::open_via_snapshot(world)
}
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test -p construct-core --test store
```

Expected: 9 passed.

- [ ] **Step 5: Verify against a genuinely locked world**

This is the case tests cannot reach. Launch Minecraft via mcpelauncher, load "construct show", leave it running, then after Task 13 exists run `construct list "construct show"` and confirm it prints the snapshot notice and still lists structures. Record the result in the manual checklist (Task 15). Note POSIX `fcntl` locks are per-process, so an in-process double-open may not fail — only a real second process proves this.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "Fall back to a db/ snapshot when a world is in use

leveldb has no read-only open, so a world Minecraft has open cannot be
read directly. Read commands copy db/ to a temp dir and open the copy,
reporting the size rather than doing it silently.

The fallback triggers on any open failure of an existing db directory
rather than on an error message: the lock error is untyped, and its text
differs between leveldb's POSIX and Windows implementations."
```

---

## Task 11: The catalog

One namespace over both sources. Stage 1 only populates the world source; stage 2 adds the pack source. The `Source` enum and the ambiguity error exist now so that adding packs later is a change in one function rather than a change to every command.

**Files:**
- Create: `crates/construct-core/src/catalog.rs`
- Modify: `crates/construct-core/src/lib.rs`
- Test: inline `#[cfg(test)]` module in `catalog.rs`

**Interfaces:**
- Consumes: `StructureStore`, `MemoryStore` (Task 9); `key` (Task 8); `CoreError` (Task 2).
- Produces:
  - `pub enum Source { World, Pack }` with `pub fn as_str(&self) -> &'static str`
  - `pub struct Entry { pub name: String, pub id: String, pub source: Source, pub size_bytes: u64 }`
  - `pub fn from_world(store: &dyn StructureStore) -> Result<Vec<Entry>>`
  - `pub fn resolve(name: &str, entries: &[Entry], source: Option<Source>) -> Result<Entry>`

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::MemoryStore;

    fn entries() -> Vec<Entry> {
        vec![
            Entry { name: "house".into(), id: "mystructure:house".into(), source: Source::World, size_bytes: 12 },
            Entry { name: "barn".into(), id: "mystructure:barn".into(), source: Source::World, size_bytes: 4 },
            Entry { name: "tower".into(), id: "mystructure:tower".into(), source: Source::Pack, size_bytes: 31 },
        ]
    }

    #[test]
    fn builds_entries_from_a_world_store() {
        let store = MemoryStore::with(&[("house", b"abc"), ("understudy:players", b"de")]);
        let mut got = from_world(&store).unwrap();
        got.sort_by(|a, b| a.name.cmp(&b.name));

        assert_eq!(got[0].name, "house");
        assert_eq!(got[0].id, "mystructure:house");
        assert_eq!(got[0].size_bytes, 3);
        assert_eq!(got[0].source, Source::World);
        // A non-default namespace stays visible in the display name.
        assert_eq!(got[1].name, "understudy:players");
    }

    #[test]
    fn resolves_a_unique_bare_name() {
        assert_eq!(resolve("house", &entries(), None).unwrap().id, "mystructure:house");
    }

    #[test]
    fn a_name_in_both_sources_is_an_error_pointing_at_source() {
        let mut e = entries();
        e.push(Entry { name: "house".into(), id: "mystructure:house".into(), source: Source::Pack, size_bytes: 9 });
        assert!(matches!(
            resolve("house", &e, None),
            Err(CoreError::AmbiguousStructure { .. })
        ));
    }

    #[test]
    fn source_disambiguates_a_name_present_in_both() {
        let mut e = entries();
        e.push(Entry { name: "house".into(), id: "mystructure:house".into(), source: Source::Pack, size_bytes: 9 });
        assert_eq!(resolve("house", &e, Some(Source::Pack)).unwrap().size_bytes, 9);
        assert_eq!(resolve("house", &e, Some(Source::World)).unwrap().size_bytes, 12);
    }

    #[test]
    fn a_missing_structure_suggests_near_matches() {
        let CoreError::StructureNotFound { near, .. } = resolve("hous", &entries(), None).unwrap_err()
        else {
            panic!("expected StructureNotFound");
        };
        assert!(near.contains(&"house".to_string()));
    }

    #[test]
    fn a_qualified_name_resolves() {
        assert_eq!(resolve("mystructure:house", &entries(), None).unwrap().name, "house");
    }

    #[test]
    fn filtering_by_a_source_with_no_matches_is_not_found() {
        assert!(matches!(
            resolve("barn", &entries(), Some(Source::Pack)),
            Err(CoreError::StructureNotFound { .. })
        ));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p construct-core catalog
```

Expected: FAIL to compile — `Entry`, `Source`, `from_world`, `resolve` are not defined.

- [ ] **Step 3: Write the implementation**

`crates/construct-core/src/catalog.rs`:

```rust
//! One structure namespace over both sources.
//!
//! Construct presents world structures and pack structures as a single in-game
//! list, so the CLI does too — two lists would model it worse than the thing it
//! drives. Stage 1 populates only [`Source::World`]; stage 2 adds packs.

use crate::error::{CoreError, Result};
use crate::store::{StructureStore, key};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    World,
    Pack,
}

impl Source {
    pub fn as_str(&self) -> &'static str {
        match self {
            Source::World => "world",
            Source::Pack => "pack",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// What the user types and sees: bare for `mystructure`, qualified otherwise.
    pub name: String,
    /// The fully qualified id, always carrying a namespace.
    pub id: String,
    pub source: Source,
    pub size_bytes: u64,
}

/// Every structure in a world's database.
pub fn from_world(store: &dyn StructureStore) -> Result<Vec<Entry>> {
    let mut out = Vec::new();
    for id in store.ids()? {
        let size_bytes = store.get(&id)?.map(|b| b.len() as u64).unwrap_or(0);
        out.push(Entry {
            name: key::display_name(&id).to_string(),
            id,
            source: Source::World,
            size_bytes,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Finds exactly one structure by name, never guessing between sources.
pub fn resolve(name: &str, entries: &[Entry], source: Option<Source>) -> Result<Entry> {
    let qualified = key::qualify(name);
    let matches: Vec<&Entry> = entries
        .iter()
        .filter(|e| e.name == name || e.id == qualified)
        .filter(|e| source.is_none_or(|s| e.source == s))
        .collect();

    match matches.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(CoreError::StructureNotFound {
            name: name.to_string(),
            near: near_matches(name, entries, source),
        }),
        _ => Err(CoreError::AmbiguousStructure { name: name.to_string() }),
    }
}

fn near_matches(needle: &str, entries: &[Entry], source: Option<Source>) -> Vec<String> {
    let needle = needle.to_lowercase();
    let mut out: Vec<String> = entries
        .iter()
        .filter(|e| source.is_none_or(|s| e.source == s))
        .filter(|e| e.name.to_lowercase().contains(&needle))
        .map(|e| e.name.clone())
        .collect();
    out.truncate(5);
    out
}
```

Add to `crates/construct-core/src/lib.rs`:

```rust
pub mod catalog;
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test -p construct-core catalog
```

Expected: 7 passed.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "Unify structures into one namespace with a source

Construct shows world and pack structures as a single in-game list, so
the CLI does too. Stage 1 populates only the world source; the Source
enum and the both-sources ambiguity error exist now so stage 2 changes
one function rather than every command."
```

---

## Task 12: The CLI shell, output channels, and `construct worlds`

The first user-visible command. Establishes the output contract every later command uses.

**Files:**
- Create: `crates/construct-cli/src/cli.rs`, `src/output.rs`, `src/commands/mod.rs`, `src/commands/worlds.rs`
- Modify: `crates/construct-cli/src/main.rs`
- Test: `crates/construct-cli/tests/cli.rs`

**Interfaces:**
- Consumes: `discovery`, `config`, `CoreError` (Tasks 2–7).
- Produces:
  - `pub struct Cli` (clap) with global `--com-mojang`, `--json`, `--force`, `--source`
  - `pub struct Out { json: bool, warnings: Vec<String> }` with `warn`, `emit`, `finish`
  - `pub fn exit_code(err: &CoreError) -> i32`

- [ ] **Step 1: Write the failing tests**

`crates/construct-cli/tests/cli.rs`:

**Two corrections from the pre-flight scan, both load-bearing:**

*P1 — every CLI test must pin its environment.* `run()` reads `HOME`, `APPDATA`, and
`LOCALAPPDATA` to build the discovery table. A test that leaves them alone discovers
whatever Minecraft installs exist on the machine running it — so the exit-3 test passes on
CI and fails on a developer's Mac, while the JSON test does the reverse. Neither machine can
be green. `bin()` therefore clears all three to a temp directory.

*P2 — `NoInstallations` must not preempt reference resolution.* Returning it before
resolving the world argument breaks `construct list <path>` on any machine with no Minecraft
installed, which is every CI runner — reddening all of Tasks 13 and 14. §6 promises a
filesystem path works as a world reference; that promise cannot depend on a Minecraft
install existing. Report `NoInstallations` only for `worlds`, or when resolution has already
failed.

```rust
use std::process::Command;

/// A binary invocation with a pinned, empty environment, so discovery finds
/// exactly what the test puts there and nothing from the host machine.
fn bin() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_construct"));
    let empty = std::env::temp_dir().join("construct-empty-home");
    std::fs::create_dir_all(&empty).unwrap();
    c.env("HOME", &empty).env("USERPROFILE", &empty);
    c.env_remove("APPDATA").env_remove("LOCALAPPDATA");
    c.env_remove("CONSTRUCT_COM_MOJANG").env_remove("CONSTRUCT_INSTALLATION");
    c.env("CONSTRUCT_CONFIG", empty.join("no-such-config.toml"));
    c
}

#[test]
fn help_lists_the_read_commands() {
    let out = bin().arg("--help").output().unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    for cmd in ["worlds", "list", "export"] {
        assert!(text.contains(cmd), "--help should mention {cmd}:\n{text}");
    }
}

#[test]
fn an_unknown_command_is_a_usage_error() {
    let out = bin().arg("nonsense").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn worlds_json_is_exactly_one_document_on_stdout() {
    // The GUI contract: stdout parses as JSON with no scanning.
    // Points at a real (empty) com.mojang so an installation exists and a
    // payload is emitted — the exit-3 case is a different test.
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("minecraftWorlds")).unwrap();
    let out = bin()
        .args(["worlds", "--json", "--com-mojang", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("stdout was not one JSON doc: {e}\n{text}"));
    assert_eq!(parsed["schema"], 1);
    assert!(parsed["worlds"].is_array());
    assert!(parsed["warnings"].is_array());
}

#[test]
fn warnings_go_to_stderr_and_never_pollute_json_stdout() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("minecraftWorlds")).unwrap();
    let out = bin()
        .args(["worlds", "--json", "--com-mojang", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    // Whatever is on stderr, stdout must still parse.
    assert!(serde_json::from_slice::<serde_json::Value>(&out.stdout).is_ok());
}

#[test]
fn a_path_referenced_world_works_with_no_installations_at_all() {
    // §6 promises a filesystem path is a valid world reference. That must not
    // depend on a Minecraft install existing — CI runners have none.
    let tmp = tempfile::tempdir().unwrap();
    let world = tmp.path().join("SomeWorld");
    std::fs::create_dir_all(world.join("db")).unwrap();
    std::fs::write(world.join("level.dat"), b"x").unwrap();
    let out = bin().args(["list", world.to_str().unwrap()]).output().unwrap();
    // The db is not a real leveldb, so this fails — but it must NOT fail with
    // exit 3 "no installation found", which would mean the check preempted
    // resolution.
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        !err.contains("no Minecraft installation found"),
        "installation check preempted a path reference:\n{err}"
    );
}

#[test]
fn no_installation_found_exits_3_and_lists_probed_paths() {
    let out = bin().args(["worlds", "--com-mojang", "/nonexistent"]).output().unwrap();
    assert_eq!(out.status.code(), Some(3));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("/nonexistent"), "should name what it probed:\n{err}");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p construct-cli
```

Expected: FAIL — the binary only prints its version.

- [ ] **Step 3: Write the output layer**

`crates/construct-cli/src/output.rs`:

```rust
//! The two output channels.
//!
//! Under `--json`, stdout carries exactly one JSON document and nothing else,
//! so a consumer can parse it without scanning. Warnings and progress go to
//! stderr as plain text and are *also* embedded in the payload: a GUI linking
//! construct-core gets them as values, a terminal user gets them as they
//! happen, and neither has to read the other stream.

use serde::Serialize;
use serde_json::{Map, Value, json};

pub struct Out {
    json: bool,
    warnings: Vec<String>,
}

impl Out {
    pub fn new(json: bool) -> Self {
        Self { json, warnings: Vec::new() }
    }

    pub fn is_json(&self) -> bool {
        self.json
    }

    /// Records a warning and shows it immediately on stderr.
    pub fn warn(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        eprintln!("warning: {msg}");
        self.warnings.push(msg);
    }

    /// Emits a human line, suppressed under `--json`.
    pub fn line(&self, text: impl AsRef<str>) {
        if !self.json {
            println!("{}", text.as_ref());
        }
    }

    /// Emits the single JSON document. Does nothing when not in JSON mode.
    pub fn emit<T: Serialize>(&self, payload: T) {
        if !self.json {
            return;
        }
        let mut map = match serde_json::to_value(payload) {
            Ok(Value::Object(m)) => m,
            other => {
                let mut m = Map::new();
                m.insert("value".into(), other.unwrap_or(Value::Null));
                m
            }
        };
        map.insert("schema".into(), json!(1));
        map.insert("warnings".into(), json!(self.warnings));
        println!("{}", Value::Object(map));
    }
}
```

- [ ] **Step 4: Write the CLI definitions**

`crates/construct-cli/src/cli.rs`:

```rust
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "construct", version, about = "Move structures between Minecraft Bedrock worlds")]
pub struct Cli {
    /// Extra com.mojang root to probe. Repeatable.
    #[arg(long, global = true, value_name = "PATH")]
    pub com_mojang: Vec<PathBuf>,

    /// Emit one JSON document on stdout instead of human output.
    #[arg(long, global = true)]
    pub json: bool,

    /// Overwrite an existing target file. Never relaxes the world-in-use refusal.
    #[arg(long, global = true)]
    pub force: bool,

    /// Disambiguate a structure name present in both a world and a pack.
    #[arg(long, global = true, value_enum)]
    pub source: Option<SourceArg>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
pub enum SourceArg {
    World,
    Pack,
}

impl From<SourceArg> for construct_core::catalog::Source {
    fn from(s: SourceArg) -> Self {
        match s {
            SourceArg::World => Self::World,
            SourceArg::Pack => Self::Pack,
        }
    }
}

#[derive(Subcommand)]
pub enum Command {
    /// List discovered worlds.
    Worlds,

    /// List the structures in a world.
    List {
        /// World name, qualified reference, or path.
        world: String,
    },

    /// Write structures out as .mcstructure files.
    Export {
        /// World name, qualified reference, or path.
        world: String,
        /// One or more structure names.
        #[arg(required = true)]
        structures: Vec<String>,
        /// Output file. Only valid with a single structure.
        #[arg(short = 'o', long)]
        output: Option<PathBuf>,
    },
}
```

- [ ] **Step 5: Write `worlds` and the dispatcher**

`crates/construct-cli/src/commands/worlds.rs`:

```rust
use crate::output::Out;
use construct_core::discovery::World;
use construct_core::{Result, discovery};
use serde::Serialize;

#[derive(Serialize)]
struct Payload<'a> {
    worlds: Vec<Row<'a>>,
}

#[derive(Serialize)]
struct Row<'a> {
    installation: &'a str,
    account: Option<&'a str>,
    folder: &'a str,
    display_name: &'a str,
    qualified: String,
    path: String,
    size_bytes: u64,
    last_played: Option<i64>,
    /// "level.dat" or "dir-mtime" — the two disagree often enough to matter.
    last_played_source: &'static str,
}

pub fn run(worlds: &[World], out: &Out) -> Result<()> {
    if !out.is_json() {
        out.line(format!("{:<28} {:<14} {:>8}  {}", "NAME", "INSTALLATION", "SIZE", "REFERENCE"));
        for w in worlds {
            out.line(format!(
                "{:<28} {:<14} {:>8}  {}",
                truncate(&w.display_name, 28),
                w.installation,
                human_size(w.size_bytes),
                w.qualified()
            ));
        }
        if worlds.is_empty() {
            out.line("no worlds found");
        }
    }

    out.emit(Payload {
        worlds: worlds
            .iter()
            .map(|w| Row {
                installation: &w.installation,
                account: w.account.as_deref(),
                folder: &w.folder,
                display_name: &w.display_name,
                qualified: w.qualified(),
                path: w.path.display().to_string(),
                size_bytes: w.size_bytes,
                last_played: w.last_played,
                last_played_source: match w.last_played_source {
                    discovery::LastPlayedSource::LevelDat => "level.dat",
                    discovery::LastPlayedSource::DirMtime => "dir-mtime",
                },
            })
            .collect(),
    });
    Ok(())
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{v:.1} {}", UNITS[unit]) }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max - 1).collect::<String>() + "…"
    }
}
```

`crates/construct-cli/src/commands/mod.rs`:

```rust
pub mod worlds;
```

`crates/construct-cli/src/main.rs`:

```rust
mod cli;
mod commands;
mod output;

use clap::Parser;
use cli::{Cli, Command};
use construct_core::error::CoreError;
use construct_core::{config, discovery};
use output::Out;

fn main() {
    let cli = Cli::parse();
    let mut out = Out::new(cli.json);

    match run(&cli, &mut out) {
        Ok(()) => {}
        Err(err) => {
            report(&err);
            std::process::exit(exit_code(&err));
        }
    }
}

fn run(cli: &Cli, out: &mut Out) -> construct_core::Result<()> {
    let loaded = config::load(None, &|k| std::env::var(k).ok())?;
    for w in &loaded.warnings {
        out.warn(w.clone());
    }

    // Precedence: CLI flag beats everything below it.
    //
    // Config roots keep their configured names — that is the entire reason §7 makes
    // `name` mandatory, and `config::parse` has already rejected duplicates and any
    // name that would shadow a built-in installation. Roots from `--com-mojang` have
    // no name to carry, so they are numbered.
    let mut extra_roots: Vec<(String, std::path::PathBuf)> = loaded
        .config
        .roots
        .iter()
        .map(|r| (r.name.clone(), r.path.clone()))
        .collect();
    for (i, path) in cli.com_mojang.iter().enumerate() {
        extra_roots.push((format!("flag{}", i + 1), path.clone()));
    }

    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_default();
    let appdata = std::env::var("APPDATA").ok().map(std::path::PathBuf::from);
    let localappdata = std::env::var("LOCALAPPDATA").ok().map(std::path::PathBuf::from);

    let mut candidates = discovery::platform::candidates(
        std::path::Path::new(&home),
        appdata.as_deref(),
        localappdata.as_deref(),
    );
    for (name, root) in &extra_roots {
        candidates.push(discovery::platform::Candidate {
            name: name.clone(),
            dev_pack_root: root.clone(),
            world_root_parents: vec![root.clone()],
            per_account: false,
        });
    }
    let probed: Vec<std::path::PathBuf> =
        candidates.iter().map(|c| c.dev_pack_root.clone()).collect();
    // A configured root must be addressable by its name, so assert the wiring holds:
    // every configured root name appears among the candidates.
    debug_assert!(
        loaded.config.roots.iter().all(|r| candidates.iter().any(|c| c.name == r.name)),
        "a configured root lost its name before discovery"
    );

    let installations = discovery::platform::resolve(candidates);
    let worlds = discovery::enumerate(&installations);

    // Deliberately NOT an early return. A world reference may be a filesystem
    // path, which resolves with zero installations — §6 promises that, and every
    // CI runner depends on it. `no_installations` is only reported when it is
    // genuinely the explanation.
    let no_installations = || CoreError::NoInstallations { probed: probed.clone() };
    let resolve_world = |r: &str| {
        discovery::reference::resolve(r, &worlds).map_err(|e| {
            if installations.is_empty() { no_installations() } else { e }
        })
    };

    match &cli.command {
        Command::Worlds if installations.is_empty() => Err(no_installations()),
        Command::Worlds => commands::worlds::run(&worlds, out),
        Command::List { .. } | Command::Export { .. } => {
            unimplemented!("added in the next tasks")
        }
    }
}

/// Every message names the thing, says why, and gives the next action.
fn report(err: &CoreError) {
    eprintln!("error: {err}");
    match err {
        CoreError::NoInstallations { probed } => {
            eprintln!("\nprobed:");
            for p in probed {
                eprintln!("  {}", p.display());
            }
            eprintln!("\nPoint at one explicitly:\n  construct worlds --com-mojang <path>");
        }
        CoreError::AmbiguousWorld { candidates, .. } => {
            eprintln!("\nUse a qualified reference:");
            for c in candidates {
                eprintln!("  {c}");
            }
        }
        CoreError::WorldNotFound { near, .. } if !near.is_empty() => {
            eprintln!("\nDid you mean:");
            for n in near {
                eprintln!("  {n}");
            }
        }
        CoreError::StructureNotFound { near, .. } if !near.is_empty() => {
            eprintln!("\nDid you mean:");
            for n in near {
                eprintln!("  {n}");
            }
        }
        CoreError::TargetExists { .. } => {
            eprintln!("\nPass --force to overwrite.");
        }
        CoreError::AmbiguousStructure { name } => {
            eprintln!("\nDisambiguate with --source:");
            eprintln!("  construct list --source world   # or: --source pack");
            eprintln!("  (structure: {name})");
        }
        _ => {}
    }
}

/// 0 success · 1 failure · 2 usage · 3 not found · 4 world in use.
///
/// Ambiguity is 2, not 3: the target exists, the reference was underspecified.
fn exit_code(err: &CoreError) -> i32 {
    match err {
        CoreError::NoInstallations { .. }
        | CoreError::WorldNotFound { .. }
        | CoreError::StructureNotFound { .. } => 3,
        CoreError::AmbiguousWorld { .. }
        | CoreError::AmbiguousStructure { .. }
        | CoreError::AmbiguousInstallation { .. } => 2,
        CoreError::WorldInUse { .. } => 4,
        _ => 1,
    }
}
```

- [ ] **Step 6: Run the tests to verify they pass**

```bash
cargo test -p construct-cli
```

Expected: 5 passed.

- [ ] **Step 7: Run it against the real machine**

```bash
cargo run -p construct-cli -- worlds
cargo run -p construct-cli -- worlds --json | python3 -m json.tool
```

Expected: four worlds — `Amelix CMP`, `construct show`, `HASTE Deco from java`, `Canopy & WorldEdit` — all under installation `mcpelauncher`, with no account segment in the qualified references, and `Amelix CMP` around 35 MB.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "Add the CLI shell, output channels, and construct worlds

Under --json, stdout is exactly one document carrying schema, payload,
and a warnings array; warnings also stream to stderr as they happen.
Asserted by a test that parses stdout with no scanning.

Exit codes follow the spec, with ambiguity as 2 rather than 3 — the
target exists, the reference was underspecified."
```

---

## Task 13: `construct list`

**Files:**
- Create: `crates/construct-cli/src/commands/list.rs`
- Modify: `crates/construct-cli/src/commands/mod.rs`, `src/main.rs`
- Test: `crates/construct-cli/tests/cli.rs` (extend), `crates/construct-cli/tests/fixtures/` (symlink or copy of the core fixture)

**Interfaces:**
- Consumes: `reference::resolve` (Task 6), `open_world_store` (Task 10), `catalog::from_world` (Task 11).
- Produces: `pub fn run(world: &World, source: Option<Source>, out: &mut Out) -> Result<()>`

- [ ] **Step 1: Write the failing tests**

Append to `crates/construct-cli/tests/cli.rs`:

```rust
/// Extract the shared fixture world and return its directory.
fn fixture_world() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let gz = std::fs::File::open("../construct-core/tests/fixtures/world.tar.gz")
        .expect("fixture missing");
    tar::Archive::new(flate2::read::GzDecoder::new(gz)).unpack(tmp.path()).unwrap();
    let world = tmp.path().join("test_level");
    (tmp, world)
}

#[test]
fn list_by_path_shows_structures_with_their_source() {
    let (_tmp, world) = fixture_world();
    let out = bin().args(["list", world.to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("house"), "{text}");
    assert!(text.contains("barn"), "{text}");
    assert!(text.contains("world"), "source column should say world:\n{text}");
}

#[test]
fn list_json_carries_schema_and_entries() {
    let (_tmp, world) = fixture_world();
    let out = bin().args(["list", world.to_str().unwrap(), "--json"]).output().unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["schema"], 1);
    let names: Vec<&str> =
        v["structures"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"house"));
    assert!(names.contains(&"understudy:players"), "non-default namespaces stay qualified");
}

#[test]
fn list_source_pack_is_empty_in_stage_one() {
    let (_tmp, world) = fixture_world();
    let out = bin()
        .args(["list", world.to_str().unwrap(), "--source", "pack", "--json"])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["structures"].as_array().unwrap().len(), 0);
}

#[test]
fn list_of_a_missing_world_exits_3() {
    let out = bin().args(["list", "definitely-not-a-world"]).output().unwrap();
    assert_eq!(out.status.code(), Some(3));
}
```

Add to `crates/construct-cli/Cargo.toml` under `[dev-dependencies]`:

```toml
tempfile.workspace = true
tar.workspace = true
flate2.workspace = true
serde_json.workspace = true
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p construct-cli list
```

Expected: FAIL — `list` panics with `unimplemented!`.

- [ ] **Step 3: Write the implementation**

`crates/construct-cli/src/commands/list.rs`:

```rust
use crate::commands::worlds::human_size;
use crate::output::Out;
use construct_core::catalog::{self, Source};
use construct_core::discovery::World;
use construct_core::store::{self, StructureStore};
use construct_core::Result;
use serde::Serialize;

#[derive(Serialize)]
struct Payload<'a> {
    world: String,
    structures: Vec<Row<'a>>,
}

#[derive(Serialize)]
struct Row<'a> {
    name: &'a str,
    id: &'a str,
    source: &'static str,
    size_bytes: u64,
}

pub fn run(world: &World, source: Option<Source>, out: &mut Out) -> Result<()> {
    let store = store::open_world_store(world)?;
    if let Some(bytes) = store.via_snapshot {
        out.warn(format!("reading from a {} snapshot", human_size(bytes)));
    }

    let mut entries = catalog::from_world(&store)?;
    // Stage 2 adds pack entries here. Until then, filtering to `pack` is
    // legitimately empty rather than an error.
    if let Some(s) = source {
        entries.retain(|e| e.source == s);
    }

    if !out.is_json() {
        if entries.is_empty() {
            out.line("no structures");
        } else {
            out.line(format!("{:<24} {:<8} {:>9}", "NAME", "SOURCE", "SIZE"));
            for e in &entries {
                out.line(format!(
                    "{:<24} {:<8} {:>9}",
                    e.name,
                    e.source.as_str(),
                    human_size(e.size_bytes)
                ));
            }
        }
    }

    out.emit(Payload {
        world: world.qualified(),
        structures: entries
            .iter()
            .map(|e| Row {
                name: &e.name,
                id: &e.id,
                source: e.source.as_str(),
                size_bytes: e.size_bytes,
            })
            .collect(),
    });
    Ok(())
}
```

Update `crates/construct-cli/src/commands/mod.rs`:

```rust
pub mod list;
pub mod worlds;
```

In `main.rs`, replace the `Command::List` arm:

```rust
        Command::List { world } => {
            let w = resolve_world(world)?;
            commands::list::run(&w, cli.source.map(Into::into), out)
        }
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test -p construct-cli
```

Expected: 9 passed.

- [ ] **Step 5: Run against the real world**

```bash
cargo run -p construct-cli -- list "construct show"
```

Expected: one row, `copy`, source `world` — the key confirmed present in `Ssu8ww1SFbM=`. If more rows appear, that is fine and expected: a raw grep only sees uncompressed leveldb blocks, so one was a floor.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "Add construct list

Reads a world's structures through the catalog, announcing a snapshot
read when the world is in use rather than doing it silently.

--source pack is legitimately empty in stage 1 rather than an error;
stage 2 fills it in."
```

---

## Task 14: `construct export`

The command that replaces the holoprint upload. Byte-transparency means it copies bytes and parses nothing.

**Files:**
- Create: `crates/construct-cli/src/commands/export.rs`
- Modify: `crates/construct-cli/src/commands/mod.rs`, `src/main.rs`
- Test: `crates/construct-cli/tests/cli.rs` (extend)

**Interfaces:**
- Consumes: `catalog::resolve` (Task 11), `open_world_store` (Task 10).
- Produces: `pub fn run(world, structures: &[String], output: Option<&Path>, source, force, out) -> Result<()>`

- [ ] **Step 1: Write the failing tests**

Append to `crates/construct-cli/tests/cli.rs`:

```rust
#[test]
fn export_without_o_writes_the_derived_name_into_cwd() {
    let (_tmp, world) = fixture_world();
    let dir = tempfile::tempdir().unwrap();
    let out = bin()
        .current_dir(dir.path())
        .args(["export", world.to_str().unwrap(), "house"])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let written = dir.path().join("house.mcstructure");
    assert!(written.is_file());
    // Byte transparency: the file is the database value, untouched.
    assert_eq!(std::fs::read(&written).unwrap()[0], 0x0a);
}

#[test]
fn export_with_o_uses_the_given_name() {
    let (_tmp, world) = fixture_world();
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("custom.mcstructure");
    let out = bin()
        .args(["export", world.to_str().unwrap(), "house", "-o", target.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(target.is_file());
}

#[test]
fn export_refuses_an_existing_target_and_points_at_force() {
    let (_tmp, world) = fixture_world();
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("taken.mcstructure");
    std::fs::write(&target, b"existing").unwrap();

    let out = bin()
        .args(["export", world.to_str().unwrap(), "house", "-o", target.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--force"));
    assert_eq!(std::fs::read(&target).unwrap(), b"existing", "must not have overwritten");
}

#[test]
fn export_force_overwrites() {
    let (_tmp, world) = fixture_world();
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("taken.mcstructure");
    std::fs::write(&target, b"existing").unwrap();

    let out = bin()
        .args(["export", world.to_str().unwrap(), "house", "-o", target.to_str().unwrap(), "--force"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_ne!(std::fs::read(&target).unwrap(), b"existing");
}

#[test]
fn exporting_several_structures_writes_one_file_each() {
    let (_tmp, world) = fixture_world();
    let dir = tempfile::tempdir().unwrap();
    let out = bin()
        .current_dir(dir.path())
        .args(["export", world.to_str().unwrap(), "house", "barn"])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(dir.path().join("house.mcstructure").is_file());
    assert!(dir.path().join("barn.mcstructure").is_file());
}

#[test]
fn several_structures_with_o_is_a_usage_error() {
    // -o names a single file; it cannot name several.
    let (_tmp, world) = fixture_world();
    let dir = tempfile::tempdir().unwrap();
    let out = bin()
        .args([
            "export", world.to_str().unwrap(), "house", "barn",
            "-o", dir.path().join("x.mcstructure").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn a_multi_export_refuses_before_writing_anything_if_one_target_exists() {
    let (_tmp, world) = fixture_world();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("barn.mcstructure"), b"existing").unwrap();

    let out = bin()
        .current_dir(dir.path())
        .args(["export", world.to_str().unwrap(), "house", "barn"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(!dir.path().join("house.mcstructure").exists(), "must write nothing on refusal");
}

#[test]
fn exporting_a_missing_structure_exits_3() {
    let (_tmp, world) = fixture_world();
    let out = bin().args(["export", world.to_str().unwrap(), "nope"]).output().unwrap();
    assert_eq!(out.status.code(), Some(3));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

```bash
cargo test -p construct-cli export
```

Expected: FAIL — `export` panics with `unimplemented!`.

- [ ] **Step 3: Write the implementation**

`crates/construct-cli/src/commands/export.rs`:

```rust
//! Writing structures out as `.mcstructure` files.
//!
//! A structure's leveldb value is byte-identical to a `.mcstructure` file, so
//! this command copies bytes and parses nothing.

use crate::commands::worlds::human_size;
use crate::output::Out;
use construct_core::catalog::{self, Source};
use construct_core::discovery::World;
use construct_core::error::CoreError;
use construct_core::store::{self, StructureStore};
use construct_core::Result;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
struct Payload {
    world: String,
    written: Vec<Written>,
}

#[derive(Serialize)]
struct Written {
    name: String,
    path: String,
    bytes: u64,
}

pub fn run(
    world: &World,
    structures: &[String],
    output: Option<&Path>,
    source: Option<Source>,
    force: bool,
    out: &mut Out,
) -> Result<()> {
    let store = store::open_world_store(world)?;
    if let Some(bytes) = store.via_snapshot {
        out.warn(format!("reading from a {} snapshot", human_size(bytes)));
    }
    let entries = catalog::from_world(&store)?;

    // Resolve every name and target path before writing anything, so a
    // collision refuses the whole command rather than leaving half a job done.
    let mut plan: Vec<(catalog::Entry, PathBuf)> = Vec::new();
    for name in structures {
        let entry = catalog::resolve(name, &entries, source)?;
        let target = match output {
            Some(path) => path.to_path_buf(),
            None => PathBuf::from(format!("{}.mcstructure", entry.name)),
        };
        plan.push((entry, target));
    }

    for (_, target) in &plan {
        if target.exists() && !force {
            return Err(CoreError::TargetExists { path: target.clone() });
        }
    }

    let mut written = Vec::new();
    for (entry, target) in plan {
        let bytes = store
            .get(&entry.id)?
            .ok_or_else(|| CoreError::StructureNotFound { name: entry.name.clone(), near: vec![] })?;
        if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target, &bytes)?;
        out.line(format!("wrote {} ({})", target.display(), human_size(bytes.len() as u64)));
        written.push(Written {
            name: entry.name,
            path: target.display().to_string(),
            bytes: bytes.len() as u64,
        });
    }

    out.emit(Payload { world: world.qualified(), written });
    Ok(())
}
```

Update `crates/construct-cli/src/commands/mod.rs`:

```rust
pub mod export;
pub mod list;
pub mod worlds;
```

In `main.rs`, replace the `Command::Export` arm:

```rust
        Command::Export { world, structures, output } => {
            if structures.len() > 1 && output.is_some() {
                // -o names a single file and cannot name several. Usage error,
                // not a failure: nothing was attempted.
                eprintln!(
                    "error: -o takes a single output file, but {} structures were given\n\n\
                     Drop -o to write one file per structure, or pass --merge (stage 3).",
                    structures.len()
                );
                std::process::exit(2);
            }
            let w = resolve_world(world)?;
            commands::export::run(
                &w,
                structures,
                output.as_deref(),
                cli.source.map(Into::into),
                cli.force,
                out,
            )
        }
```

- [ ] **Step 4: Run the tests to verify they pass**

```bash
cargo test -p construct-cli
```

Expected: 17 passed.

- [ ] **Step 5: Verify byte transparency end to end**

This is the property the whole staging argument rests on. Export from the fixture and compare against the value read directly from the database:

```bash
cargo test --workspace
cd /tmp && cargo run --manifest-path <repo>/Cargo.toml -p construct-cli -- \
  export "$HOME/Library/Application Support/mcpelauncher/games/com.mojang/minecraftWorlds/Ssu8ww1SFbM=" copy
xxd copy.mcstructure | head -3
```

Expected: a file whose first byte is `0x0a`. Confirm it is a real structure by opening it in a `.mcstructure` viewer, or by carrying it forward to the stage 3 codec work.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "Add construct export

Copies bytes and parses nothing — a structure's leveldb value is already
a .mcstructure file. Every name and target is resolved before anything is
written, so a collision refuses the whole command rather than leaving a
half-finished job.

Several structures without --merge write one file each; -o with several
is a usage error, since it names one file."
```

---

## Task 15: README and the manual checklist

**Files:**
- Create: `README.md`, `LICENSE`, `docs/manual-verification.md`

**Interfaces:**
- Consumes: everything.
- Produces: no code.

- [ ] **Step 1: Write the README**

Lead with the workflow being replaced — that is what tells a reader whether this tool is for them.

`README.md`:

````markdown
# ConstructCLI

Move structures between Minecraft Bedrock worlds from the command line.

[Construct](https://github.com/ForestOfLight/Construct)'s documented workflow
for getting a structure out of a world is to upload the whole world to
holoprint-mc.github.io and use its "Extract From World" feature. This replaces
that with:

```
construct export "My Survival" house
```

## Status

Stage 1 of 4. The read path works: `worlds`, `list`, `export`. Installing
Construct, importing, copying, merging, and deleting are not built yet — see
`docs/superpowers/specs/2026-08-31-constructcli-design.md`.

## Install

Prebuilt binaries are on the releases page. From source you need Rust,
CMake, and a C++ compiler, because the leveldb backend is Mojang's own C++
implementation rather than a reimplementation:

```
cargo install --git https://github.com/ForestOfLight/ConstructCLI
```

## Usage

```
construct worlds                          # every world this machine can see
construct list <world>                    # structures in a world
construct export <world> <structure>      # write <structure>.mcstructure
construct export <world> <s1> <s2>        # one file each
construct export <world> <s> -o out.mcstructure
```

A `<world>` is a world's name, a folder name, a qualified
`<installation>/<account>/<world>` reference, or a path to a world directory.
When a name is ambiguous, the error prints the qualified forms to pick from.

`--json` makes any command print a single JSON document instead, with a
`schema` field, for scripting and for the GUI this library is meant to support.

## Safety

Reading a world that Minecraft currently has open is safe: leveldb has no
read-only mode, so ConstructCLI copies `db/` to a temporary directory and reads
the copy, telling you when it does. Nothing in stage 1 writes to a world.

## License

MIT, matching Construct. The test fixture world is derived from
[bedrock-rs](https://github.com/bedrock-crustaceans/bedrock-rs) under Apache-2.0;
see `crates/construct-core/tests/fixtures/NOTICE`.
````

- [ ] **Step 2: Add the MIT license**

Write a standard MIT `LICENSE` with the current year and `ForestOfLight` as the copyright holder.

- [ ] **Step 3: Write the manual checklist**

`docs/manual-verification.md`:

```markdown
# Manual verification

Things automated tests cannot settle. Stage 1 items only; §16 of the design
spec holds the full list.

- [ ] `construct worlds` finds every world the launcher shows, with matching names.
- [ ] `construct list` on a world with structures matches what Construct shows in-game.
- [ ] **Locked world:** load a world in Minecraft, leave it running, then
      `construct list <that world>`. It must print the snapshot notice and still
      list structures. POSIX `fcntl` locks are per-process, so this cannot be
      tested from inside the test binary — only a second process proves it.
- [ ] An exported `.mcstructure` loads in a structure block, or in a
      third-party `.mcstructure` viewer.
- [ ] *(Windows, needs real hardware)* GDK worlds under
      `%appdata%\Minecraft Bedrock\Users\<account>\...` are discovered, and the
      qualified reference includes the account segment.
- [ ] *(Windows)* Files written into the GDK folder by an ordinary process read
      back in-game. Expected to be a non-issue — the ACL problem was specific to
      UWP's `LocalState` inside an AppContainer, and GDK uses ordinary
      `AppData\Roaming`.
```

- [ ] **Step 4: Verify the whole workspace one more time**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p construct-cli -- worlds
```

Expected: clean formatting, no warnings, all tests passing, four worlds listed.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "Add README, license, and the manual verification checklist

The README leads with the holoprint upload workflow being replaced,
which is what tells a reader whether this tool is for them."
```

---

## Self-Review

**Spec coverage for stages 0–1.** §14 stage 0 → Task 1. Scaffolding → Task 2. `config` (§7) → Task 7. `discovery` (§6), including multi-root, world references, and paths-as-references → Tasks 4, 5, 6. `store` reads (§4, §8) → Tasks 8, 9, 10. `worlds` / `list` / `export` (§5) → Tasks 12, 13, 14. Output and `--json` (§5) → Task 12. Exit codes (§11) → Task 12. Testing strategy (§12) → Tasks 3–14, with the disposable-copy pattern and the temp-dir guard in Task 9. Build and distribution (§13) → Task 2's CI.

**Deliberately deferred to stages 2–4, and not gaps in this plan:** `pack`, `install`, `status`, `experiment`, `import`, `copy`, the `mcstructure` codec, `merge`, `backup`, and all leveldb writes. `catalog::Source::Pack` exists but is always empty, which Task 13 asserts.

**Two things this plan discovered that the spec did not know:**

1. `leveldb::Options::create_if_missing` is `false` and the FFI never sets it, so a database cannot be created from Rust. §12's "tarred fixture world" is therefore mandatory rather than stylistic, and Task 9 generates one from bedrock-rs's Apache-2.0 test level.
2. The world-in-use error is untyped (`CoreError::Db(String)`) and its text differs between leveldb's POSIX and Windows implementations. `bedrock_level::Error::DatabaseLockError` is mutex poisoning, not the LOCK file. Task 10 therefore triggers the snapshot fallback on any open failure of an existing `db` directory rather than matching a message. Worth folding back into §8 of the spec after Task 10 confirms it.

**Type consistency.** `World`, `Installation`, `WorldRoot`, `LastPlayedSource` are defined in Tasks 4–5 and used unchanged in 6, 10, 12, 13, 14. `StructureStore::{ids, get}` is defined in Task 9 and used unchanged in 10, 11, 13, 14. `catalog::{Entry, Source}` is defined in Task 11 and used in 13, 14. `Out::{warn, line, emit, is_json}` is defined in Task 12 and used in 13, 14. `human_size` is defined once in `commands/worlds.rs` and imported by `list` and `export`. `CoreError` variants used by `exit_code` in Task 12 all exist in Task 2.

**Collision handling.** `CoreError::TargetExists` is its own variant rather than a reused `Db(String)`, so the "point at `--force`" hint lives in the CLI's `report` function alongside every other next-action hint, instead of being baked into an error message in the core crate. It falls through `exit_code` to `1`, as §11 requires.
