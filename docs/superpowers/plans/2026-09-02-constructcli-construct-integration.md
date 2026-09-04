# Construct Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver stage 2 of the spec — `pack`, `catalog`, `install`, `status`, `experiment`, `import`, `copy`, and `delete --source pack` — so the CLI drives Construct itself, not just the worlds beside it.

**Architecture:** Stage 1 built the read path: discovery, a snapshot-only leveldb reader, and `worlds` / `list` / `export`. Stage 2 adds the *other* half of Construct's documented manual workflow. Two new subsystems sit side by side in `construct-core`: a `pack` module that reads and writes Construct's `structures/` folder (ordinary files — no database involved), and an `install` module that fetches a release, places the packs by UUID, and enables them in a world. They meet in `catalog`, which already models "one structure namespace, two sources" and gains its second source here.

**Tech Stack:** Rust 2024, `serde_json` for pack manifests and world pack lists, `nbtx` for `level.dat`, `ureq` 3.4 for the GitHub releases API, `zip` 8.6 for `.mcaddon` extraction. No async runtime.

**Spec:** `docs/superpowers/specs/2026-08-31-constructcli-design.md` — the binding authority. Where this plan and the spec disagree, the spec wins and the plan is wrong.

## Global Constraints

Every task's requirements implicitly include this section.

- **Rust edition 2024, floor 1.88.** Let-chains are in use. `cargo` lives at `~/.cargo/bin`; export it on `PATH` before building.
- **First build in a fresh checkout runs `./scripts/setup-deps.sh`** — it clones and patches the pinned `bedrock-rs` and `leveldb-sys` forks into git-ignored `third_party/checkouts/`. Idempotent; re-running is free.
- **`construct-core` never prints, never panics, and never references arguments or exit codes.** It returns typed values and `CoreError`. `construct-cli` owns `clap`, formatting, and exit codes.
- **Exit codes:** `0` success · `1` failure · `2` usage error · `3` not found · `4` world in use · `5` partial success.
- **`--json` emits exactly ONE JSON document on stdout and nothing else**, carrying `"schema": 1`. Warnings go to stderr as plain text *and* into the payload's `"warnings"` array.
- **One collision rule everywhere:** `export -o`, `import`, and `copy` refuse when the target exists; `--force` overwrites. `--force` governs file collisions only and never relaxes a LOCK refusal.
- **A world's leveldb is never opened directly.** Reads go through `store::open_world_store`, which copies `db/` and opens the copy. Stage 2 adds no leveldb write of any kind.
- **Every `level.dat` write:** back up the file first (§8), run the fidelity gate (§10), write to a temporary file in the same directory and rename, then re-read and verify. "Write succeeded, value unchanged" is a failure.
- **Known Construct coordinates:** BP header UUID `8c0c0153-d8b9-482a-889f-aef922b8fe58`, RP header UUID `375ec465-3dc1-429f-8b4c-a337889e1ed4`, `min_engine_version [1,26,40]`. Packs are matched **by header UUID, never by folder name**.
- **A pack structure file directly in `structures/` is `mystructure:<stem>`**; one in `structures/<ns>/` is `<ns>:<stem>` (§17 — the flat form is confirmed, the nested form is inferred and read-only by default).
- **No test touches the network or the user's real Minecraft data.** The releases client is a trait; tests use a stub. Synthetic trees go in `tempfile::tempdir()`.
- **Before every commit:** `cargo fmt --all` and `cargo clippy --all-targets -- -D warnings` must be clean, and `cargo test --workspace` must pass.

---

## File Structure

**New in `construct-core`:**

| File | Responsibility |
|---|---|
| `src/pack/mod.rs` | `Pack`, locating packs in a root, finding Construct by UUID, choosing a target for a world |
| `src/pack/manifest.rs` | `manifest.json` — a small local serde struct, BP vs RP by module type |
| `src/pack/structures.rs` | The `structures/` folder: enumerate, path-for-id, write, remove, derive a name from a file stem |
| `src/worldpacks.rs` | `world_behavior_packs.json` / `world_resource_packs.json` read and upsert |
| `src/backup.rs` | Copy a file into the backup directory; retention per world reference |
| `src/install/mod.rs` | Place packs, preserve `structures/`, enable in a world, report the outcome |
| `src/install/releases.rs` | `Releases` trait, the GitHub implementation, asset matching |
| `src/install/mcaddon.rs` | Extract an `.mcaddon`, identify BP and RP |

**Modified in `construct-core`:** `src/catalog.rs` (pack source), `src/store/mod.rs` (`size`), `src/leveldat.rs` (write path), `src/error.rs` (new variants), `src/lib.rs` (module list), `Cargo.toml`.

**New in `construct-cli`:** `src/commands/import.rs`, `copy.rs`, `delete.rs`, `experiment.rs`, `install.rs`, `status.rs`.

**Modified in `construct-cli`:** `src/cli.rs` (subcommands), `src/main.rs` (wiring, exit 5), `src/commands/list.rs` (both sources), `src/commands/mod.rs`.

**Tests:** unit tests live beside the code in `#[cfg(test)] mod tests`, matching stage 1. New integration files: `crates/construct-core/tests/pack.rs` and `crates/construct-core/tests/install.rs`. CLI behaviour is appended to `crates/construct-cli/tests/cli.rs`.

---

### Task 1: Carried-forward corrections in `store` and `catalog`

Stage 1 triaged four findings into `docs/carried-forward.md` that all sit in code this stage
rewrites. Fixing them first means later tasks build on corrected foundations instead of
inheriting the defects.

**Files:**
- Modify: `crates/construct-core/src/store/key.rs`
- Modify: `crates/construct-core/src/store/mod.rs`
- Modify: `crates/construct-core/src/store/bedrock.rs`
- Modify: `crates/construct-core/src/catalog.rs`
- Modify: `crates/construct-core/src/error.rs`

**Interfaces:**
- Consumes: `StructureStore::{ids, get}`, `catalog::{Entry, Source, from_world, resolve}`.
- Produces:
  - `key::encode_exact(id: &str) -> Vec<u8>` — the key for an id *without* qualifying it.
  - `key::candidates(id: &str) -> Vec<Vec<u8>>` — every key an id could name, qualified form first.
  - `StructureStore::sizes(&self) -> Result<Vec<(String, u64)>>` — id and byte length in one pass. Default implementation calls `ids()` then `get()`; `BedrockStore` overrides it.
  - `CoreError::AmbiguousStructure { name: String, sources: Vec<String> }` — now carries which sources matched.

- [ ] **Step 1: Write the failing tests**

In `crates/construct-core/src/store/key.rs`, inside `mod tests`:

```rust
#[test]
fn an_unqualified_key_is_reachable_by_its_own_name() {
    // A world Minecraft did not write can hold `structuretemplate_foo` with no
    // namespace. `list` shows it, so `export` must be able to fetch it.
    assert_eq!(encode_exact("foo"), b"structuretemplate_foo".to_vec());
    assert_eq!(
        candidates("foo"),
        vec![
            b"structuretemplate_mystructure:foo".to_vec(),
            b"structuretemplate_foo".to_vec(),
        ]
    );
}

#[test]
fn a_qualified_name_has_exactly_one_candidate() {
    assert_eq!(
        candidates("understudy:players"),
        vec![b"structuretemplate_understudy:players".to_vec()]
    );
}
```

In `crates/construct-core/src/store/mod.rs`, inside `mod tests` (create the module if absent):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_memory_store_fetches_an_unqualified_id() {
        let mut map = BTreeMap::new();
        map.insert("foo".to_string(), b"bytes".to_vec());
        let store = MemoryStore(map);
        assert_eq!(store.get("foo").unwrap(), Some(b"bytes".to_vec()));
    }

    #[test]
    fn sizes_reports_every_id_with_its_length() {
        let store = MemoryStore::with(&[("house", b"abc"), ("barn", b"de")]);
        let mut got = store.sizes().unwrap();
        got.sort();
        assert_eq!(
            got,
            vec![
                ("mystructure:barn".to_string(), 2),
                ("mystructure:house".to_string(), 3),
            ]
        );
    }
}
```

In `crates/construct-core/src/catalog.rs`, inside `mod tests`:

```rust
#[test]
fn entries_sort_by_name_then_source() {
    // Two sources can hold the same display name; the order between them is
    // stated rather than inherited from concatenation order.
    let entries = vec![
        Entry { name: "a".into(), id: "mystructure:a".into(), source: Source::Pack,  size_bytes: 1, path: None },
        Entry { name: "a".into(), id: "mystructure:a".into(), source: Source::World, size_bytes: 1, path: None },
    ];
    let mut sorted = entries.clone();
    sort(&mut sorted);
    assert_eq!(sorted[0].source, Source::World);
    assert_eq!(sorted[1].source, Source::Pack);
}

#[test]
fn an_ambiguous_name_names_the_sources_that_matched() {
    let mut e = entries();
    e.push(Entry { name: "house".into(), id: "mystructure:house".into(), source: Source::Pack, size_bytes: 9, path: None });
    let CoreError::AmbiguousStructure { sources, .. } = resolve("house", &e, None).unwrap_err()
    else {
        panic!("expected AmbiguousStructure");
    };
    assert_eq!(sources, vec!["world".to_string(), "pack".to_string()]);
}
```

- [ ] **Step 2: Run them and watch them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p construct-core key:: store:: catalog::
```

Expected: compile errors — `encode_exact`, `candidates`, `sizes`, `sort`, and the `path` field do not exist.

- [ ] **Step 3: Add the key helpers**

In `key.rs`, alongside `encode`:

```rust
/// The leveldb key for an id exactly as given, with no namespace added.
pub fn encode_exact(id: &str) -> Vec<u8> {
    let mut out = PREFIX.to_vec();
    out.extend_from_slice(id.as_bytes());
    out
}

/// Every key a user-supplied name could refer to, most likely first.
///
/// A bare name normally means the `mystructure` namespace, but a world that
/// Minecraft did not write can hold a key with no namespace at all — and stage
/// 1's `list` shows those, so `export` has to be able to fetch them.
pub fn candidates(id: &str) -> Vec<Vec<u8>> {
    let qualified = encode(id);
    let exact = encode_exact(id);
    if qualified == exact {
        vec![qualified]
    } else {
        vec![qualified, exact]
    }
}
```

- [ ] **Step 4: Widen `get` and add `sizes`**

In `store/mod.rs`, add to the trait:

```rust
    /// Every structure id with the byte length of its value.
    ///
    /// Separate from `ids` because `list` needs both and the leveldb backend can
    /// produce them in a single pass. The default implementation is the obvious
    /// two-step; backends that can do better should.
    fn sizes(&self) -> Result<Vec<(String, u64)>> {
        let mut out = Vec::new();
        for id in self.ids()? {
            let len = self.get(&id)?.map(|b| b.len() as u64).unwrap_or(0);
            out.push((id, len));
        }
        Ok(out)
    }
```

And fix `MemoryStore::get` to try both forms:

```rust
    fn get(&self, id: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .0
            .get(&key::qualify(id))
            .or_else(|| self.0.get(id))
            .cloned())
    }
```

In `bedrock.rs`:

```rust
    fn get(&self, id: &str) -> Result<Option<Vec<u8>>> {
        for k in key::candidates(id) {
            if let Some(buf) = self.db.get(&k).map_err(|e| CoreError::Db(e.to_string()))? {
                return Ok(Some(buf.to_vec()));
            }
        }
        Ok(None)
    }

    fn sizes(&self) -> Result<Vec<(String, u64)>> {
        // One pass over the iterator, which yields key and value together.
        // Stage 1 read every structure twice on a `list` — 63.5 MB on a real
        // 910-structure world.
        let mut out = Vec::new();
        let mut keys = self.db.keys();
        for kv in &mut keys {
            if let Some(id) = key::decode(&kv.key()) {
                out.push((id, kv.value().len() as u64));
            }
        }
        Ok(out)
    }
```

- [ ] **Step 5: Rework `catalog`**

Add `path` to `Entry` — pack entries need to name the file they came from, and
deriving it again at delete time would be a second chance to get it wrong:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// What the user types and sees: bare for `mystructure`, qualified otherwise.
    pub name: String,
    /// The fully qualified id, always carrying a namespace.
    pub id: String,
    pub source: Source,
    pub size_bytes: u64,
    /// Where the bytes live, for [`Source::Pack`]. `None` for world structures,
    /// which live in a database rather than a file.
    pub path: Option<std::path::PathBuf>,
}
```

Replace `from_world`'s body and the ad-hoc sort:

```rust
/// The order `list` presents and every command resolves against.
///
/// Name first, then source, so that two entries sharing a display name across
/// sources have a stated order rather than one inherited from concatenation.
pub fn sort(entries: &mut [Entry]) {
    entries.sort_by(|a, b| a.name.cmp(&b.name).then(a.source.cmp(&b.source)));
}

/// Every structure in a world's database.
pub fn from_world(store: &dyn StructureStore) -> Result<Vec<Entry>> {
    let mut out: Vec<Entry> = store
        .sizes()?
        .into_iter()
        .map(|(id, size_bytes)| Entry {
            name: key::display_name(&id).to_string(),
            id,
            source: Source::World,
            size_bytes,
            path: None,
        })
        .collect();
    sort(&mut out);
    Ok(out)
}
```

`sort` compares `Source`, so derive the ordering traits on it and put `World` first — a world
structure is the thing the user saved, a pack structure is a file that was put there:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    World,
    Pack,
}
```

Then make the ambiguity error carry its evidence:

```rust
        _ => Err(CoreError::AmbiguousStructure {
            name: name.to_string(),
            sources: matches.iter().map(|e| e.source.as_str().to_string()).collect(),
        }),
```

- [ ] **Step 6: Update the error variant and its CLI rendering**

In `error.rs`:

```rust
    #[error("structure {name} matches {} sources", sources.len())]
    AmbiguousStructure { name: String, sources: Vec<String> },
```

In `crates/construct-cli/src/main.rs`, replace the `AmbiguousStructure` arm of `report`:

```rust
        CoreError::AmbiguousStructure { name, sources } => {
            eprintln!("\n{name} exists in: {}", sources.join(", "));
            eprintln!("\nDisambiguate with --source:");
            eprintln!("  construct list <world> --source world   # or: --source pack");
            // Construct's own list resolves this by letting the pack copy win
            // (§17). The CLI refuses instead — but the user is usually asking
            // which one the game shows, so answer it.
            if sources.iter().any(|s| s == "pack") {
                eprintln!("\nConstruct shows the pack copy in-game.");
            }
        }
```

- [ ] **Step 7: Run the whole suite**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test --workspace
cargo clippy --all-targets -- -D warnings && cargo fmt --all --check
```

Expected: PASS. Existing `catalog` tests need `path: None` added to their literals; that is
part of this task, not a later one.

- [ ] **Step 8: Commit**

```bash
git add crates/construct-core/src crates/construct-cli/src
git commit -m "Fix what stage 1 carried forward into catalog and store"
```

---

### Task 2: `manifest.json`

**Files:**
- Create: `crates/construct-core/src/pack/mod.rs`
- Create: `crates/construct-core/src/pack/manifest.rs`
- Modify: `crates/construct-core/src/lib.rs`
- Modify: `crates/construct-core/src/error.rs`

**Interfaces:**
- Consumes: `serde_json`, `CoreError`.
- Produces:
  - `pack::manifest::{Manifest, PackKind}` where `Manifest { name: String, uuid: String, version: [u32; 3], kind: PackKind }` and `PackKind { Behavior, Resource }`.
  - `pack::manifest::parse(text: &str, path: &Path) -> Result<Manifest>`
  - `pack::manifest::read(pack_dir: &Path) -> Result<Manifest>` — reads `pack_dir/manifest.json`.
  - `pack::manifest::version_string(v: [u32; 3]) -> String` — `"1.2.0"`.
  - `CoreError::BadPack { path: PathBuf, reason: String }`.

Real manifests, measured from `Construct-v1.2.0.mcaddon`: the BP header carries
`uuid 8c0c0153-d8b9-482a-889f-aef922b8fe58`, `version [1, 2, 0]`, and modules of type `data`
and `script`; the RP header carries `375ec465-3dc1-429f-8b4c-a337889e1ed4` and one module of
type `resources`. **A pack is a resource pack when any module has type `resources`, and a
behavior pack otherwise** — that is the only field that distinguishes them.

- [ ] **Step 1: Write the failing test**

Create `crates/construct-core/src/pack/manifest.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// Trimmed from the real Construct[BP]/manifest.json.
    const BP: &str = r#"{
        "format_version": 2,
        "header": {
            "name": "Construct [BP] v1.2.0",
            "uuid": "8c0c0153-d8b9-482a-889f-aef922b8fe58",
            "min_engine_version": [1, 26, 40],
            "version": [1, 2, 0]
        },
        "modules": [
            { "type": "data", "uuid": "f4d52ae1-2c26-4938-b8c2-7e455d495620", "version": [1, 0, 0] },
            { "type": "script", "language": "javascript", "entry": "scripts/main.js",
              "uuid": "8bb9a1e4-0531-4b93-8ade-3880bfe1e2fc", "version": [1, 0, 0] }
        ],
        "dependencies": [ { "module_name": "@minecraft/server", "version": "2.10.0-beta" } ]
    }"#;

    const RP: &str = r#"{
        "format_version": 2,
        "header": {
            "name": "Construct [RP] v1.2.0",
            "uuid": "375ec465-3dc1-429f-8b4c-a337889e1ed4",
            "version": [1, 2, 0],
            "min_engine_version": [1, 26, 40]
        },
        "modules": [ { "type": "resources", "uuid": "7f6b23df-a583-476b-b0e4-87457e65f7c0", "version": [1, 0, 0] } ],
        "capabilities": [ "pbr" ]
    }"#;

    #[test]
    fn reads_the_real_behaviour_pack_manifest() {
        let m = parse(BP, Path::new("manifest.json")).unwrap();
        assert_eq!(m.uuid, "8c0c0153-d8b9-482a-889f-aef922b8fe58");
        assert_eq!(m.version, [1, 2, 0]);
        assert_eq!(m.kind, PackKind::Behavior);
        assert_eq!(m.name, "Construct [BP] v1.2.0");
    }

    #[test]
    fn a_module_of_type_resources_makes_it_a_resource_pack() {
        assert_eq!(parse(RP, Path::new("m.json")).unwrap().kind, PackKind::Resource);
    }

    #[test]
    fn a_dependency_module_name_does_not_break_parsing() {
        // Dependencies mix `{uuid, version:[..]}` with `{module_name, version:"2.10.0-beta"}`.
        // The struct ignores dependencies entirely; this asserts it stays ignored.
        assert!(parse(BP, Path::new("m.json")).is_ok());
    }

    #[test]
    fn a_missing_header_is_a_bad_pack_not_a_panic() {
        let err = parse(r#"{"format_version": 2}"#, Path::new("m.json")).unwrap_err();
        assert!(matches!(err, CoreError::BadPack { .. }), "got {err:?}");
    }

    #[test]
    fn malformed_json_names_the_file() {
        let err = parse("{ not json", Path::new("/packs/Thing/manifest.json")).unwrap_err();
        let CoreError::BadPack { path, .. } = err else { panic!("expected BadPack") };
        assert_eq!(path, Path::new("/packs/Thing/manifest.json"));
    }

    #[test]
    fn a_two_part_version_is_rejected_rather_than_padded() {
        let text = r#"{"header":{"name":"x","uuid":"u","version":[1,2]},"modules":[{"type":"data"}]}"#;
        assert!(parse(text, Path::new("m.json")).is_err());
    }

    #[test]
    fn version_string_is_dotted() {
        assert_eq!(version_string([1, 2, 0]), "1.2.0");
    }
}
```

- [ ] **Step 2: Run it and watch it fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p construct-core pack::manifest
```

Expected: the module does not exist yet.

- [ ] **Step 3: Add the error variant**

In `error.rs`:

```rust
    #[error("not a usable pack at {}: {reason}", path.display())]
    BadPack { path: PathBuf, reason: String },
```

- [ ] **Step 4: Write the parser**

At the top of `manifest.rs`:

```rust
//! `manifest.json`, read with a small local serde struct.
//!
//! Deliberately not `bedrock_addon`: the fields that matter here are the header
//! UUID, the version, and whether any module is `resources`. Twenty lines of
//! serde keeps the unpublished git-dependency surface down to `bedrock_level`.

use crate::error::{CoreError, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackKind {
    Behavior,
    Resource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub name: String,
    pub uuid: String,
    pub version: [u32; 3],
    pub kind: PackKind,
}

#[derive(Deserialize)]
struct Raw {
    header: Header,
    #[serde(default)]
    modules: Vec<Module>,
}

#[derive(Deserialize)]
struct Header {
    #[serde(default)]
    name: String,
    uuid: String,
    version: Vec<u32>,
}

#[derive(Deserialize)]
struct Module {
    #[serde(default)]
    r#type: String,
}

/// `[1, 2, 0]` as `"1.2.0"`.
pub fn version_string(v: [u32; 3]) -> String {
    format!("{}.{}.{}", v[0], v[1], v[2])
}

pub fn parse(text: &str, path: &Path) -> Result<Manifest> {
    let bad = |reason: String| CoreError::BadPack {
        path: path.to_path_buf(),
        reason,
    };
    let raw: Raw = serde_json::from_str(text).map_err(|e| bad(e.to_string()))?;
    let version: [u32; 3] = raw
        .header
        .version
        .as_slice()
        .try_into()
        .map_err(|_| bad(format!("header version is not three numbers: {:?}", raw.header.version)))?;

    // A resource pack is the one carrying a `resources` module. Behaviour packs
    // carry `data` and `script`; this is the only field that tells them apart.
    let kind = if raw.modules.iter().any(|m| m.r#type == "resources") {
        PackKind::Resource
    } else {
        PackKind::Behavior
    };

    Ok(Manifest { name: raw.header.name, uuid: raw.header.uuid, version, kind })
}

/// Reads `pack_dir/manifest.json`.
pub fn read(pack_dir: &Path) -> Result<Manifest> {
    let path: PathBuf = pack_dir.join("manifest.json");
    let text = std::fs::read_to_string(&path).map_err(|e| CoreError::BadPack {
        path: path.clone(),
        reason: e.to_string(),
    })?;
    parse(&text, &path)
}
```

Create `crates/construct-core/src/pack/mod.rs` with `pub mod manifest;` and register
`pub mod pack;` in `lib.rs`.

- [ ] **Step 5: Run the tests**

```bash
cargo test -p construct-core pack::manifest
```

Expected: PASS, 7 tests.

- [ ] **Step 6: Commit**

```bash
git add crates/construct-core/src
git commit -m "Read a pack manifest"
```

---

### Task 3: Finding Construct on disk

**Files:**
- Modify: `crates/construct-core/src/pack/mod.rs`
- Create: `crates/construct-core/tests/pack.rs`
- Modify: `crates/construct-core/src/error.rs`

**Interfaces:**
- Consumes: `pack::manifest::{Manifest, PackKind, read}`, `discovery::{Installation, World}`.
- Produces:
  - `pack::{CONSTRUCT_BP_UUID, CONSTRUCT_RP_UUID}` — the two `&'static str` UUIDs.
  - `pack::Pack { dir: PathBuf, manifest: Manifest }`
  - `pack::packs_in(root: &Path) -> Vec<Pack>` — every readable pack directly under `root`; unreadable ones are skipped.
  - `pack::find_by_uuid(root: &Path, uuid: &str) -> Option<Pack>`
  - `pack::behavior_root(dev_pack_root: &Path) -> PathBuf` — `<root>/development_behavior_packs`
  - `pack::resource_root(dev_pack_root: &Path) -> PathBuf` — `<root>/development_resource_packs`
  - `pack::world_behavior_root(world: &World) -> PathBuf` — `<world>/behavior_packs`
  - `CoreError::ConstructNotInstalled { searched: Vec<PathBuf> }`

Matching is **by header UUID, never by folder name** (§10 step 5). On the development machine
the folder happens to be `Construct[BP]`, but a user who renamed it must still be found — and a
second copy under a different name must not be installed alongside the first.

- [ ] **Step 1: Write the failing integration test**

Create `crates/construct-core/tests/pack.rs`:

```rust
use construct_core::pack;
use std::path::Path;

/// Writes a minimal pack directory and returns its path.
fn make_pack(root: &Path, folder: &str, uuid: &str, version: [u32; 3], resources: bool) -> std::path::PathBuf {
    let dir = root.join(folder);
    std::fs::create_dir_all(&dir).unwrap();
    let module = if resources { "resources" } else { "data" };
    let manifest = format!(
        r#"{{"format_version":2,
            "header":{{"name":"{folder}","uuid":"{uuid}","version":[{},{},{}]}},
            "modules":[{{"type":"{module}","uuid":"11111111-1111-1111-1111-111111111111","version":[1,0,0]}}]}}"#,
        version[0], version[1], version[2]
    );
    std::fs::write(dir.join("manifest.json"), manifest).unwrap();
    dir
}

#[test]
fn construct_is_found_by_uuid_under_any_folder_name() {
    let root = tempfile::tempdir().unwrap();
    make_pack(root.path(), "SomethingElse", pack::CONSTRUCT_BP_UUID, [1, 2, 0], false);
    make_pack(root.path(), "Canopy[BP]", "aaaaaaaa-0000-0000-0000-000000000000", [1, 0, 0], false);

    let found = pack::find_by_uuid(root.path(), pack::CONSTRUCT_BP_UUID).expect("should find Construct");
    assert_eq!(found.dir.file_name().unwrap(), "SomethingElse");
    assert_eq!(found.manifest.version, [1, 2, 0]);
}

#[test]
fn a_directory_without_a_manifest_is_skipped_not_an_error() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("junk")).unwrap();
    std::fs::write(root.path().join("loose-file.txt"), "x").unwrap();
    make_pack(root.path(), "Real", pack::CONSTRUCT_BP_UUID, [1, 0, 0], false);

    let packs = pack::packs_in(root.path());
    assert_eq!(packs.len(), 1);
    assert_eq!(packs[0].manifest.uuid, pack::CONSTRUCT_BP_UUID);
}

#[test]
fn a_pack_with_a_broken_manifest_is_skipped_rather_than_failing_the_scan() {
    // One corrupt pack must not hide every other pack on the machine.
    let root = tempfile::tempdir().unwrap();
    let broken = root.path().join("Broken");
    std::fs::create_dir_all(&broken).unwrap();
    std::fs::write(broken.join("manifest.json"), "{ not json").unwrap();
    make_pack(root.path(), "Good", pack::CONSTRUCT_BP_UUID, [1, 0, 0], false);

    assert_eq!(pack::packs_in(root.path()).len(), 1);
}

#[test]
fn a_missing_root_yields_no_packs() {
    assert!(pack::packs_in(Path::new("/no/such/root")).is_empty());
}

#[test]
fn the_pack_roots_are_the_documented_folder_names() {
    let base = Path::new("/com.mojang");
    assert_eq!(pack::behavior_root(base), Path::new("/com.mojang/development_behavior_packs"));
    assert_eq!(pack::resource_root(base), Path::new("/com.mojang/development_resource_packs"));
}
```

- [ ] **Step 2: Run it and watch it fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p construct-core --test pack
```

- [ ] **Step 3: Implement**

In `pack/mod.rs`:

```rust
//! Construct on disk: where its packs live and what is inside their `structures/`.

pub mod manifest;
pub mod structures;

use crate::discovery::World;
use manifest::Manifest;
use std::path::{Path, PathBuf};

/// Construct's behaviour-pack header UUID. Packs are matched on this, never on
/// folder name — a renamed folder is still the same pack, and two folders
/// carrying this UUID are two copies of one pack.
pub const CONSTRUCT_BP_UUID: &str = "8c0c0153-d8b9-482a-889f-aef922b8fe58";
/// Construct's resource-pack header UUID.
pub const CONSTRUCT_RP_UUID: &str = "375ec465-3dc1-429f-8b4c-a337889e1ed4";

#[derive(Debug, Clone)]
pub struct Pack {
    pub dir: PathBuf,
    pub manifest: Manifest,
}

pub fn behavior_root(dev_pack_root: &Path) -> PathBuf {
    dev_pack_root.join("development_behavior_packs")
}

pub fn resource_root(dev_pack_root: &Path) -> PathBuf {
    dev_pack_root.join("development_resource_packs")
}

/// A world's own copy of its packs, which takes precedence over the shared root.
pub fn world_behavior_root(world: &World) -> PathBuf {
    world.path.join("behavior_packs")
}

/// Every readable pack directly under `root`.
///
/// A directory with no manifest, or one that will not parse, is skipped: a
/// single corrupt pack must not hide every other pack on the machine.
pub fn packs_in(root: &Path) -> Vec<Pack> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out: Vec<Pack> = entries
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| {
            let dir = e.path();
            manifest::read(&dir).ok().map(|manifest| Pack { dir, manifest })
        })
        .collect();
    out.sort_by(|a, b| a.dir.cmp(&b.dir));
    out
}

pub fn find_by_uuid(root: &Path, uuid: &str) -> Option<Pack> {
    packs_in(root).into_iter().find(|p| p.manifest.uuid == uuid)
}
```

Add to `error.rs`:

```rust
    #[error("Construct is not installed")]
    ConstructNotInstalled { searched: Vec<PathBuf> },
```

- [ ] **Step 4: Run the tests**

```bash
cargo test -p construct-core --test pack
```

Expected: PASS, 5 tests. `structures` does not exist yet — add `pub mod structures;` in Task 5,
not now, or the module will not compile.

- [ ] **Step 5: Commit**

```bash
git add crates/construct-core
git commit -m "Find Construct by its header UUID"
```

---

### Task 4: Choosing an installation

`install`, `status`, `import`, and `copy` all have to answer "which Minecraft?" when no world
names one. §10 fixes the rule: `default_installation` from config, failing that the sole
installation if only one exists, failing that an error listing the candidates. Same never-guess
rule as world references.

**Files:**
- Create: `crates/construct-core/src/discovery/installation.rs`
- Modify: `crates/construct-core/src/discovery/mod.rs`
- Modify: `crates/construct-core/src/error.rs`

**Interfaces:**
- Consumes: `discovery::Installation`, `config::Config`.
- Produces:
  - `discovery::installation::choose<'a>(installations: &'a [Installation], requested: Option<&str>, default: Option<&str>) -> Result<&'a Installation>`
  - `discovery::installation::for_world<'a>(installations: &'a [Installation], world: &World) -> Result<&'a Installation>`
  - `CoreError::InstallationNotFound { name: String, available: Vec<String> }`

`requested` is the resolved CLI-flag-or-environment value (`CONSTRUCT_INSTALLATION`), `default`
is `config.default_installation`. Precedence is the spec's: flag → environment → config →
auto-discovery, and the caller has already collapsed the first two into `requested`.

- [ ] **Step 1: Write the failing tests**

Create `crates/construct-core/src/discovery/installation.rs` with its tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn inst(name: &str) -> Installation {
        Installation {
            name: name.to_string(),
            dev_pack_root: PathBuf::from(format!("/{name}")),
            world_roots: Vec::new(),
        }
    }

    #[test]
    fn a_sole_installation_needs_no_configuration() {
        let all = vec![inst("mcpelauncher")];
        assert_eq!(choose(&all, None, None).unwrap().name, "mcpelauncher");
    }

    #[test]
    fn two_installations_and_no_default_is_ambiguous_not_a_guess() {
        let all = vec![inst("release"), inst("preview")];
        let CoreError::AmbiguousInstallation { candidates } = choose(&all, None, None).unwrap_err()
        else {
            panic!("expected AmbiguousInstallation");
        };
        assert_eq!(candidates, vec!["release".to_string(), "preview".to_string()]);
    }

    #[test]
    fn the_configured_default_settles_it() {
        let all = vec![inst("release"), inst("preview")];
        assert_eq!(choose(&all, None, Some("preview")).unwrap().name, "preview");
    }

    #[test]
    fn an_explicit_request_beats_the_configured_default() {
        let all = vec![inst("release"), inst("preview")];
        assert_eq!(choose(&all, Some("release"), Some("preview")).unwrap().name, "release");
    }

    #[test]
    fn a_request_naming_nothing_is_an_error_listing_what_exists() {
        let all = vec![inst("release")];
        let CoreError::InstallationNotFound { name, available } =
            choose(&all, Some("nope"), None).unwrap_err()
        else {
            panic!("expected InstallationNotFound");
        };
        assert_eq!(name, "nope");
        assert_eq!(available, vec!["release".to_string()]);
    }

    #[test]
    fn a_stale_default_pointing_at_nothing_is_an_error_not_a_silent_fallback() {
        // Falling back to the sole installation would quietly ignore what the
        // user configured, which is exactly the "never guess" case.
        let all = vec![inst("release")];
        assert!(matches!(
            choose(&all, None, Some("preview")),
            Err(CoreError::InstallationNotFound { .. })
        ));
    }

    #[test]
    fn no_installations_at_all_says_so() {
        assert!(matches!(choose(&[], None, None), Err(CoreError::NoInstallations { .. })));
    }
}
```

- [ ] **Step 2: Run and watch it fail**

```bash
cargo test -p construct-core installation::
```

- [ ] **Step 3: Implement**

```rust
//! Which Minecraft, when no world names one.
//!
//! §10's rule, and the same never-guess rule §6 applies to world references:
//! an explicit request, failing that the configured default, failing that the
//! sole installation, failing that an error listing the candidates.

use crate::discovery::{Installation, World};
use crate::error::{CoreError, Result};

pub fn choose<'a>(
    installations: &'a [Installation],
    requested: Option<&str>,
    default: Option<&str>,
) -> Result<&'a Installation> {
    if installations.is_empty() {
        return Err(CoreError::NoInstallations { probed: Vec::new() });
    }
    let names = || installations.iter().map(|i| i.name.clone()).collect::<Vec<_>>();

    // A name that was asked for and does not exist is an error even when there
    // is only one installation: silently using it would ignore the request.
    if let Some(name) = requested.or(default) {
        return installations
            .iter()
            .find(|i| i.name == name)
            .ok_or_else(|| CoreError::InstallationNotFound {
                name: name.to_string(),
                available: names(),
            });
    }
    match installations {
        [only] => Ok(only),
        _ => Err(CoreError::AmbiguousInstallation { candidates: names() }),
    }
}

/// The installation a world belongs to.
///
/// `install --world W` deploys into *that* installation's dev-pack root, so
/// `preview` and `release` are never mixed (§6).
pub fn for_world<'a>(installations: &'a [Installation], world: &World) -> Result<&'a Installation> {
    installations
        .iter()
        .find(|i| i.name == world.installation)
        .ok_or_else(|| CoreError::InstallationNotFound {
            name: world.installation.clone(),
            available: installations.iter().map(|i| i.name.clone()).collect(),
        })
}
```

Add to `error.rs`:

```rust
    #[error("no installation named {name}")]
    InstallationNotFound { name: String, available: Vec<String> },
```

Export from `discovery/mod.rs`: `pub mod installation;`.

- [ ] **Step 4: Map the new errors to exit codes and messages**

In `crates/construct-cli/src/main.rs`, `exit_code`: add `CoreError::InstallationNotFound { .. }`
and `CoreError::ConstructNotInstalled { .. }` to the `3` arm. In `report`:

```rust
        CoreError::InstallationNotFound { available, .. } => {
            eprintln!("\navailable:");
            for a in available {
                eprintln!("  {a}");
            }
            eprintln!("\nSet one in config.toml:\n  default_installation = \"<name>\"");
        }
        CoreError::AmbiguousInstallation { candidates } => {
            eprintln!("\ncandidates:");
            for c in candidates {
                eprintln!("  {c}");
            }
            eprintln!("\nSet one in config.toml:\n  default_installation = \"<name>\"");
        }
        CoreError::ConstructNotInstalled { searched } => {
            eprintln!("\nsearched:");
            for s in searched {
                eprintln!("  {}", s.display());
            }
            eprintln!("\nInstall it:\n  construct install");
        }
```

- [ ] **Step 5: Run everything and commit**

```bash
cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add crates
git commit -m "Choose an installation without guessing"
```

---

### Task 5: The `structures/` folder

**Files:**
- Create: `crates/construct-core/src/pack/structures.rs`
- Modify: `crates/construct-core/src/pack/mod.rs` (add `pub mod structures;`)
- Modify: `crates/construct-core/src/error.rs`

**Interfaces:**
- Consumes: `store::key::{DEFAULT_NAMESPACE, qualify, display_name}`, `CoreError`.
- Produces:
  - `pack::structures::PackStructure { id: String, name: String, path: PathBuf, size_bytes: u64 }`
  - `pack::structures::dir(pack_dir: &Path) -> PathBuf` — `<pack>/structures`
  - `pack::structures::list(pack_dir: &Path) -> Vec<PackStructure>`
  - `pack::structures::path_for(pack_dir: &Path, id: &str) -> Result<PathBuf>`
  - `pack::structures::write(pack_dir: &Path, id: &str, bytes: &[u8], force: bool) -> Result<PathBuf>`
  - `pack::structures::remove(path: &Path) -> Result<()>`
  - `pack::structures::derive_name(stem: &str) -> Result<String>`
  - `CoreError::BadStructureName { name: String, reason: String }`

**The naming rule (§17, Global Constraints).** A file directly in `structures/` is
`mystructure:<stem>`; a file in `structures/<dir>/` is `<dir lowercased>:<stem>`. Nothing
deeper is addressable in-game under either reading of §17, so files more than one level down
are not listed. Directory names are lowercased for the id because Minecraft namespaces are
lowercase while the one real example on disk is `structures/Understudy/players.mcstructure`,
implying `understudy:players`.

**Writing is restricted where reading is not.** `list` reports whatever is on disk. `write`
only ever creates `structures/<name>.mcstructure` or `structures/<ns>/<name>.mcstructure`
with both segments already validated by `derive_name`'s character set, so a crafted id can
never escape the pack directory. This is a security boundary: ids reach `write` from file
stems and `--name`.

- [ ] **Step 1: Write the failing tests**

Append to `crates/construct-core/tests/pack.rs`:

```rust
use construct_core::pack::structures;

fn touch(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn a_file_directly_in_structures_is_mystructure_namespaced() {
    let root = tempfile::tempdir().unwrap();
    let pack = root.path().join("Construct[BP]");
    touch(&pack.join("structures/bomber.mcstructure"), b"12345");

    let found = structures::list(&pack);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, "mystructure:bomber");
    assert_eq!(found[0].name, "bomber");
    assert_eq!(found[0].size_bytes, 5);
}

#[test]
fn a_subdirectory_supplies_the_namespace_lowercased() {
    let root = tempfile::tempdir().unwrap();
    let pack = root.path().join("Understudy");
    touch(&pack.join("structures/Understudy/players.mcstructure"), b"x");

    let found = structures::list(&pack);
    assert_eq!(found[0].id, "understudy:players");
    // A non-default namespace stays visible in the display name.
    assert_eq!(found[0].name, "understudy:players");
}

#[test]
fn non_mcstructure_files_and_deeper_nesting_are_not_listed() {
    let root = tempfile::tempdir().unwrap();
    let pack = root.path().join("P");
    touch(&pack.join("structures/readme.txt"), b"x");
    touch(&pack.join("structures/a/b/deep.mcstructure"), b"x");
    touch(&pack.join("structures/ok.mcstructure"), b"x");

    let found = structures::list(&pack);
    assert_eq!(found.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), vec!["mystructure:ok"]);
}

#[test]
fn a_pack_with_no_structures_folder_lists_nothing() {
    let root = tempfile::tempdir().unwrap();
    assert!(structures::list(&root.path().join("Empty")).is_empty());
}

#[test]
fn path_for_puts_the_default_namespace_flat_and_others_in_a_subdirectory() {
    let pack = Path::new("/p");
    assert_eq!(structures::path_for(pack, "house").unwrap(), Path::new("/p/structures/house.mcstructure"));
    assert_eq!(
        structures::path_for(pack, "mystructure:house").unwrap(),
        Path::new("/p/structures/house.mcstructure")
    );
    assert_eq!(
        structures::path_for(pack, "understudy:players").unwrap(),
        Path::new("/p/structures/understudy/players.mcstructure")
    );
}

#[test]
fn path_for_refuses_an_id_that_would_escape_the_pack() {
    for evil in ["../../etc/passwd", "a/b", "..", "ns:../x", "ns:", ":name", "C:\\x"] {
        assert!(structures::path_for(Path::new("/p"), evil).is_err(), "{evil} should be refused");
    }
}

#[test]
fn write_refuses_an_existing_file_unless_forced() {
    let root = tempfile::tempdir().unwrap();
    let pack = root.path().join("P");
    structures::write(&pack, "house", b"first", false).unwrap();

    let err = structures::write(&pack, "house", b"second", false).unwrap_err();
    assert!(matches!(err, construct_core::CoreError::TargetExists { .. }));
    // Untouched by the refusal.
    assert_eq!(std::fs::read(pack.join("structures/house.mcstructure")).unwrap(), b"first");

    structures::write(&pack, "house", b"second", true).unwrap();
    assert_eq!(std::fs::read(pack.join("structures/house.mcstructure")).unwrap(), b"second");
}

#[test]
fn write_creates_the_structures_folder_and_any_namespace_directory() {
    let root = tempfile::tempdir().unwrap();
    let pack = root.path().join("P");
    let at = structures::write(&pack, "understudy:players", b"x", false).unwrap();
    assert_eq!(at, pack.join("structures/understudy/players.mcstructure"));
    assert!(at.is_file());
}

#[test]
fn derive_name_lowercases_and_maps_spaces() {
    assert_eq!(structures::derive_name("My House").unwrap(), "my_house");
    assert_eq!(structures::derive_name("tower-2.v1_a").unwrap(), "tower-2.v1_a");
}

#[test]
fn derive_name_rejects_rather_than_mangles() {
    // A mangled name is one Construct will not list, so the user is told to
    // pass --name instead of being handed something silently different.
    for bad in ["café", "a/b", "what?", "", "  ", "..", "."] {
        assert!(structures::derive_name(bad).is_err(), "{bad:?} should be rejected");
    }
}
```

- [ ] **Step 2: Run and watch it fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p construct-core --test pack
```

- [ ] **Step 3: Add the error variant**

```rust
    #[error("unusable structure name {name:?}: {reason}")]
    BadStructureName { name: String, reason: String },
```

- [ ] **Step 4: Implement**

```rust
//! Construct's `structures/` folder: ordinary `.mcstructure` files, no database.
//!
//! A file directly in `structures/` is `mystructure:<stem>` — confirmed by
//! Construct's own source, which strips exactly that prefix from the ids the
//! game hands it. A file in `structures/<dir>/` is `<dir>:<stem>`, inferred from
//! the one shipped pack that uses the layout. Anything deeper is not addressable
//! in-game under either reading, so it is not listed.

use crate::error::{CoreError, Result};
use crate::store::key;
use std::path::{Path, PathBuf};

pub const EXTENSION: &str = "mcstructure";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackStructure {
    /// The qualified id, always carrying a namespace.
    pub id: String,
    /// What the user types and sees.
    pub name: String,
    pub path: PathBuf,
    pub size_bytes: u64,
}

pub fn dir(pack_dir: &Path) -> PathBuf {
    pack_dir.join("structures")
}

/// Every structure file in a pack, sorted by id.
pub fn list(pack_dir: &Path) -> Vec<PackStructure> {
    let root = dir(pack_dir);
    let mut out = Vec::new();
    collect(&root, None, &mut out);
    let Ok(entries) = std::fs::read_dir(&root) else {
        return out;
    };
    for e in entries.flatten() {
        if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            let ns = e.file_name().to_string_lossy().to_lowercase();
            collect(&e.path(), Some(&ns), &mut out);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn collect(dir: &Path, namespace: Option<&str>, out: &mut Vec<PackStructure>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) != Some(EXTENSION) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let id = match namespace {
            Some(ns) => format!("{ns}:{stem}"),
            None => key::qualify(stem),
        };
        let size_bytes = e.metadata().map(|m| m.len()).unwrap_or(0);
        out.push(PackStructure {
            name: key::display_name(&id).to_string(),
            id,
            path,
            size_bytes,
        });
    }
}

/// One segment of a structure id, validated for use as a path component.
///
/// This is a security boundary, not a nicety: ids arrive from file stems and
/// from `--name`, so a segment containing a separator or `..` would let a
/// crafted name write outside the pack.
fn safe_segment(segment: &str, whole: &str) -> Result<()> {
    let bad = |reason: &str| CoreError::BadStructureName {
        name: whole.to_string(),
        reason: reason.to_string(),
    };
    if segment.is_empty() {
        return Err(bad("an empty name or namespace"));
    }
    if segment == "." || segment == ".." {
        return Err(bad("a name that would point outside the pack"));
    }
    if let Some(c) = segment
        .chars()
        .find(|c| !matches!(c, 'a'..='z' | '0'..='9' | '_' | '.' | '-'))
    {
        return Err(bad(&format!(
            "{c:?} is not allowed; names may use a-z, 0-9, and _ . -"
        )));
    }
    Ok(())
}

/// Where a structure with this id belongs inside a pack.
pub fn path_for(pack_dir: &Path, id: &str) -> Result<PathBuf> {
    let qualified = key::qualify(id);
    let (namespace, name) = qualified.split_once(':').ok_or_else(|| CoreError::BadStructureName {
        name: id.to_string(),
        reason: "no namespace".to_string(),
    })?;
    if name.contains(':') {
        return Err(CoreError::BadStructureName {
            name: id.to_string(),
            reason: "more than one namespace separator".to_string(),
        });
    }
    safe_segment(namespace, id)?;
    safe_segment(name, id)?;

    let root = dir(pack_dir);
    let file = format!("{name}.{EXTENSION}");
    Ok(if namespace == key::DEFAULT_NAMESPACE {
        root.join(file)
    } else {
        root.join(namespace).join(file)
    })
}

/// Writes a structure into a pack, refusing an existing file unless forced.
pub fn write(pack_dir: &Path, id: &str, bytes: &[u8], force: bool) -> Result<PathBuf> {
    let path = path_for(pack_dir, id)?;
    if path.exists() && !force {
        return Err(CoreError::TargetExists { path });
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, bytes)?;
    Ok(path)
}

pub fn remove(path: &Path) -> Result<()> {
    std::fs::remove_file(path)?;
    Ok(())
}

/// A structure name from a file stem: lowercased, spaces to `_`.
///
/// Anything outside `[a-z0-9_.-]` is rejected rather than mangled — a mangled
/// name is one Construct will not list, so the user gets told to pass `--name`.
pub fn derive_name(stem: &str) -> Result<String> {
    let derived: String = stem.trim().to_lowercase().replace(' ', "_");
    safe_segment(&derived, stem)?;
    Ok(derived)
}
```

Note `list` calls `collect` on the root before reading directories; keep both, the first pass
handles flat files and the loop handles one level of namespace directories.

- [ ] **Step 5: Run the tests**

```bash
cargo test -p construct-core --test pack
```

Expected: PASS, 15 tests.

- [ ] **Step 6: Commit**

```bash
git add crates/construct-core
git commit -m "Read and write Construct's structures folder"
```

---

### Task 6: One catalog over two sources

**Files:**
- Modify: `crates/construct-core/src/catalog.rs`
- Modify: `crates/construct-core/src/pack/mod.rs`
- Modify: `crates/construct-core/tests/pack.rs`

**Interfaces:**
- Consumes: `pack::{Pack, find_by_uuid, behavior_root, world_behavior_root, CONSTRUCT_BP_UUID}`, `pack::structures::list`, `catalog::{Entry, Source, sort}`.
- Produces:
  - `pack::Scope { WorldLocal, Shared }`
  - `pack::Target { pack: Pack, scope: Scope, also_at: Option<PathBuf> }`
  - `pack::for_world(world: &World, installation: &Installation) -> Result<Target>`
  - `pack::for_installation(installation: &Installation) -> Result<Target>`
  - `catalog::from_pack(pack_dir: &Path) -> Vec<Entry>`
  - `catalog::unify(world: Vec<Entry>, pack: Vec<Entry>) -> Vec<Entry>`

§5's targeting rule: a world's own `behavior_packs/` copy of Construct wins over the
installation's shared `development_behavior_packs` copy, and the command says which it chose.
When both exist, `also_at` names the one not used — §11's "two Construct copies: state which
was chosen and why".

- [ ] **Step 1: Write the failing tests**

Append to `crates/construct-core/tests/pack.rs`:

```rust
use construct_core::catalog::{self, Source};

#[test]
fn pack_entries_carry_their_file_path() {
    let root = tempfile::tempdir().unwrap();
    let pack = root.path().join("Construct[BP]");
    touch(&pack.join("structures/bomber.mcstructure"), b"12345");

    let entries = catalog::from_pack(&pack);
    assert_eq!(entries[0].source, Source::Pack);
    assert_eq!(entries[0].id, "mystructure:bomber");
    assert_eq!(entries[0].path.as_deref(), Some(pack.join("structures/bomber.mcstructure").as_path()));
}

#[test]
fn unify_interleaves_both_sources_by_name() {
    let world = vec![
        catalog::Entry { name: "house".into(), id: "mystructure:house".into(), source: Source::World, size_bytes: 1, path: None },
        catalog::Entry { name: "zebra".into(), id: "mystructure:zebra".into(), source: Source::World, size_bytes: 1, path: None },
    ];
    let pack = vec![catalog::Entry {
        name: "barn".into(), id: "mystructure:barn".into(), source: Source::Pack, size_bytes: 1,
        path: Some(std::path::PathBuf::from("/p/structures/barn.mcstructure")),
    }];
    let all = catalog::unify(world, pack);
    assert_eq!(all.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), vec!["barn", "house", "zebra"]);
}
```

And a target-resolution test that builds a world directory beside a shared root:

```rust
#[test]
fn a_worlds_own_copy_of_construct_wins_over_the_shared_one() {
    let base = tempfile::tempdir().unwrap();
    let com_mojang = base.path().join("com.mojang");
    let shared = com_mojang.join("development_behavior_packs");
    make_pack(&shared, "Construct[BP]", pack::CONSTRUCT_BP_UUID, [1, 2, 0], false);

    let world_dir = com_mojang.join("minecraftWorlds/Test");
    std::fs::create_dir_all(world_dir.join("db")).unwrap();
    make_pack(&world_dir.join("behavior_packs"), "Construct[BP]", pack::CONSTRUCT_BP_UUID, [1, 1, 0], false);

    let world = test_world(&world_dir);
    let installation = test_installation(&com_mojang);

    let target = pack::for_world(&world, &installation).unwrap();
    assert_eq!(target.scope, pack::Scope::WorldLocal);
    assert_eq!(target.pack.manifest.version, [1, 1, 0]);
    assert_eq!(target.also_at, Some(shared.join("Construct[BP]")));
}

#[test]
fn without_a_local_copy_the_shared_installation_pack_is_used() {
    let base = tempfile::tempdir().unwrap();
    let com_mojang = base.path().join("com.mojang");
    make_pack(&com_mojang.join("development_behavior_packs"), "Construct[BP]", pack::CONSTRUCT_BP_UUID, [1, 2, 0], false);
    let world_dir = com_mojang.join("minecraftWorlds/Test");
    std::fs::create_dir_all(&world_dir).unwrap();

    let target = pack::for_world(&test_world(&world_dir), &test_installation(&com_mojang)).unwrap();
    assert_eq!(target.scope, pack::Scope::Shared);
    assert_eq!(target.also_at, None);
}

#[test]
fn no_construct_anywhere_says_where_it_looked() {
    let base = tempfile::tempdir().unwrap();
    let com_mojang = base.path().join("com.mojang");
    let world_dir = com_mojang.join("minecraftWorlds/Test");
    std::fs::create_dir_all(&world_dir).unwrap();

    let err = pack::for_world(&test_world(&world_dir), &test_installation(&com_mojang)).unwrap_err();
    let construct_core::CoreError::ConstructNotInstalled { searched } = err else {
        panic!("expected ConstructNotInstalled");
    };
    assert_eq!(searched.len(), 2, "both the world copy and the shared root: {searched:?}");
}
```

Add the two helpers at the top of the test file:

```rust
use construct_core::discovery::{Installation, LastPlayedSource, World};

fn test_world(dir: &Path) -> World {
    World {
        installation: "test".into(),
        account: None,
        folder: dir.file_name().unwrap().to_string_lossy().into_owned(),
        display_name: "Test".into(),
        path: dir.to_path_buf(),
        last_played: None,
        last_played_source: LastPlayedSource::Mtime,
        size_bytes: 0,
    }
}

fn test_installation(com_mojang: &Path) -> Installation {
    Installation { name: "test".into(), dev_pack_root: com_mojang.to_path_buf(), world_roots: Vec::new() }
}
```

If `World`'s or `Installation`'s field list has drifted, fix the helper — the struct is the
authority, not this snippet.

- [ ] **Step 2: Run and watch it fail**

```bash
cargo test -p construct-core --test pack
```

- [ ] **Step 3: Implement the catalog half**

In `catalog.rs`:

```rust
/// Every structure file in a pack.
pub fn from_pack(pack_dir: &Path) -> Vec<Entry> {
    crate::pack::structures::list(pack_dir)
        .into_iter()
        .map(|s| Entry {
            name: s.name,
            id: s.id,
            source: Source::Pack,
            size_bytes: s.size_bytes,
            path: Some(s.path),
        })
        .collect()
}

/// The single list Construct presents in-game, over both sources.
///
/// Construct's own list lets a pack structure shadow a world structure of the
/// same name. This does not: §5 refuses an ambiguous name rather than picking a
/// winner, so both entries survive here and `resolve` reports the collision.
pub fn unify(world: Vec<Entry>, pack: Vec<Entry>) -> Vec<Entry> {
    let mut all = world;
    all.extend(pack);
    sort(&mut all);
    all
}
```

- [ ] **Step 4: Implement target resolution**

In `pack/mod.rs`:

```rust
use crate::discovery::Installation;
use crate::error::{CoreError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The world's own `behavior_packs/` copy.
    WorldLocal,
    /// The installation's shared `development_behavior_packs` copy.
    Shared,
}

#[derive(Debug, Clone)]
pub struct Target {
    pub pack: Pack,
    pub scope: Scope,
    /// The copy that was found and *not* used, if there was one. §11 asks the
    /// command to state which of two copies it chose.
    pub also_at: Option<PathBuf>,
}

/// The Construct copy that governs a world: its own, else the installation's.
pub fn for_world(world: &World, installation: &Installation) -> Result<Target> {
    let local_root = world_behavior_root(world);
    let shared_root = behavior_root(&installation.dev_pack_root);
    let local = find_by_uuid(&local_root, CONSTRUCT_BP_UUID);
    let shared = find_by_uuid(&shared_root, CONSTRUCT_BP_UUID);

    match (local, shared) {
        (Some(pack), other) => Ok(Target {
            pack,
            scope: Scope::WorldLocal,
            also_at: other.map(|p| p.dir),
        }),
        (None, Some(pack)) => Ok(Target { pack, scope: Scope::Shared, also_at: None }),
        (None, None) => Err(CoreError::ConstructNotInstalled {
            searched: vec![local_root, shared_root],
        }),
    }
}

/// The Construct copy in an installation's shared root.
pub fn for_installation(installation: &Installation) -> Result<Target> {
    let root = behavior_root(&installation.dev_pack_root);
    find_by_uuid(&root, CONSTRUCT_BP_UUID)
        .map(|pack| Target { pack, scope: Scope::Shared, also_at: None })
        .ok_or(CoreError::ConstructNotInstalled { searched: vec![root] })
}
```

- [ ] **Step 5: Run and commit**

```bash
cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add crates/construct-core
git commit -m "Unify world and pack structures into one catalog"
```

---

### Task 7: `list` shows both sources

**Files:**
- Modify: `crates/construct-cli/src/commands/list.rs`
- Modify: `crates/construct-cli/src/main.rs`
- Modify: `crates/construct-cli/tests/cli.rs`

**Interfaces:**
- Consumes: `catalog::{from_world, from_pack, unify}`, `pack::{for_world, Scope}`, `discovery::installation::for_world`, `store::open_world_store`.
- Produces: `commands::list::run(world: &World, installations: &[Installation], source: Option<Source>, out: &mut Out) -> Result<()>` — the added `installations` parameter is how the command finds the world's Construct.

Two behaviours worth stating, because both are easy to get wrong:

- **`--source pack` must not snapshot the world.** Copying gigabytes of `db/` to list files that
  are not in it would be absurd. `--source world` likewise skips the pack scan.
- **A missing Construct is not a failure of plain `list`.** It warns and shows world structures.
  Only an explicit `--source pack` turns it into an error, because then the user asked for
  exactly the thing that is not there.

- [ ] **Step 1: Write the failing CLI tests**

Append to `crates/construct-cli/tests/cli.rs`:

```rust
/// A com.mojang tree with one world and, optionally, Construct installed.
fn world_with_construct(structures: &[(&str, &[u8])]) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let world = root.path().join("minecraftWorlds/Test");
    std::fs::create_dir_all(&world).unwrap();
    std::fs::write(world.join("levelname.txt"), "Test").unwrap();
    std::fs::create_dir_all(world.join("db")).unwrap();

    let bp = root.path().join("development_behavior_packs/Construct[BP]");
    std::fs::create_dir_all(&bp).unwrap();
    std::fs::write(
        bp.join("manifest.json"),
        r#"{"format_version":2,
            "header":{"name":"Construct [BP] v1.2.0","uuid":"8c0c0153-d8b9-482a-889f-aef922b8fe58","version":[1,2,0]},
            "modules":[{"type":"data","uuid":"f4d52ae1-2c26-4938-b8c2-7e455d495620","version":[1,0,0]}]}"#,
    )
    .unwrap();
    for (name, bytes) in structures {
        let p = bp.join("structures").join(format!("{name}.mcstructure"));
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }
    root
}

#[test]
fn list_shows_pack_structures_without_touching_the_world_database() {
    let root = world_with_construct(&[("bomber", b"12345")]);
    let out = bin()
        .args(["list", "Test", "--source", "pack", "--json", "--com-mojang", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["structures"][0]["name"], "bomber");
    assert_eq!(v["structures"][0]["source"], "pack");
    assert_eq!(v["structures"][0]["size_bytes"], 5);
}

#[test]
fn source_pack_on_a_machine_without_construct_is_not_found() {
    let root = tempfile::tempdir().unwrap();
    let world = root.path().join("minecraftWorlds/Test");
    std::fs::create_dir_all(world.join("db")).unwrap();
    let out = bin()
        .args(["list", "Test", "--source", "pack", "--com-mojang", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&out.stderr).contains("construct install"));
}
```

- [ ] **Step 2: Run and watch it fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p construct-cli --test cli list
```

- [ ] **Step 3: Rewrite the command**

Replace the body of `commands/list.rs`'s `run`:

```rust
pub fn run(
    world: &World,
    installations: &[Installation],
    source: Option<Source>,
    out: &mut Out,
) -> Result<()> {
    // `--source pack` never opens the world: copying gigabytes of db/ to list
    // files that are not in it would be indefensible.
    let world_entries = if source == Some(Source::Pack) {
        Vec::new()
    } else {
        let store = store::open_world_store(world)?;
        if let Some(bytes) = store.via_snapshot {
            out.warn(format!("reading from a {} snapshot", human_size(bytes)));
        }
        catalog::from_world(&store)?
    };

    let pack_entries = if source == Some(Source::World) {
        Vec::new()
    } else {
        match installation::for_world(installations, world).and_then(|i| pack::for_world(world, i)) {
            Ok(target) => {
                if let Some(other) = &target.also_at {
                    out.warn(format!(
                        "two copies of Construct; using {} (the world's own), not {}",
                        target.pack.dir.display(),
                        other.display()
                    ));
                }
                catalog::from_pack(&target.pack.dir)
            }
            // Asking for pack structures on a machine with no Construct is an
            // error; a plain `list` just says so and shows the world.
            Err(e) if source == Some(Source::Pack) => return Err(e),
            Err(_) => {
                out.warn("Construct is not installed; showing world structures only");
                Vec::new()
            }
        }
    };

    let entries = catalog::unify(world_entries, pack_entries);
    // ... the existing human table and `out.emit` follow, unchanged.
}
```

Update `main.rs`'s `Command::List` arm to pass `&installations`.

- [ ] **Step 4: Run the suite**

```bash
cargo test --workspace
```

Expected: PASS. The stage-1 `list` tests still pass — a world with no Construct now emits one
extra stderr warning, which those tests do not assert against.

- [ ] **Step 5: Commit**

```bash
git add crates/construct-cli
git commit -m "list both sources"
```

---

### Task 8: `import`

**Files:**
- Create: `crates/construct-cli/src/commands/import.rs`
- Modify: `crates/construct-cli/src/commands/mod.rs`, `src/cli.rs`, `src/main.rs`
- Modify: `crates/construct-cli/tests/cli.rs`

**Interfaces:**
- Consumes: `pack::{for_world, for_installation, Scope}`, `pack::structures::{derive_name, write}`, `discovery::installation::{choose, for_world}`, `config::Config`.
- Produces: `commands::import::run(file: &Path, world: Option<&World>, installation: &Installation, name: Option<&str>, force: bool, out: &mut Out) -> Result<()>`

CLI surface (§5): `construct import <file> [--world W] [--name N]`. The derived name is always
printed. The command states which Construct copy it wrote into, and that the world must be
reloaded before Construct sees the structure.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn import_derives_a_name_from_the_file_stem_and_reports_it() {
    let root = world_with_construct(&[]);
    let src = root.path().join("My House.mcstructure");
    std::fs::write(&src, b"structure-bytes").unwrap();

    let out = bin()
        .args(["import", src.to_str().unwrap(), "--world", "Test", "--json",
               "--com-mojang", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));

    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["name"], "my_house");
    assert_eq!(v["id"], "mystructure:my_house");
    let written = root.path().join("development_behavior_packs/Construct[BP]/structures/my_house.mcstructure");
    assert_eq!(std::fs::read(&written).unwrap(), b"structure-bytes");
}

#[test]
fn import_refuses_an_unusable_name_instead_of_mangling_it() {
    let root = world_with_construct(&[]);
    let src = root.path().join("café.mcstructure");
    std::fs::write(&src, b"x").unwrap();

    let out = bin()
        .args(["import", src.to_str().unwrap(), "--world", "Test",
               "--com-mojang", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--name"), "should point at --name:\n{stderr}");
}

#[test]
fn import_refuses_to_overwrite_without_force() {
    let root = world_with_construct(&[("house", b"original")]);
    let src = root.path().join("house.mcstructure");
    std::fs::write(&src, b"replacement").unwrap();
    let args = ["import", src.to_str().unwrap(), "--world", "Test",
                "--com-mojang", root.path().to_str().unwrap()];

    let out = bin().args(args).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("--force"));
    let target = root.path().join("development_behavior_packs/Construct[BP]/structures/house.mcstructure");
    assert_eq!(std::fs::read(&target).unwrap(), b"original");

    let out = bin().args(args).arg("--force").output().unwrap();
    assert!(out.status.success());
    assert_eq!(std::fs::read(&target).unwrap(), b"replacement");
}

#[test]
fn import_says_the_world_must_be_reloaded() {
    let root = world_with_construct(&[]);
    let src = root.path().join("tower.mcstructure");
    std::fs::write(&src, b"x").unwrap();
    let out = bin()
        .args(["import", src.to_str().unwrap(), "--world", "Test",
               "--com-mojang", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.to_lowercase().contains("reload"), "stdout:\n{text}");
}

#[test]
fn import_without_construct_points_at_install() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("minecraftWorlds/Test/db")).unwrap();
    let src = root.path().join("x.mcstructure");
    std::fs::write(&src, b"x").unwrap();
    let out = bin()
        .args(["import", src.to_str().unwrap(), "--world", "Test",
               "--com-mojang", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&out.stderr).contains("construct install"));
}
```

- [ ] **Step 2: Run and watch it fail**

```bash
cargo test -p construct-cli --test cli import
```

- [ ] **Step 3: Add the subcommand**

In `cli.rs`:

```rust
    /// Copy a .mcstructure file into Construct's structures folder.
    Import {
        /// The .mcstructure file to import.
        file: PathBuf,
        /// Target this world's Construct copy.
        #[arg(long, value_name = "WORLD")]
        world: Option<String>,
        /// Override the name derived from the file stem.
        #[arg(long, value_name = "NAME")]
        name: Option<String>,
    },
```

- [ ] **Step 4: Write the command**

```rust
//! Copying a `.mcstructure` file into Construct's `structures/` folder.
//!
//! No database is involved in either direction: a structure's leveldb value is
//! byte-identical to a `.mcstructure` file, so this is a file copy with a name
//! derived under Construct's rules.

use crate::output::Out;
use construct_core::discovery::{Installation, World};
use construct_core::pack::{self, structures};
use construct_core::store::key;
use construct_core::{CoreError, Result};
use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
struct Payload {
    name: String,
    id: String,
    path: String,
    bytes: u64,
    pack: String,
    scope: &'static str,
}

pub fn run(
    file: &Path,
    world: Option<&World>,
    installation: &Installation,
    name: Option<&str>,
    force: bool,
    out: &mut Out,
) -> Result<()> {
    let bytes = std::fs::read(file)?;

    let id = match name {
        // An explicit --name is the user's own choice; it still has to be a
        // name Construct can address, so it goes through the same validation.
        Some(n) => key::qualify(n),
        None => {
            let stem = file
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| CoreError::BadStructureName {
                    name: file.display().to_string(),
                    reason: "the file has no usable stem".to_string(),
                })?;
            key::qualify(&structures::derive_name(stem)?)
        }
    };

    let target = match world {
        Some(w) => pack::for_world(w, installation)?,
        None => pack::for_installation(installation)?,
    };
    if let Some(other) = &target.also_at {
        out.warn(format!(
            "two copies of Construct; writing into {} (the world's own), not {}",
            target.pack.dir.display(),
            other.display()
        ));
    }
    // Construct's in-game list only shows `mystructure:` structures (§17), so a
    // namespaced name lands somewhere the addon will not display.
    if !id.starts_with(&format!("{}:", key::DEFAULT_NAMESPACE)) {
        out.warn(format!(
            "{id} is outside the mystructure namespace; Construct's in-game list will not show it"
        ));
    }

    let path = structures::write(&target.pack.dir, &id, &bytes, force)?;

    out.line(format!("imported {} as {}", file.display(), key::display_name(&id)));
    out.line(format!("  {}", path.display()));
    out.line("Reload the world before Construct sees it.");

    out.emit(Payload {
        name: key::display_name(&id).to_string(),
        id: id.clone(),
        path: path.display().to_string(),
        bytes: bytes.len() as u64,
        pack: target.pack.dir.display().to_string(),
        scope: match target.scope {
            pack::Scope::WorldLocal => "world",
            pack::Scope::Shared => "shared",
        },
    });
    Ok(())
}
```

- [ ] **Step 5: Wire it into `main.rs`**

The world is optional here, and the installation comes from the world when one is given:

```rust
        Command::Import { file, world, name } => {
            let w = world.as_deref().map(resolve_world).transpose()?;
            let installation = match &w {
                Some(w) => discovery::installation::for_world(&installations, w)?,
                None => discovery::installation::choose(
                    &installations,
                    std::env::var("CONSTRUCT_INSTALLATION").ok().as_deref(),
                    loaded.config.default_installation.as_deref(),
                )?,
            };
            commands::import::run(file, w.as_ref(), installation, name.as_deref(), cli.force, out)
        }
```

Add `CoreError::BadStructureName { .. }` to `exit_code`'s `2` arm, and to `report`:

```rust
        CoreError::BadStructureName { .. } => {
            eprintln!("\nChoose a name explicitly:\n  construct import <file> --name <name>");
        }
```

- [ ] **Step 6: Run and commit**

```bash
cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add crates/construct-cli
git commit -m "import a .mcstructure into Construct"
```

---

### Task 9: `copy`

**Files:**
- Create: `crates/construct-cli/src/commands/catalog.rs` (extracted from `list.rs`)
- Create: `crates/construct-cli/src/commands/copy.rs`
- Modify: `crates/construct-cli/src/commands/{mod.rs,list.rs}`, `src/cli.rs`, `src/main.rs`
- Modify: `crates/construct-core/src/catalog.rs`
- Modify: `crates/construct-cli/tests/cli.rs`

**Interfaces:**
- Consumes: everything from Tasks 6-8.
- Produces:
  - `catalog::read_entry(entry: &Entry, store: Option<&dyn StructureStore>) -> Result<Vec<u8>>` (core) — the bytes behind an entry, from the database or from the file.
  - `commands::catalog::for_world(world: &World, installations: &[Installation], source: Option<Source>, out: &mut Out) -> Result<Loaded>` (CLI) where `Loaded { entries: Vec<Entry>, store: Option<OpenedStore> }`.
  - `commands::copy::run(src: &World, structure: &str, dst: &World, installations: &[Installation], source: Option<Source>, force: bool, out: &mut Out) -> Result<()>`

`copy` reads from the source world's **unified** catalog — a structure already sitting in the
source world's Construct copy is as copyable as one in its database — and always writes into
the destination's Construct `structures/`, never into a leveldb. Source and destination
resolve independently, so cross-root copies work (§6).

This task also does the extraction the second caller earns: `list`'s "build a catalog for a
world" logic moves into `commands/catalog.rs` and both commands call it. `delete` (Task 10)
becomes the third caller.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn copy_moves_bytes_from_a_world_pack_into_another_worlds_pack() {
    // Two worlds under one root, so one --com-mojang covers both.
    let root = world_with_construct(&[("barn", b"barn-bytes")]);
    let other = root.path().join("minecraftWorlds/Other");
    std::fs::create_dir_all(other.join("db")).unwrap();
    std::fs::write(other.join("levelname.txt"), "Other").unwrap();

    let out = bin()
        .args(["copy", "Test", "barn", "Other", "--json",
               "--com-mojang", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));

    // Both worlds share the installation's Construct copy, so the destination
    // file is the same one — assert the JSON says what it did rather than
    // asserting a second file exists.
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["name"], "barn");
    assert_eq!(v["from"], "Test");
    assert_eq!(v["to"], "Other");
}

#[test]
fn copy_refuses_an_existing_target_unless_forced() {
    let root = world_with_construct(&[("barn", b"barn-bytes")]);
    let other = root.path().join("minecraftWorlds/Other");
    std::fs::create_dir_all(other.join("db")).unwrap();
    // Give Other its own Construct copy holding a different `barn`.
    let bp = other.join("behavior_packs/Construct[BP]");
    std::fs::create_dir_all(bp.join("structures")).unwrap();
    std::fs::copy(
        root.path().join("development_behavior_packs/Construct[BP]/manifest.json"),
        bp.join("manifest.json"),
    )
    .unwrap();
    std::fs::write(bp.join("structures/barn.mcstructure"), b"theirs").unwrap();

    let args = ["copy", "Test", "barn", "Other", "--com-mojang"];
    let out = bin().args(args).arg(root.path()).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(std::fs::read(bp.join("structures/barn.mcstructure")).unwrap(), b"theirs");

    let out = bin().args(args).arg(root.path()).arg("--force").output().unwrap();
    assert!(out.status.success());
    assert_eq!(std::fs::read(bp.join("structures/barn.mcstructure")).unwrap(), b"barn-bytes");
}

#[test]
fn copy_of_a_name_that_is_not_there_suggests_near_matches() {
    let root = world_with_construct(&[("barn", b"x")]);
    let other = root.path().join("minecraftWorlds/Other");
    std::fs::create_dir_all(other.join("db")).unwrap();
    let out = bin()
        .args(["copy", "Test", "bar", "Other", "--com-mojang", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&out.stderr).contains("barn"));
}
```

- [ ] **Step 2: Run and watch it fail**

```bash
cargo test -p construct-cli --test cli copy
```

- [ ] **Step 3: Add `read_entry` to core**

In `catalog.rs`:

```rust
/// The bytes behind an entry, wherever they live.
///
/// A world entry needs the store it came from; a pack entry is a file. Either
/// way the bytes are a complete `.mcstructure` — that is the byte-transparency
/// property this whole tool rests on.
pub fn read_entry(entry: &Entry, store: Option<&dyn StructureStore>) -> Result<Vec<u8>> {
    match (&entry.path, store) {
        (Some(path), _) => Ok(std::fs::read(path)?),
        (None, Some(store)) => store.get(&entry.id)?.ok_or_else(|| CoreError::StructureNotFound {
            name: entry.name.clone(),
            near: Vec::new(),
        }),
        (None, None) => Err(CoreError::StructureNotFound {
            name: entry.name.clone(),
            near: Vec::new(),
        }),
    }
}
```

- [ ] **Step 4: Extract the shared catalog loader**

Create `crates/construct-cli/src/commands/catalog.rs` holding exactly the logic Task 7 put in
`list.rs` — the `--source` short-circuits, the snapshot warning, the two-copies warning, and
the "Construct is not installed" handling:

```rust
//! Building a world's structure catalog, for every command that resolves a name.

pub struct Loaded {
    pub entries: Vec<Entry>,
    /// Held open so `catalog::read_entry` can fetch world-source bytes.
    pub store: Option<OpenedStore>,
}

pub fn for_world(
    world: &World,
    installations: &[Installation],
    source: Option<Source>,
    out: &mut Out,
) -> Result<Loaded> {
    // ...moved verbatim from list.rs, returning the store rather than dropping it
}
```

Then `list.rs` becomes a caller of it, and keeps only its formatting.

- [ ] **Step 5: Write `copy`**

```rust
pub fn run(
    src: &World,
    structure: &str,
    dst: &World,
    installations: &[Installation],
    source: Option<Source>,
    force: bool,
    out: &mut Out,
) -> Result<()> {
    let loaded = super::catalog::for_world(src, installations, source, out)?;
    let entry = catalog::resolve(structure, &loaded.entries, source)?;
    let bytes = catalog::read_entry(&entry, loaded.store.as_ref().map(|s| s as &dyn StructureStore))?;

    let installation = installation::for_world(installations, dst)?;
    let target = pack::for_world(dst, installation)?;
    if let Some(other) = &target.also_at {
        out.warn(format!(
            "two copies of Construct on {}; writing into {}, not {}",
            dst.display_name, target.pack.dir.display(), other.display()
        ));
    }
    let path = structures::write(&target.pack.dir, &entry.id, &bytes, force)?;

    out.line(format!(
        "copied {} from {} to {}",
        entry.name, src.display_name, dst.display_name
    ));
    out.line(format!("  {}", path.display()));
    out.line("Reload the world before Construct sees it.");

    out.emit(Payload {
        name: entry.name.clone(),
        id: entry.id.clone(),
        from: src.qualified(),
        to: dst.qualified(),
        path: path.display().to_string(),
        bytes: bytes.len() as u64,
    });
    Ok(())
}
```

Add the subcommand:

```rust
    /// Copy a structure into another world's Construct.
    Copy {
        /// Source world name, qualified reference, or path.
        src_world: String,
        /// Structure name.
        structure: String,
        /// Destination world name, qualified reference, or path.
        dst_world: String,
    },
```

- [ ] **Step 6: Run and commit**

```bash
cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add crates
git commit -m "copy a structure into another world's Construct"
```

---

### Task 10: `delete --source pack`

**Files:**
- Create: `crates/construct-cli/src/commands/delete.rs`
- Modify: `crates/construct-cli/src/commands/mod.rs`, `src/cli.rs`, `src/main.rs`
- Modify: `crates/construct-cli/tests/cli.rs`

**Interfaces:**
- Consumes: `commands::catalog::for_world`, `catalog::resolve`, `pack::structures::remove`.
- Produces: `commands::delete::run(world: &World, structure: &str, installations: &[Installation], source: Option<Source>, out: &mut Out) -> Result<()>`

**Deleting a pack structure is an `unlink` and carries none of §8's ceremony.** Deleting from a
world's database is the only leveldb write in the tool and arrives in stage 4 — until then,
resolving to a world structure is refused with an exit-2 usage error that names `--source pack`.
That refusal is the whole reason `delete` ships now: users get the capability long before the
risky path exists.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn delete_unlinks_a_pack_structure() {
    let root = world_with_construct(&[("bomber", b"x")]);
    let file = root.path().join("development_behavior_packs/Construct[BP]/structures/bomber.mcstructure");
    assert!(file.exists());

    let out = bin()
        .args(["delete", "Test", "bomber", "--source", "pack",
               "--com-mojang", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(!file.exists());
}

#[test]
fn deleting_from_a_world_database_is_refused_for_now() {
    // Stage 4 territory. The refusal must arrive before anything is touched.
    let root = world_with_construct(&[]);
    let out = bin()
        .args(["delete", "Test", "house", "--source", "world",
               "--com-mojang", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--source pack"), "stderr:\n{stderr}");
}
```

- [ ] **Step 2: Run and watch it fail**

```bash
cargo test -p construct-cli --test cli delete
```

- [ ] **Step 3: Implement**

```rust
pub fn run(
    world: &World,
    structure: &str,
    installations: &[Installation],
    source: Option<Source>,
    out: &mut Out,
) -> Result<()> {
    // Refuse before doing any work at all: this is the one command whose
    // world-source form can damage a save, and it does not exist yet.
    if source == Some(Source::World) {
        return Err(not_yet_implemented());
    }
    let loaded = super::catalog::for_world(world, installations, Some(Source::Pack), out)?;
    let entry = catalog::resolve(structure, &loaded.entries, Some(Source::Pack))?;

    let Some(path) = entry.path.clone() else {
        return Err(not_yet_implemented());
    };
    structures::remove(&path)?;

    out.line(format!("deleted {} from {}", entry.name, path.display()));
    out.line("Reload the world before Construct stops showing it.");
    out.emit(Payload { name: entry.name, id: entry.id, path: path.display().to_string() });
    Ok(())
}
```

`not_yet_implemented` lives in this module and is a `CoreError` the CLI already maps to exit 2:

```rust
/// Stage 4 owns the leveldb write path (§14). Refusing here is deliberate.
fn not_yet_implemented() -> CoreError {
    CoreError::BadStructureName {
        name: "--source world".to_string(),
        reason: "deleting a structure from a world's database is not implemented yet; \
                 use --source pack to delete an imported structure"
            .to_string(),
    }
}
```

**Ruling to check before implementing:** if a reviewer objects that `BadStructureName` is the
wrong shape for this, add a dedicated `CoreError::NotImplemented { what: String }` mapped to
exit 2 and use it here. Either is acceptable; a made-up error message inside a mismatched
variant is not.

Note the `--source pack` short-circuit in `commands::catalog::for_world` means `delete` never
snapshots the world's database — deleting a file must not copy gigabytes first.

- [ ] **Step 4: Run and commit**

```bash
cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add crates/construct-cli
git commit -m "delete an imported structure"
```

---

### Task 11: `backup`

§8: "`level.dat` writes (`install --world`, `experiment`) copy the file to the same backup
directory before modifying it. Minecraft's own `level.dat_old` is not a substitute — it is
overwritten by the game on its own schedule." Stage 2 needs the file half of `backup`; stage 4
adds the `db/` directory half on top of it.

**Files:**
- Create: `crates/construct-core/src/backup.rs`
- Modify: `crates/construct-core/src/lib.rs`

**Interfaces:**
- Consumes: `config::Backups { dir: Option<PathBuf>, keep: usize }`.
- Produces:
  - `backup::root(backups: &Backups) -> Result<PathBuf>` — the configured directory, else the platform data directory.
  - `backup::sanitize(reference: &str) -> String`
  - `backup::file(src: &Path, world_reference: &str, backups: &Backups) -> Result<PathBuf>` — copies, prunes, returns the backup's path.

Backups live **outside the world folder** — a `db.backup-*` inside a world would confuse
Minecraft, bloat the world, and ride along into any world export. Retention is keyed on the
path-sanitized qualified reference `<installation>/<account>/<folder>`, not the folder name
alone, which is not unique across roots.

- [ ] **Step 1: Write the failing tests**

In `backup.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Backups;

    fn backups(dir: &Path, keep: usize) -> Backups {
        Backups { dir: Some(dir.to_path_buf()), keep }
    }

    #[test]
    fn a_qualified_reference_becomes_one_safe_directory_name() {
        assert_eq!(sanitize("release/Shared/Ssu8ww1SFbM="), "release_Shared_Ssu8ww1SFbM_");
        assert_eq!(sanitize("mcpelauncher/Amelix CMP"), "mcpelauncher_Amelix_CMP");
        // Nothing that could climb out of the backup directory survives.
        assert_eq!(sanitize("../../etc"), "______etc");
    }

    #[test]
    fn a_backup_is_a_copy_outside_the_world() {
        let tmp = tempfile::tempdir().unwrap();
        let world = tmp.path().join("world");
        std::fs::create_dir_all(&world).unwrap();
        let level = world.join("level.dat");
        std::fs::write(&level, b"original").unwrap();

        let store = tmp.path().join("backups");
        let at = file(&level, "test/World", &backups(&store, 10)).unwrap();

        assert_eq!(std::fs::read(&at).unwrap(), b"original");
        assert!(at.starts_with(&store), "backup must live outside the world: {}", at.display());
        assert!(at.file_name().unwrap().to_string_lossy().starts_with("level.dat."));
    }

    #[test]
    fn retention_keeps_the_newest_and_drops_the_rest() {
        let tmp = tempfile::tempdir().unwrap();
        let level = tmp.path().join("level.dat");
        let store = tmp.path().join("backups");
        let cfg = backups(&store, 3);

        let mut made = Vec::new();
        for i in 0..5 {
            std::fs::write(&level, format!("v{i}")).unwrap();
            made.push(file(&level, "test/World", &cfg).unwrap());
        }
        let kept: Vec<_> = std::fs::read_dir(store.join("test_World")).unwrap().flatten().collect();
        assert_eq!(kept.len(), 3, "keep = 3");
        // The newest survives, whatever the clock did.
        assert!(made.last().unwrap().exists());
        assert!(!made[0].exists());
    }

    #[test]
    fn two_backups_in_the_same_second_do_not_collide() {
        let tmp = tempfile::tempdir().unwrap();
        let level = tmp.path().join("level.dat");
        std::fs::write(&level, b"x").unwrap();
        let cfg = backups(&tmp.path().join("b"), 10);

        let a = file(&level, "w", &cfg).unwrap();
        let b = file(&level, "w", &cfg).unwrap();
        assert_ne!(a, b);
        assert!(a.exists() && b.exists());
    }

    #[test]
    fn backups_of_different_worlds_do_not_share_retention() {
        let tmp = tempfile::tempdir().unwrap();
        let level = tmp.path().join("level.dat");
        std::fs::write(&level, b"x").unwrap();
        let cfg = backups(&tmp.path().join("b"), 1);

        // The folder name is the same in both; the qualified reference is not.
        let a = file(&level, "release/Survival", &cfg).unwrap();
        let b = file(&level, "preview/Survival", &cfg).unwrap();
        assert!(a.exists() && b.exists(), "one world's backup must not evict another's");
    }
}
```

- [ ] **Step 2: Run and watch it fail**

```bash
cargo test -p construct-core backup::
```

- [ ] **Step 3: Implement**

```rust
//! Copies taken before anything is modified.
//!
//! Backups live outside the world folder: a `db.backup-*` inside a world would
//! confuse Minecraft, bloat the world, and ride along into any world export.
//! Retention is keyed on the qualified reference rather than the folder name,
//! which is not unique across roots.

use crate::config::Backups;
use crate::error::{CoreError, Result};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The directory backups go in: configured, else the platform data directory.
pub fn root(backups: &Backups) -> Result<PathBuf> {
    if let Some(dir) = &backups.dir {
        return Ok(dir.clone());
    }
    directories::ProjectDirs::from("", "", "constructcli")
        .map(|d| d.data_dir().join("backups"))
        .ok_or_else(|| {
            CoreError::Io(std::io::Error::other(
                "no platform data directory for backups; set [backups] dir in config.toml",
            ))
        })
}

/// One directory name per world, from its qualified reference.
pub fn sanitize(reference: &str) -> String {
    reference
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
        .collect()
}

/// Copies `src` into the backup directory for `world_reference` and prunes.
pub fn file(src: &Path, world_reference: &str, backups: &Backups) -> Result<PathBuf> {
    let dir = root(backups)?.join(sanitize(world_reference));
    std::fs::create_dir_all(&dir)?;

    let name = src
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("backup")
        .to_string();
    // Epoch seconds rather than a formatted date: a date needs a dependency,
    // and the full path is printed to the user anyway.
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let mut at = dir.join(format!("{name}.{stamp}"));
    let mut n = 1;
    while at.exists() {
        at = dir.join(format!("{name}.{stamp}-{n}"));
        n += 1;
    }
    std::fs::copy(src, &at)?;
    prune(&dir, &name, backups.keep);
    Ok(at)
}

/// Keeps the newest `keep` backups of one file, by modification time.
///
/// Best-effort: a backup that cannot be removed is not worth failing a write
/// that already succeeded.
fn prune(dir: &Path, name: &str, keep: usize) {
    let prefix = format!("{name}.");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut found: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
        .filter_map(|e| {
            let modified = e.metadata().ok()?.modified().ok()?;
            Some((modified, e.path()))
        })
        .collect();
    if found.len() <= keep {
        return;
    }
    found.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, path) in found.into_iter().skip(keep) {
        let _ = std::fs::remove_file(path);
    }
}
```

If `retention_keeps_the_newest_and_drops_the_rest` proves flaky because five copies land in one
filesystem timestamp tick, sort by the numeric suffix parsed out of the filename instead of by
mtime — the suffix is monotonic by construction, including the `-n` disambiguator. Do not
"fix" it with a sleep.

- [ ] **Step 4: Run and commit**

```bash
cargo test -p construct-core backup:: && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add crates/construct-core
git commit -m "Back up a file before modifying it"
```

---

### Task 12: Writing `level.dat`

The dangerous one. Read §10's "Writing `level.dat` safely" before starting — it is short, it is
measured, and it explains why the fidelity gate exists.

**Files:**
- Modify: `crates/construct-core/src/leveldat.rs`

**Interfaces:**
- Consumes: `nbtx`, `CoreError`.
- Produces:
  - `LevelDat::is_faithful(&self) -> bool` — false when this file cannot be re-serialized without changing.
  - `LevelDat::beta_apis(&self) -> Option<bool>`
  - `LevelDat::set_beta_apis(&mut self, on: bool)`
  - `LevelDat::to_bytes(&self) -> Result<Vec<u8>>` — header plus payload; refuses when not faithful.
  - `leveldat::write(dat: &LevelDat, path: &Path) -> Result<()>` — temp file in the same directory, then rename.
  - `leveldat::apply_beta_apis(path: &Path, on: bool) -> Result<BetaApisChange>` where `BetaApisChange { before: Option<bool>, after: bool, changed: bool }` — reads, mutates, writes, re-reads, and verifies.
  - `CoreError::UnwritableLevelDat { path: PathBuf, reason: String }`

**The three flags (§10), measured from a real world with Beta APIs on:**

```
Compound experiments
  Byte experiments_ever_used            = 1
  Byte gametest                         = 1
  Byte saved_with_toggled_experiments   = 1
```

Enabling sets all three to `1`, creating the `experiments` compound if absent. Disabling sets
`gametest` to `0` and **leaves both companion flags at `1`** — they record that the world once
used experiments, and clearing them would misrepresent the save. The compound is
read-modify-written, never replaced: worlds carry other experiment keys, and a wholesale
rewrite would silently disable them.

- [ ] **Step 1: Write the failing tests**

Append to `leveldat.rs`'s `mod tests` (the existing `build` helper constructs a `level.dat`):

```rust
    fn experiments(entries: &[(&str, i8)]) -> nbtx::Value {
        let mut inner = HashMap::new();
        for (k, v) in entries {
            inner.insert(k.to_string(), nbtx::Value::Byte(*v));
        }
        let mut root = HashMap::new();
        root.insert("experiments".to_string(), nbtx::Value::Compound(inner));
        root.insert("LevelName".to_string(), nbtx::Value::String("Test".into()));
        nbtx::Value::Compound(root)
    }

    #[test]
    fn enabling_creates_the_compound_when_a_world_has_none() {
        let mut root = HashMap::new();
        root.insert("LevelName".to_string(), nbtx::Value::String("Test".into()));
        let bytes = build(10, nbtx::Value::Compound(root));
        let mut dat = parse(&bytes, Path::new("level.dat")).unwrap();

        assert_eq!(dat.beta_apis(), None);
        dat.set_beta_apis(true);
        assert_eq!(dat.experiments().unwrap().get("gametest"), Some(&1));
        assert_eq!(dat.experiments().unwrap().get("experiments_ever_used"), Some(&1));
        assert_eq!(dat.experiments().unwrap().get("saved_with_toggled_experiments"), Some(&1));
    }

    #[test]
    fn enabling_a_world_that_already_has_it_changes_nothing() {
        let bytes = build(10, experiments(&[
            ("experiments_ever_used", 1), ("gametest", 1), ("saved_with_toggled_experiments", 1),
        ]));
        let mut dat = parse(&bytes, Path::new("level.dat")).unwrap();
        let before = dat.experiments().unwrap();
        dat.set_beta_apis(true);
        assert_eq!(dat.experiments().unwrap(), before);
    }

    #[test]
    fn disabling_clears_only_gametest() {
        let bytes = build(10, experiments(&[
            ("experiments_ever_used", 1), ("gametest", 1), ("saved_with_toggled_experiments", 1),
        ]));
        let mut dat = parse(&bytes, Path::new("level.dat")).unwrap();
        dat.set_beta_apis(false);

        let after = dat.experiments().unwrap();
        assert_eq!(after.get("gametest"), Some(&0));
        // Historical records of the world having used experiments, not mirrors
        // of the current state.
        assert_eq!(after.get("experiments_ever_used"), Some(&1));
        assert_eq!(after.get("saved_with_toggled_experiments"), Some(&1));
    }

    #[test]
    fn unrelated_experiments_survive_the_flip() {
        // A wholesale rewrite of the compound would silently disable these.
        let bytes = build(10, experiments(&[
            ("data_driven_biomes", 1), ("upcoming_creator_features", 1), ("gametest", 0),
        ]));
        let mut dat = parse(&bytes, Path::new("level.dat")).unwrap();
        dat.set_beta_apis(true);

        let after = dat.experiments().unwrap();
        assert_eq!(after.get("data_driven_biomes"), Some(&1));
        assert_eq!(after.get("upcoming_creator_features"), Some(&1));
        assert_eq!(after.get("gametest"), Some(&1));
    }

    #[test]
    fn a_file_that_cannot_round_trip_refuses_to_be_written() {
        // nbtx 3.0.1 drops the element type and length of an empty list, five
        // bytes short, producing NBT it cannot parse back (§9). Refusing beats
        // corrupting a save.
        let mut root = HashMap::new();
        root.insert("gaps".to_string(), nbtx::Value::List(vec![]));
        let bytes = build(10, nbtx::Value::Compound(root));
        let dat = parse(&bytes, Path::new("level.dat")).unwrap();

        assert!(!dat.is_faithful());
        assert!(matches!(dat.to_bytes(), Err(CoreError::UnwritableLevelDat { .. })));
    }

    #[test]
    fn an_ordinary_file_round_trips_and_reports_the_new_state() {
        let bytes = build(10, experiments(&[("gametest", 0)]));
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("level.dat");
        std::fs::write(&path, &bytes).unwrap();

        let change = apply_beta_apis(&path, true).unwrap();
        assert_eq!(change.before, Some(false));
        assert_eq!(change.after, true);
        assert!(change.changed);

        // The written file is a real level.dat: it parses, and the value took.
        let reread = read(&path).unwrap();
        assert_eq!(reread.beta_apis(), Some(true));
        assert_eq!(reread.version, 10);
    }

    #[test]
    fn applying_the_state_a_world_already_has_reports_no_change() {
        let bytes = build(10, experiments(&[
            ("experiments_ever_used", 1), ("gametest", 1), ("saved_with_toggled_experiments", 1),
        ]));
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("level.dat");
        std::fs::write(&path, &bytes).unwrap();

        let change = apply_beta_apis(&path, true).unwrap();
        assert!(!change.changed);
        assert_eq!(change.before, Some(true));
    }
```

- [ ] **Step 2: Run and watch it fail**

```bash
cargo test -p construct-core leveldat::
```

- [ ] **Step 3: Carry the fidelity verdict on the parsed value**

Extend the struct and `parse`:

```rust
pub struct LevelDat {
    pub version: i32,
    pub root: nbtx::Value,
    /// The payload length this file was read with.
    payload_len: usize,
    /// Whether re-serializing the untouched value reproduces a payload of the
    /// same length. See §9: `nbtx` turns array tags into lists (one byte longer
    /// each) and writes empty lists five bytes short and unparseable. Both
    /// change the length, so this is a reliable refusal rather than a guess.
    faithful: bool,
}
```

At the end of `parse`, before returning:

```rust
    let faithful = nbtx::to_le_bytes(&root)
        .map(|re| re.len() == payload.len())
        .unwrap_or(false);

    Ok(LevelDat { version, root, payload_len: payload.len(), faithful })
```

The extra serialization costs a few kilobytes per world and buys the one check that stands
between a `--beta-apis` flip and a corrupted save.

- [ ] **Step 4: Add the mutation and write path**

```rust
/// The three `Byte` entries that make up the Beta APIs state (§10).
const GAMETEST: &str = "gametest";
const EVER_USED: &str = "experiments_ever_used";
const TOGGLED: &str = "saved_with_toggled_experiments";

impl LevelDat {
    pub fn is_faithful(&self) -> bool {
        self.faithful
    }

    /// Whether Beta APIs are on, or `None` when the world has no `experiments`.
    pub fn beta_apis(&self) -> Option<bool> {
        Some(self.experiments()?.get(GAMETEST).copied().unwrap_or(0) != 0)
    }

    /// Sets the Beta APIs state, creating the compound if the world has none.
    ///
    /// Read-modify-write, never a replacement: worlds carry other experiment
    /// keys and rewriting the compound wholesale would silently disable them.
    /// Disabling leaves the two companion flags at 1 — they record that the
    /// world once used experiments rather than mirroring the current state.
    pub fn set_beta_apis(&mut self, on: bool) {
        let nbtx::Value::Compound(root) = &mut self.root else {
            return;
        };
        let entry = root
            .entry("experiments".to_string())
            .or_insert_with(|| nbtx::Value::Compound(Default::default()));
        let nbtx::Value::Compound(experiments) = entry else {
            return;
        };
        experiments.insert(GAMETEST.to_string(), nbtx::Value::Byte(on as i8));
        if on {
            experiments.insert(EVER_USED.to_string(), nbtx::Value::Byte(1));
            experiments.insert(TOGGLED.to_string(), nbtx::Value::Byte(1));
        }
    }

    /// The complete file: the 8-byte header, then the payload.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        if !self.faithful {
            return Err(CoreError::UnwritableLevelDat {
                path: PathBuf::new(),
                reason: "this level.dat uses NBT this tool cannot rewrite without changing it \
                         (an array tag or an empty list); the world was not modified"
                    .to_string(),
            });
        }
        let payload = nbtx::to_le_bytes(&self.root).map_err(|e| CoreError::UnwritableLevelDat {
            path: PathBuf::new(),
            reason: format!("could not encode NBT: {e}"),
        })?;
        let mut out = Vec::with_capacity(payload.len() + 8);
        out.extend_from_slice(&self.version.to_le_bytes());
        out.extend_from_slice(&(payload.len() as i32).to_le_bytes());
        out.extend_from_slice(&payload);
        Ok(out)
    }
}

/// Writes a `level.dat` atomically: a temporary file beside it, then a rename.
pub fn write(dat: &LevelDat, path: &Path) -> Result<()> {
    let bytes = dat.to_bytes().map_err(|e| match e {
        // `to_bytes` has no path to name; supply it here.
        CoreError::UnwritableLevelDat { reason, .. } => CoreError::UnwritableLevelDat {
            path: path.to_path_buf(),
            reason,
        },
        other => other,
    })?;
    let dir = path.parent().unwrap_or(Path::new("."));
    let tmp = dir.join(format!(
        ".{}.construct-tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("level.dat")
    ));
    std::fs::write(&tmp, &bytes)?;
    // Same directory, so the rename is atomic on every platform we target.
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BetaApisChange {
    pub before: Option<bool>,
    pub after: bool,
    pub changed: bool,
}

/// Reads, flips, writes, then re-reads and verifies.
///
/// §10: treat "write succeeded, value unchanged" as a failure. Backing the file
/// up first is the caller's job — it needs configuration this module does not
/// have.
pub fn apply_beta_apis(path: &Path, on: bool) -> Result<BetaApisChange> {
    let mut dat = read(path)?;
    let before = dat.beta_apis();
    if before == Some(on) {
        return Ok(BetaApisChange { before, after: on, changed: false });
    }
    dat.set_beta_apis(on);
    write(&dat, path)?;

    let verified = read(path)?;
    if verified.beta_apis() != Some(on) {
        return Err(CoreError::UnwritableLevelDat {
            path: path.to_path_buf(),
            reason: format!("wrote the Beta APIs flag but read back {:?}", verified.beta_apis()),
        });
    }
    Ok(BetaApisChange { before, after: on, changed: true })
}
```

Add to `error.rs`:

```rust
    #[error("cannot rewrite {}: {reason}", path.display())]
    UnwritableLevelDat { path: PathBuf, reason: String },
```

`payload_len` is stored but only read by `parse` today; keep it — stage 4 and any future
`level.dat` writer want it, and the field documents what `faithful` was measured against. If
clippy objects to it being unused, expose it as `pub fn payload_len(&self) -> usize`.

- [ ] **Step 5: Run and commit**

```bash
cargo test -p construct-core leveldat:: && cargo test --workspace
cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add crates/construct-core
git commit -m "Flip Beta APIs in level.dat, with a gate against corrupting it"
```

---

### Task 13: `experiment`

**Files:**
- Create: `crates/construct-cli/src/commands/experiment.rs`
- Modify: `crates/construct-cli/src/commands/mod.rs`, `src/cli.rs`, `src/main.rs`
- Modify: `crates/construct-cli/tests/cli.rs`

**Interfaces:**
- Consumes: `leveldat::{read, apply_beta_apis, BetaApisChange}`, `backup::file`, `config::Backups`.
- Produces: `commands::experiment::run(world: &World, state: Option<bool>, backups: &Backups, out: &mut Out) -> Result<()>` — `None` reads and prints, `Some` writes.

`construct experiment <world> --beta-apis [on|off]`. With the value omitted it prints the
current state, which is what makes the flip verifiable without launching the game and gives
`status` something to report.

**Order of operations, and it matters:** back the file up *first*, then flip. A backup taken
after a bad write preserves the bad write.

- [ ] **Step 1: Write the failing tests**

```rust
/// A world whose level.dat carries an `experiments` compound.
fn world_with_experiments(gametest: i8) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let world = root.path().join("minecraftWorlds/Test");
    std::fs::create_dir_all(world.join("db")).unwrap();
    std::fs::write(world.join("levelname.txt"), "Test").unwrap();

    let mut experiments = std::collections::HashMap::new();
    experiments.insert("gametest".to_string(), nbtx::Value::Byte(gametest));
    let mut level = std::collections::HashMap::new();
    level.insert("experiments".to_string(), nbtx::Value::Compound(experiments));
    level.insert("LevelName".to_string(), nbtx::Value::String("Test".into()));

    let payload = nbtx::to_le_bytes(&nbtx::Value::Compound(level)).unwrap();
    let mut bytes = 10i32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&(payload.len() as i32).to_le_bytes());
    bytes.extend_from_slice(&payload);
    std::fs::write(world.join("level.dat"), bytes).unwrap();
    root
}

#[test]
fn experiment_reads_the_current_state_without_writing() {
    let root = world_with_experiments(1);
    let level = root.path().join("minecraftWorlds/Test/level.dat");
    let before = std::fs::read(&level).unwrap();

    let out = bin()
        .args(["experiment", "Test", "--beta-apis", "--json",
               "--com-mojang", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["beta_apis"], true);
    assert_eq!(v["changed"], false);
    assert_eq!(std::fs::read(&level).unwrap(), before, "a read must not write");
}

#[test]
fn experiment_turns_beta_apis_on_and_backs_the_file_up_first() {
    let root = world_with_experiments(0);
    let backups = root.path().join("backups");
    let config = root.path().join("config.toml");
    std::fs::write(&config, format!("[backups]\ndir = {:?}\nkeep = 5\n", backups)).unwrap();

    let out = bin()
        .env("CONSTRUCT_CONFIG", &config)
        .args(["experiment", "Test", "--beta-apis", "on", "--json",
               "--com-mojang", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));

    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["beta_apis"], true);
    assert_eq!(v["changed"], true);
    assert!(v["backup"].is_string());

    // Verified by re-reading, which is what the command itself does.
    let out = bin()
        .args(["experiment", "Test", "--beta-apis", "--json",
               "--com-mojang", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["beta_apis"], true);

    // The backup holds the pre-flip file, so it is not the same as the world's.
    let saved: Vec<_> = walk(&backups).into_iter().filter(|p| p.is_file()).collect();
    assert_eq!(saved.len(), 1, "one backup: {saved:?}");
    assert_ne!(
        std::fs::read(&saved[0]).unwrap(),
        std::fs::read(root.path().join("minecraftWorlds/Test/level.dat")).unwrap()
    );
}

#[test]
fn experiment_on_a_world_with_no_level_dat_fails_cleanly() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("minecraftWorlds/Test/db")).unwrap();
    let out = bin()
        .args(["experiment", "Test", "--beta-apis", "on",
               "--com-mojang", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0));
}
```

Add a small `walk(dir) -> Vec<PathBuf>` helper to the test file if one is not already there, and
add `nbtx` to `construct-cli`'s `[dev-dependencies]`.

- [ ] **Step 2: Run and watch it fail**

```bash
cargo test -p construct-cli --test cli experiment
```

- [ ] **Step 3: Add the subcommand**

```rust
    /// Read or set a world's experimental toggles.
    Experiment {
        /// World name, qualified reference, or path.
        world: String,
        /// Beta APIs (`gametest`). Omit the value to print the current state.
        #[arg(long, required = true, num_args = 0..=1, value_name = "on|off")]
        beta_apis: Option<OnOff>,
    },
```

```rust
#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
pub enum OnOff {
    On,
    Off,
}
```

`--beta-apis` is `required`, so `None` unambiguously means "the flag was given without a
value" — which is the read form. Both spellings need a test; the reading test above covers
`None` and the writing test covers `Some(On)`.

- [ ] **Step 4: Write the command**

```rust
pub fn run(world: &World, state: Option<bool>, backups: &Backups, out: &mut Out) -> Result<()> {
    let path = world.path.join("level.dat");

    let Some(on) = state else {
        let dat = leveldat::read(&path)?;
        let current = dat.beta_apis();
        out.line(format!(
            "Beta APIs: {}",
            match current {
                Some(true) => "on",
                Some(false) => "off",
                None => "off (this world has no experiments)",
            }
        ));
        out.emit(Payload {
            world: world.qualified(),
            beta_apis: current.unwrap_or(false),
            changed: false,
            backup: None,
        });
        return Ok(());
    };

    // Back up before touching anything: a backup taken after a bad write
    // preserves the bad write.
    let backup = backup::file(&path, &world.qualified(), backups)?;
    let change = leveldat::apply_beta_apis(&path, on)?;

    if change.changed {
        out.line(format!("Beta APIs: {} → {}", yes_no(change.before), yes_no(Some(change.after))));
        out.line(format!("  backup: {}", backup.display()));
        out.line("Reload the world for the change to take effect.");
    } else {
        out.line(format!("Beta APIs already {}; nothing to do", yes_no(Some(on))));
    }
    out.emit(Payload {
        world: world.qualified(),
        beta_apis: change.after,
        changed: change.changed,
        backup: Some(backup.display().to_string()),
    });
    Ok(())
}
```

Map `CoreError::UnwritableLevelDat` to exit 1 (the default arm already does) and give it a
`report` arm saying the world was not modified.

- [ ] **Step 5: Run and commit**

```bash
cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add crates/construct-cli
git commit -m "Read and flip a world's Beta APIs toggle"
```

---

### Task 14: The releases client

**Files:**
- Create: `crates/construct-core/src/install/mod.rs`
- Create: `crates/construct-core/src/install/releases.rs`
- Modify: `crates/construct-core/src/lib.rs`, `Cargo.toml` (workspace and core)
- Modify: `crates/construct-core/src/error.rs`

**Interfaces:**
- Consumes: `ureq` 3.4, `serde_json`.
- Produces:
  - `install::releases::{Release, Asset, Releases, GitHub}` where `Release { tag: String, assets: Vec<Asset> }`, `Asset { name: String, url: String, size: u64 }`.
  - `trait Releases { fn release(&self, version: Option<&str>) -> Result<Release>; fn download(&self, asset: &Asset, to: &Path) -> Result<u64>; }`
  - `install::releases::parse_release(json: &str) -> Result<Release>` — pure, so the shape is tested without a network.
  - `install::releases::asset_for(release: &Release) -> Result<&Asset>` — matches `Construct-v*.mcaddon`.
  - `GitHub::new(token: Option<String>) -> Self`, `GitHub::with_base(base: impl Into<String>, token: Option<String>) -> Self`
  - `CoreError::{Network { reason }, RateLimited, AssetNotFound { version, available }}`

Dependencies to add to `[workspace.dependencies]` and to `construct-core`:

```toml
ureq = "3.4"
zip = { version = "8.6", default-features = false, features = ["deflate-flate2-zlib-rs"] }
```

Both were resolved and built against this toolchain while writing this plan. `zip`'s default
features drag in bzip2, lzma, zstd, and AES; a `.mcaddon` is stored-or-deflated, so the minimal
feature set is the right one and keeps the build pure Rust.

Endpoints (measured — `ForestOfLight/Construct`'s latest is `v1.2.0` with one asset,
`Construct-v1.2.0.mcaddon`, 1,234,238 bytes):

```
https://api.github.com/repos/ForestOfLight/Construct/releases/latest
https://api.github.com/repos/ForestOfLight/Construct/releases/tags/v<version>
```

The token comes from `CONSTRUCT_GITHUB_TOKEN`, falling back to `GITHUB_TOKEN` (§7). The
unauthenticated limit is 60 requests an hour, and §11 wants that number in the error.

- [ ] **Step 1: Write the failing tests**

In `releases.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed from the real /releases/latest response.
    const LATEST: &str = r#"{
        "tag_name": "v1.2.0",
        "name": "v1.2.0 for MC 26.40",
        "assets": [
            { "name": "Construct-v1.2.0.mcaddon",
              "size": 1234238,
              "content_type": "application/octet-stream",
              "browser_download_url": "https://github.com/ForestOfLight/Construct/releases/download/v1.2.0/Construct-v1.2.0.mcaddon" }
        ]
    }"#;

    #[test]
    fn parses_a_release_and_its_asset() {
        let r = parse_release(LATEST).unwrap();
        assert_eq!(r.tag, "v1.2.0");
        assert_eq!(r.assets.len(), 1);
        assert_eq!(r.assets[0].size, 1234238);
        assert!(r.assets[0].url.ends_with("Construct-v1.2.0.mcaddon"));
    }

    #[test]
    fn the_mcaddon_asset_is_matched_by_pattern_not_position() {
        let r = Release {
            tag: "v9.9.9".into(),
            assets: vec![
                Asset { name: "sha256sums.txt".into(), url: "u1".into(), size: 1 },
                Asset { name: "Construct-v9.9.9.mcaddon".into(), url: "u2".into(), size: 2 },
            ],
        };
        assert_eq!(asset_for(&r).unwrap().url, "u2");
    }

    #[test]
    fn a_release_with_no_mcaddon_lists_what_it_did_have() {
        let r = Release {
            tag: "v9.9.9".into(),
            assets: vec![Asset { name: "notes.txt".into(), url: "u".into(), size: 1 }],
        };
        let CoreError::AssetNotFound { available, .. } = asset_for(&r).unwrap_err() else {
            panic!("expected AssetNotFound");
        };
        assert_eq!(available, vec!["notes.txt".to_string()]);
    }

    #[test]
    fn a_version_is_accepted_with_or_without_its_v() {
        assert_eq!(tag_for("1.2.0"), "v1.2.0");
        assert_eq!(tag_for("v1.2.0"), "v1.2.0");
    }

    #[test]
    fn malformed_json_is_a_network_error_not_a_panic() {
        assert!(matches!(parse_release("{ nope"), Err(CoreError::Network { .. })));
    }
}
```

- [ ] **Step 2: Run and watch it fail**

```bash
cargo test -p construct-core releases::
```

- [ ] **Step 3: Implement**

```rust
//! The GitHub releases lookup behind a trait, so install is fully mockable.

use crate::error::{CoreError, Result};
use serde::Deserialize;
use std::path::Path;

pub const REPO: &str = "ForestOfLight/Construct";
/// GitHub's unauthenticated limit, named in the error §11 asks for.
pub const UNAUTHENTICATED_LIMIT: u32 = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub tag: String,
    pub assets: Vec<Asset>,
}

pub trait Releases {
    /// `None` means the latest release.
    fn release(&self, version: Option<&str>) -> Result<Release>;
    fn download(&self, asset: &Asset, to: &Path) -> Result<u64>;
}

#[derive(Deserialize)]
struct RawRelease {
    tag_name: String,
    #[serde(default)]
    assets: Vec<RawAsset>,
}

#[derive(Deserialize)]
struct RawAsset {
    name: String,
    #[serde(default)]
    size: u64,
    browser_download_url: String,
}

pub fn parse_release(json: &str) -> Result<Release> {
    let raw: RawRelease = serde_json::from_str(json).map_err(|e| CoreError::Network {
        reason: format!("unexpected response from GitHub: {e}"),
    })?;
    Ok(Release {
        tag: raw.tag_name,
        assets: raw
            .assets
            .into_iter()
            .map(|a| Asset { name: a.name, url: a.browser_download_url, size: a.size })
            .collect(),
    })
}

/// `1.2.0` and `v1.2.0` both name the same tag.
pub fn tag_for(version: &str) -> String {
    if version.starts_with('v') {
        version.to_string()
    } else {
        format!("v{version}")
    }
}

/// The `.mcaddon`, matched by pattern rather than position.
pub fn asset_for(release: &Release) -> Result<&Asset> {
    release
        .assets
        .iter()
        .find(|a| a.name.starts_with("Construct-v") && a.name.ends_with(".mcaddon"))
        .ok_or_else(|| CoreError::AssetNotFound {
            version: release.tag.clone(),
            available: release.assets.iter().map(|a| a.name.clone()).collect(),
        })
}

pub struct GitHub {
    base: String,
    token: Option<String>,
}

impl GitHub {
    pub fn new(token: Option<String>) -> Self {
        Self { base: "https://api.github.com".to_string(), token }
    }

    /// Points the client somewhere else. Exists so a test can serve canned
    /// responses without reaching the network.
    pub fn with_base(base: impl Into<String>, token: Option<String>) -> Self {
        Self { base: base.into(), token }
    }
}

impl Releases for GitHub {
    fn release(&self, version: Option<&str>) -> Result<Release> {
        let url = match version {
            Some(v) => format!("{}/repos/{REPO}/releases/tags/{}", self.base, tag_for(v)),
            None => format!("{}/repos/{REPO}/releases/latest", self.base),
        };
        let mut req = ureq::get(&url).header("User-Agent", "ConstructCLI");
        if let Some(token) = &self.token {
            req = req.header("Authorization", &format!("Bearer {token}"));
        }
        let mut response = req.call().map_err(map_transport)?;
        let status = response.status().as_u16();
        // 403 and 429 both carry the rate limit; the header is what separates
        // "you are out of requests" from "you may not have this".
        if matches!(status, 403 | 429)
            && response.headers().get("x-ratelimit-remaining").map(|v| v == "0").unwrap_or(false)
        {
            return Err(CoreError::RateLimited);
        }
        if status == 404 {
            return Err(CoreError::AssetNotFound {
                version: version.unwrap_or("latest").to_string(),
                available: Vec::new(),
            });
        }
        if !(200..300).contains(&status) {
            return Err(CoreError::Network { reason: format!("GitHub returned HTTP {status}") });
        }
        let body = response.body_mut().read_to_string().map_err(map_transport)?;
        parse_release(&body)
    }

    fn download(&self, asset: &Asset, to: &Path) -> Result<u64> {
        let mut response = ureq::get(&asset.url)
            .header("User-Agent", "ConstructCLI")
            .call()
            .map_err(map_transport)?;
        let mut reader = response.body_mut().as_reader();
        let mut file = std::fs::File::create(to)?;
        let written = std::io::copy(&mut reader, &mut file)?;
        Ok(written)
    }
}

fn map_transport(e: impl std::fmt::Display) -> CoreError {
    CoreError::Network { reason: e.to_string() }
}
```

Errors:

```rust
    #[error("could not reach GitHub: {reason}")]
    Network { reason: String },

    #[error("GitHub rate limit reached")]
    RateLimited,

    #[error("no Construct .mcaddon for {version}")]
    AssetNotFound { version: String, available: Vec<String> },
```

CLI: `AssetNotFound` is exit 3, `Network` and `RateLimited` are exit 1, and `report` says:

```rust
        CoreError::RateLimited => {
            eprintln!(
                "\nGitHub allows {} requests an hour unauthenticated.\n\
                 Set a token to raise it:\n  export CONSTRUCT_GITHUB_TOKEN=<token>",
                construct_core::install::releases::UNAUTHENTICATED_LIMIT
            );
        }
        CoreError::AssetNotFound { available, .. } if !available.is_empty() => {
            eprintln!("\navailable assets:");
            for a in available {
                eprintln!("  {a}");
            }
        }
```

The exact `ureq` 3.x response API (`status()`, `headers()`, `body_mut().read_to_string()`) should
be checked against the version that resolves; adjust the calls, not the behaviour, if it differs.

- [ ] **Step 4: Run and commit**

```bash
cargo test -p construct-core releases:: && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add crates Cargo.toml
git commit -m "Look up Construct releases"
```

---

### Task 15: Unpacking a `.mcaddon`

**Files:**
- Create: `crates/construct-core/src/install/mcaddon.rs`
- Create: `crates/construct-core/tests/install.rs`
- Modify: `crates/construct-core/Cargo.toml` (`zip` in dependencies and dev-dependencies)

**Interfaces:**
- Consumes: `zip` 8.6, `pack::manifest::{read, PackKind}`.
- Produces:
  - `install::mcaddon::Extracted { dir: tempfile::TempDir, behavior: PathBuf, resource: PathBuf }`
  - `install::mcaddon::extract(archive: &Path) -> Result<Extracted>`

A `.mcaddon` is a zip. The real one, measured, contains exactly two top-level directories —
`Construct[BP]/` (modules `data` and `script`, and a `structures/construct.mcstructure` of its
own) and `Construct[RP]/` (module `resources`). **BP and RP are told apart by module type, never
by folder name.**

**Zip slip is a real attack and this is where it lands.** An entry named `../../../.bashrc`
would escape the extraction directory. Every entry's path is validated before it is written;
anything with a parent-directory or root component refuses the whole archive.

- [ ] **Step 1: Write the failing tests**

Create `crates/construct-core/tests/install.rs`:

```rust
use construct_core::install::mcaddon;
use std::io::Write;
use std::path::Path;

/// Builds a synthetic `.mcaddon` from (path, contents) pairs.
fn make_addon(at: &Path, entries: &[(&str, &[u8])]) {
    let file = std::fs::File::create(at).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in entries {
        zip.start_file(*name, options).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}

fn manifest(name: &str, uuid: &str, module: &str) -> Vec<u8> {
    format!(
        r#"{{"format_version":2,
            "header":{{"name":"{name}","uuid":"{uuid}","version":[1,2,0]}},
            "modules":[{{"type":"{module}","uuid":"22222222-2222-2222-2222-222222222222","version":[1,0,0]}}]}}"#
    )
    .into_bytes()
}

fn real_shaped_addon(at: &Path) {
    make_addon(
        at,
        &[
            ("Construct[BP]/manifest.json", &manifest("Construct [BP] v1.2.0", construct_core::pack::CONSTRUCT_BP_UUID, "data")),
            ("Construct[BP]/scripts/main.js", b"// code"),
            ("Construct[BP]/structures/construct.mcstructure", b"shipped"),
            ("Construct[RP]/manifest.json", &manifest("Construct [RP] v1.2.0", construct_core::pack::CONSTRUCT_RP_UUID, "resources")),
        ],
    );
}

#[test]
fn extract_finds_the_behaviour_and_resource_packs_by_module_type() {
    let tmp = tempfile::tempdir().unwrap();
    let addon = tmp.path().join("Construct-v1.2.0.mcaddon");
    real_shaped_addon(&addon);

    let extracted = mcaddon::extract(&addon).unwrap();
    assert_eq!(extracted.behavior.file_name().unwrap(), "Construct[BP]");
    assert_eq!(extracted.resource.file_name().unwrap(), "Construct[RP]");
    assert_eq!(
        std::fs::read(extracted.behavior.join("structures/construct.mcstructure")).unwrap(),
        b"shipped"
    );
    assert!(extracted.behavior.join("scripts/main.js").is_file());
}

#[test]
fn an_entry_that_would_escape_the_directory_refuses_the_whole_archive() {
    let tmp = tempfile::tempdir().unwrap();
    let addon = tmp.path().join("evil.mcaddon");
    make_addon(&addon, &[("../escaped.txt", b"pwned"), ("BP/manifest.json", &manifest("x", "u", "data"))]);

    assert!(mcaddon::extract(&addon).is_err());
    assert!(!tmp.path().join("escaped.txt").exists());
    assert!(!Path::new("../escaped.txt").exists());
}

#[test]
fn an_archive_with_no_resource_pack_says_so() {
    let tmp = tempfile::tempdir().unwrap();
    let addon = tmp.path().join("half.mcaddon");
    make_addon(&addon, &[("BP/manifest.json", &manifest("x", "u", "data"))]);

    let err = mcaddon::extract(&addon).unwrap_err();
    assert!(matches!(err, construct_core::CoreError::BadPack { .. }), "got {err:?}");
}

#[test]
fn folder_names_do_not_decide_which_pack_is_which() {
    // Swapped names, correct module types.
    let tmp = tempfile::tempdir().unwrap();
    let addon = tmp.path().join("swapped.mcaddon");
    make_addon(
        &addon,
        &[
            ("looks_like_rp/manifest.json", &manifest("a", "u1", "data")),
            ("looks_like_bp/manifest.json", &manifest("b", "u2", "resources")),
        ],
    );
    let extracted = mcaddon::extract(&addon).unwrap();
    assert_eq!(extracted.behavior.file_name().unwrap(), "looks_like_rp");
    assert_eq!(extracted.resource.file_name().unwrap(), "looks_like_bp");
}

#[test]
fn a_file_that_is_not_a_zip_is_a_bad_pack() {
    let tmp = tempfile::tempdir().unwrap();
    let not_zip = tmp.path().join("x.mcaddon");
    std::fs::write(&not_zip, b"definitely not a zip").unwrap();
    assert!(mcaddon::extract(&not_zip).is_err());
}
```

- [ ] **Step 2: Run and watch it fail**

```bash
cargo test -p construct-core --test install
```

- [ ] **Step 3: Implement**

```rust
//! A `.mcaddon` is a zip holding one behaviour pack and one resource pack.

use crate::error::{CoreError, Result};
use crate::pack::manifest::{self, PackKind};
use std::path::{Component, Path, PathBuf};

pub struct Extracted {
    /// Held so the extraction outlives the paths below.
    pub dir: tempfile::TempDir,
    pub behavior: PathBuf,
    pub resource: PathBuf,
}

/// Rejects any archive entry that would write outside the extraction directory.
///
/// Zip slip: an entry named `../../.bashrc` escapes wherever you unpack it. One
/// bad entry refuses the whole archive rather than being skipped, because a
/// partial extraction of a hostile archive is not something to carry on with.
fn safe_entry(name: &str) -> Result<PathBuf> {
    let bad = |reason: &str| CoreError::BadPack {
        path: PathBuf::from(name),
        reason: reason.to_string(),
    };
    if name.contains('\\') {
        return Err(bad("a backslash in an archive path"));
    }
    let path = Path::new(name);
    for c in path.components() {
        match c {
            Component::Normal(_) | Component::CurDir => {}
            _ => return Err(bad("a path that would escape the extraction directory")),
        }
    }
    Ok(path.to_path_buf())
}

pub fn extract(archive: &Path) -> Result<Extracted> {
    let bad = |reason: String| CoreError::BadPack { path: archive.to_path_buf(), reason };

    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| bad(e.to_string()))?;
    let dir = tempfile::tempdir()?;

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| bad(e.to_string()))?;
        let name = entry.name().to_string();
        let relative = safe_entry(&name)?;
        let out = dir.path().join(&relative);

        if name.ends_with('/') {
            std::fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut writer = std::fs::File::create(&out)?;
        std::io::copy(&mut entry, &mut writer)?;
    }

    // Which is which comes from the manifests, never from the folder names.
    let mut behavior = None;
    let mut resource = None;
    for e in std::fs::read_dir(dir.path())?.flatten() {
        let path = e.path();
        if !path.is_dir() {
            continue;
        }
        let Ok(m) = manifest::read(&path) else { continue };
        match m.kind {
            PackKind::Behavior => behavior = Some(path),
            PackKind::Resource => resource = Some(path),
        }
    }

    let behavior = behavior.ok_or_else(|| bad("no behaviour pack in this .mcaddon".into()))?;
    let resource = resource.ok_or_else(|| bad("no resource pack in this .mcaddon".into()))?;
    Ok(Extracted { dir, behavior, resource })
}
```

- [ ] **Step 4: Run and commit**

```bash
cargo test -p construct-core --test install && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add crates/construct-core
git commit -m "Unpack a .mcaddon safely"
```

---

### Task 16: Placing a pack, preserving `structures/`

**The highest-priority test in the whole spec lives here** (§12, §15): an upgrade must not
destroy `structures/`. That folder holds the user's imported structures — the very data this
tool exists to put there — and a naive delete-and-unzip would take them with it.

**Files:**
- Modify: `crates/construct-core/src/install/mod.rs`
- Modify: `crates/construct-core/tests/install.rs`

**Interfaces:**
- Consumes: `pack::{find_by_uuid, manifest}`, `store::snapshot::copy_dir`.
- Produces:
  - `install::Placed { dir: PathBuf, from: Option<[u32; 3]>, to: [u32; 3], preserved: usize, changed: bool }`
  - `install::place(root: &Path, src: &Path, force: bool) -> Result<Placed>` — `root` is a `development_*_packs` directory, `src` an extracted pack directory.

Rules, all from §10:

- The existing copy is found **by header UUID**, so a renamed folder is upgraded in place rather
  than installed a second time alongside itself.
- Same version and not `--force` → **no-op**, `changed: false`. `install` is idempotent.
- Otherwise the old directory is replaced, except that every file under its `structures/` is
  carried across — apart from ones the new version ships itself, which win. The count carried
  over is reported.

- [ ] **Step 1: Write the failing tests**

```rust
use construct_core::install;

/// A pack directory on disk, as if previously installed.
fn installed_pack(root: &Path, folder: &str, uuid: &str, version: [u32; 3], structures: &[(&str, &[u8])]) -> std::path::PathBuf {
    let dir = root.join(folder);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("manifest.json"),
        format!(
            r#"{{"format_version":2,"header":{{"name":"{folder}","uuid":"{uuid}","version":[{},{},{}]}},
                "modules":[{{"type":"data","uuid":"33333333-3333-3333-3333-333333333333","version":[1,0,0]}}]}}"#,
            version[0], version[1], version[2]
        ),
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("structures")).unwrap();
    for (name, bytes) in structures {
        std::fs::write(dir.join("structures").join(format!("{name}.mcstructure")), bytes).unwrap();
    }
    dir
}

#[test]
fn an_upgrade_keeps_every_imported_structure() {
    // The highest-priority test in the spec: this folder holds the user's data.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("development_behavior_packs");
    installed_pack(&root, "Construct[BP]", construct_core::pack::CONSTRUCT_BP_UUID, [1, 1, 0],
                   &[("bomber", b"mine"), ("castle", b"also mine"), ("construct", b"old shipped")]);

    let new = tmp.path().join("new/Construct[BP]");
    installed_pack(new.parent().unwrap(), "Construct[BP]", construct_core::pack::CONSTRUCT_BP_UUID, [1, 2, 0],
                   &[("construct", b"new shipped")]);
    std::fs::write(new.join("scripts.js"), b"v1.2.0").unwrap();

    let placed = install::place(&root, &new, false).unwrap();
    assert_eq!(placed.from, Some([1, 1, 0]));
    assert_eq!(placed.to, [1, 2, 0]);
    assert_eq!(placed.preserved, 2, "bomber and castle, not the shipped one");
    assert!(placed.changed);

    let structures = placed.dir.join("structures");
    assert_eq!(std::fs::read(structures.join("bomber.mcstructure")).unwrap(), b"mine");
    assert_eq!(std::fs::read(structures.join("castle.mcstructure")).unwrap(), b"also mine");
    // A file the new version ships wins over the copy already there.
    assert_eq!(std::fs::read(structures.join("construct.mcstructure")).unwrap(), b"new shipped");
    // And the new version's own files arrived.
    assert_eq!(std::fs::read(placed.dir.join("scripts.js")).unwrap(), b"v1.2.0");
}

#[test]
fn a_renamed_folder_is_upgraded_in_place_not_installed_twice() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("development_behavior_packs");
    installed_pack(&root, "my-construct-copy", construct_core::pack::CONSTRUCT_BP_UUID, [1, 1, 0], &[]);

    let new = tmp.path().join("new/Construct[BP]");
    installed_pack(new.parent().unwrap(), "Construct[BP]", construct_core::pack::CONSTRUCT_BP_UUID, [1, 2, 0], &[]);

    let placed = install::place(&root, &new, false).unwrap();
    assert_eq!(placed.dir.file_name().unwrap(), "my-construct-copy");
    assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1, "no second copy");
}

#[test]
fn installing_the_version_already_present_is_a_no_op() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("development_behavior_packs");
    let existing = installed_pack(&root, "Construct[BP]", construct_core::pack::CONSTRUCT_BP_UUID, [1, 2, 0], &[("keep", b"x")]);
    std::fs::write(existing.join("marker"), b"untouched").unwrap();

    let new = tmp.path().join("new/Construct[BP]");
    installed_pack(new.parent().unwrap(), "Construct[BP]", construct_core::pack::CONSTRUCT_BP_UUID, [1, 2, 0], &[]);

    let placed = install::place(&root, &new, false).unwrap();
    assert!(!placed.changed);
    assert_eq!(placed.from, Some([1, 2, 0]));
    assert_eq!(std::fs::read(existing.join("marker")).unwrap(), b"untouched");

    // --force reinstalls the same version, and still keeps the structures.
    let placed = install::place(&root, &new, true).unwrap();
    assert!(placed.changed);
    assert_eq!(placed.preserved, 1);
}

#[test]
fn a_first_install_creates_the_root_and_copies_the_pack() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("development_behavior_packs");
    let new = tmp.path().join("new/Construct[BP]");
    installed_pack(new.parent().unwrap(), "Construct[BP]", construct_core::pack::CONSTRUCT_BP_UUID, [1, 2, 0], &[("construct", b"shipped")]);

    let placed = install::place(&root, &new, false).unwrap();
    assert_eq!(placed.from, None);
    assert_eq!(placed.preserved, 0);
    assert_eq!(placed.dir, root.join("Construct[BP]"));
    assert_eq!(std::fs::read(placed.dir.join("structures/construct.mcstructure")).unwrap(), b"shipped");
}
```

- [ ] **Step 2: Run and watch it fail**

```bash
cargo test -p construct-core --test install
```

- [ ] **Step 3: Implement**

```rust
//! Installing Construct: place the packs, keep the user's structures.

pub mod mcaddon;
pub mod releases;

use crate::error::{CoreError, Result};
use crate::pack::{self, manifest};
use crate::store::snapshot::copy_dir;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    pub dir: PathBuf,
    /// The version that was there before, if any.
    pub from: Option<[u32; 3]>,
    pub to: [u32; 3],
    /// How many of the user's structures were carried across an upgrade.
    pub preserved: usize,
    pub changed: bool,
}

/// Places an extracted pack into a `development_*_packs` root.
pub fn place(root: &Path, src: &Path, force: bool) -> Result<Placed> {
    let incoming = manifest::read(src)?;
    let existing = pack::find_by_uuid(root, &incoming.uuid);

    // Idempotent: the same version already installed is nothing to do.
    if let Some(existing) = &existing
        && existing.manifest.version == incoming.version
        && !force
    {
        return Ok(Placed {
            dir: existing.dir.clone(),
            from: Some(existing.manifest.version),
            to: incoming.version,
            preserved: 0,
            changed: false,
        });
    }

    let dest = match &existing {
        // Matched by UUID, so a renamed folder is upgraded where it stands.
        Some(e) => e.dir.clone(),
        None => root.join(
            src.file_name()
                .ok_or_else(|| CoreError::BadPack {
                    path: src.to_path_buf(),
                    reason: "the extracted pack has no folder name".to_string(),
                })?,
        ),
    };

    // Carry the user's structures out of harm's way before the old directory
    // goes. This is the data the whole tool exists to put there.
    let holding = tempfile::tempdir()?;
    let mut preserved = Vec::new();
    let old_structures = pack::structures::dir(&dest);
    if old_structures.is_dir() {
        copy_dir(&old_structures, holding.path())?;
        preserved = relative_files(holding.path());
    }

    if dest.exists() {
        std::fs::remove_dir_all(&dest)?;
    }
    std::fs::create_dir_all(&dest)?;
    copy_dir(src, &dest)?;

    // Restore everything the new version does not ship itself.
    let new_structures = pack::structures::dir(&dest);
    let mut restored = 0;
    for relative in &preserved {
        let to = new_structures.join(relative);
        if to.exists() {
            continue;
        }
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(holding.path().join(relative), &to)?;
        restored += 1;
    }

    Ok(Placed {
        dir: dest,
        from: existing.map(|e| e.manifest.version),
        to: incoming.version,
        preserved: restored,
        changed: true,
    })
}

/// Every file under `dir`, as paths relative to it.
fn relative_files(dir: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, base: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() {
                walk(&path, base, out);
            } else if let Ok(rel) = path.strip_prefix(base) {
                out.push(rel.to_path_buf());
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}
```

`copy_dir` already exists in `store::snapshot` and does exactly this job; re-exporting or
moving it is not worth the churn — call it where it is.

- [ ] **Step 4: Run and commit**

```bash
cargo test -p construct-core --test install && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add crates/construct-core
git commit -m "Place a pack without destroying the user's structures"
```

---

### Task 17: The world pack lists

**Files:**
- Create: `crates/construct-core/src/worldpacks.rs`
- Modify: `crates/construct-core/src/lib.rs`

**Interfaces:**
- Produces:
  - `worldpacks::PackRef { pack_id: String, version: [u32; 3] }`
  - `worldpacks::behavior_path(world: &World) -> PathBuf` / `resource_path(world: &World) -> PathBuf`
  - `worldpacks::read(path: &Path) -> Result<Vec<PackRef>>` — a missing file is an empty list.
  - `worldpacks::upsert(path: &Path, entry: PackRef) -> Result<bool>` — `true` when the file changed.

Measured on a real world (§10): the files are flat arrays of `{ pack_id, version }`, the game
matches development packs by UUID, and it leaves a stale version in place across a pack upgrade
— that world records `[1, 0, 0]` for an installed `v1.2.0`. So the upsert keys on `pack_id`
alone and writes the installed version over whatever was recorded.

`world_behavior_pack_history.json` sits beside these and is Minecraft's own record. **It is
never written.**

- [ ] **Step 1: Write the failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// Exactly the formatting a real world uses: tabs, spaces before colons,
    /// and a stray blank first line.
    const REAL: &str = "\n[\n\t\n\t{\n\t\t\"pack_id\" : \"8c0c0153-d8b9-482a-889f-aef922b8fe58\",\n\t\t\"version\" : [ 1, 0, 0 ]\n\t}\n]";

    #[test]
    fn reads_a_file_as_minecraft_actually_writes_it() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("world_behavior_packs.json");
        std::fs::write(&path, REAL).unwrap();

        let packs = read(&path).unwrap();
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].pack_id, "8c0c0153-d8b9-482a-889f-aef922b8fe58");
        assert_eq!(packs[0].version, [1, 0, 0]);
    }

    #[test]
    fn a_missing_file_is_an_empty_list_not_an_error() {
        assert!(read(Path::new("/no/such/world_behavior_packs.json")).unwrap().is_empty());
    }

    #[test]
    fn upsert_adds_an_entry_to_a_world_that_had_none() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("world_behavior_packs.json");

        assert!(upsert(&path, PackRef { pack_id: "abc".into(), version: [1, 2, 0] }).unwrap());
        assert_eq!(read(&path).unwrap()[0].pack_id, "abc");
    }

    #[test]
    fn upsert_replaces_a_stale_version_in_place() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("world_behavior_packs.json");
        std::fs::write(&path, REAL).unwrap();

        let changed = upsert(
            &path,
            PackRef { pack_id: "8c0c0153-d8b9-482a-889f-aef922b8fe58".into(), version: [1, 2, 0] },
        )
        .unwrap();
        assert!(changed);

        let packs = read(&path).unwrap();
        assert_eq!(packs.len(), 1, "replaced, not appended");
        assert_eq!(packs[0].version, [1, 2, 0]);
    }

    #[test]
    fn upsert_of_an_identical_entry_changes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("world_behavior_packs.json");
        std::fs::write(&path, REAL).unwrap();
        let before = std::fs::read(&path).unwrap();

        let changed = upsert(
            &path,
            PackRef { pack_id: "8c0c0153-d8b9-482a-889f-aef922b8fe58".into(), version: [1, 0, 0] },
        )
        .unwrap();
        assert!(!changed);
        assert_eq!(std::fs::read(&path).unwrap(), before, "an unchanged upsert must not rewrite the file");
    }

    #[test]
    fn other_packs_survive_an_upsert() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("world_behavior_packs.json");
        std::fs::write(
            &path,
            r#"[{"pack_id":"other","version":[0,9,0]},{"pack_id":"another","version":[1,0,1]}]"#,
        )
        .unwrap();

        upsert(&path, PackRef { pack_id: "new".into(), version: [1, 2, 0] }).unwrap();
        let ids: Vec<String> = read(&path).unwrap().into_iter().map(|p| p.pack_id).collect();
        assert_eq!(ids, vec!["other".to_string(), "another".to_string(), "new".to_string()]);
    }

    #[test]
    fn a_malformed_file_is_an_error_rather_than_being_overwritten() {
        // Silently replacing an unreadable list would drop every other pack the
        // world had enabled.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("world_behavior_packs.json");
        std::fs::write(&path, "{ not an array").unwrap();
        assert!(read(&path).is_err());
        assert!(upsert(&path, PackRef { pack_id: "x".into(), version: [1, 0, 0] }).is_err());
    }
}
```

- [ ] **Step 2: Run and watch it fail**

```bash
cargo test -p construct-core worldpacks::
```

- [ ] **Step 3: Implement**

```rust
//! `world_behavior_packs.json` and `world_resource_packs.json`.
//!
//! Flat arrays of `{ pack_id, version }`. The game matches development packs by
//! UUID and leaves a stale version in place across an upgrade, so the upsert
//! keys on `pack_id` alone. `world_behavior_pack_history.json` beside these is
//! Minecraft's own record and is never written.

use crate::discovery::World;
use crate::error::{CoreError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackRef {
    pub pack_id: String,
    pub version: [u32; 3],
}

pub fn behavior_path(world: &World) -> PathBuf {
    world.path.join("world_behavior_packs.json")
}

pub fn resource_path(world: &World) -> PathBuf {
    world.path.join("world_resource_packs.json")
}

pub fn read(path: &Path) -> Result<Vec<PackRef>> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(&text).map_err(|e| CoreError::BadPack {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })
}

/// Adds or replaces one entry, keyed on `pack_id`. Returns whether anything changed.
pub fn upsert(path: &Path, entry: PackRef) -> Result<bool> {
    let mut packs = read(path)?;
    match packs.iter_mut().find(|p| p.pack_id == entry.pack_id) {
        Some(existing) if *existing == entry => return Ok(false),
        Some(existing) => *existing = entry,
        None => packs.push(entry),
    }
    let text = serde_json::to_string_pretty(&packs).map_err(|e| CoreError::BadPack {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })?;
    std::fs::write(path, text)?;
    Ok(true)
}
```

Formatting is not preserved — the game parses JSON, not whitespace (§10).

- [ ] **Step 4: Run and commit**

```bash
cargo test -p construct-core worldpacks:: && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add crates/construct-core
git commit -m "Enable a pack in a world"
```

---

### Task 18: `install`

**Files:**
- Create: `crates/construct-cli/src/commands/install.rs`
- Modify: `crates/construct-cli/src/commands/mod.rs`, `src/cli.rs`, `src/main.rs`
- Modify: `crates/construct-cli/tests/cli.rs`

**Interfaces:**
- Consumes: `install::{place, Placed, releases::{Releases, GitHub, asset_for}, mcaddon::extract}`, `worldpacks::upsert`, `leveldat::apply_beta_apis`, `backup::file`, `pack::{behavior_root, resource_root, CONSTRUCT_BP_UUID, CONSTRUCT_RP_UUID}`.
- Produces: `commands::install::run(releases: &dyn Releases, version: Option<&str>, world: Option<&World>, installation: &Installation, backups: &Backups, force: bool, out: &mut Out) -> Result<()>`

The sequence is §10's, in order:

1. Query the releases API for `latest`, or `tags/v<version>` with `--version`.
2. Match the `Construct-v*.mcaddon` asset by pattern.
3. Download to a temporary directory.
4. Unzip; identify BP versus RP by module type.
5. Place both, matching an existing install by header UUID.
6. With `--world`, upsert both pack IDs into that world's pack lists.
7. Back up `level.dat`, attempt the Beta APIs flip, then re-read and verify.

**A failed `level.dat` write exits 5** with the packs already installed, naming the remaining
manual step, rather than reporting total failure. `install --world` on a world whose Construct
copy is world-local still deploys into the *installation's* dev-pack root — §6: `install --world W`
derives the installation from W and deploys to that installation's root, so `preview` and
`release` are never mixed.

**The endpoint is overridable** with `CONSTRUCT_GITHUB_API`, which is what makes this command
testable without the network. Document it in the README as a testing and enterprise-proxy escape
hatch.

- [ ] **Step 1: Write the failing test, with a stub server**

Append to `crates/construct-cli/tests/cli.rs`:

```rust
/// A single-threaded HTTP server that answers exactly the two requests
/// `install` makes, then stops. Returns its base URL and a join handle.
///
/// This is what lets `install` be tested end to end — download included —
/// without the network or a mocking framework.
fn stub_github(addon: Vec<u8>) -> (String, std::thread::JoinHandle<()>) {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let base = format!("http://127.0.0.1:{port}");
    let release = format!(
        r#"{{"tag_name":"v1.2.0","assets":[{{"name":"Construct-v1.2.0.mcaddon","size":{},"browser_download_url":"{base}/download"}}]}}"#,
        addon.len()
    );

    let handle = std::thread::spawn(move || {
        for _ in 0..2 {
            let Ok((mut stream, _)) = listener.accept() else { return };
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
            let body: Vec<u8> = if line.contains("/download") {
                addon.clone()
            } else {
                release.clone().into_bytes()
            };
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
            let _ = stream.flush();
        }
    });
    (base, handle)
}

#[test]
fn install_places_both_packs_and_enables_them_in_a_world() {
    let root = tempfile::tempdir().unwrap();
    let world = root.path().join("minecraftWorlds/Test");
    std::fs::create_dir_all(world.join("db")).unwrap();
    std::fs::write(world.join("levelname.txt"), "Test").unwrap();
    write_level_dat(&world.join("level.dat"), 0); // gametest = 0

    let addon = build_mcaddon_bytes(); // the same synthetic archive as the core tests
    let (base, server) = stub_github(addon);

    let out = bin()
        .env("CONSTRUCT_GITHUB_API", &base)
        .args(["install", "--world", "Test", "--json",
               "--com-mojang", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    server.join().unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));

    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["version"], "1.2.0");
    assert_eq!(v["world"], "Test");
    assert_eq!(v["beta_apis"], true);

    assert!(root.path().join("development_behavior_packs/Construct[BP]/manifest.json").is_file());
    assert!(root.path().join("development_resource_packs/Construct[RP]/manifest.json").is_file());

    let enabled = std::fs::read_to_string(world.join("world_behavior_packs.json")).unwrap();
    assert!(enabled.contains("8c0c0153-d8b9-482a-889f-aef922b8fe58"));
    let enabled_rp = std::fs::read_to_string(world.join("world_resource_packs.json")).unwrap();
    assert!(enabled_rp.contains("375ec465-3dc1-429f-8b4c-a337889e1ed4"));
}

#[test]
fn install_reports_an_unreachable_github_without_touching_anything() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("minecraftWorlds")).unwrap();
    let out = bin()
        // Port 1 refuses immediately on every platform we target.
        .env("CONSTRUCT_GITHUB_API", "http://127.0.0.1:1")
        .args(["install", "--com-mojang", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(!root.path().join("development_behavior_packs").exists());
}
```

`write_level_dat` and `build_mcaddon_bytes` are test helpers; write them once in `cli.rs` and
reuse. `build_mcaddon_bytes` produces the same shape as Task 15's `real_shaped_addon`, in
memory. Add `zip` and `nbtx` to `construct-cli`'s `[dev-dependencies]`.

- [ ] **Step 2: Run and watch it fail**

```bash
cargo test -p construct-cli --test cli install
```

- [ ] **Step 3: Add the subcommand**

```rust
    /// Download and install Construct.
    Install {
        /// A specific version, e.g. 1.2.0. Defaults to the latest release.
        #[arg(long, value_name = "VERSION")]
        version: Option<String>,
        /// Also enable Construct in this world and turn Beta APIs on.
        #[arg(long, value_name = "WORLD")]
        world: Option<String>,
    },
```

- [ ] **Step 4: Write the command**

```rust
pub fn run(
    releases: &dyn Releases,
    version: Option<&str>,
    world: Option<&World>,
    installation: &Installation,
    backups: &Backups,
    force: bool,
    out: &mut Out,
) -> Result<()> {
    let release = releases.release(version)?;
    let asset = releases::asset_for(&release)?;
    out.line(format!("Construct {} — {}", release.tag, asset.name));

    let staging = tempfile::tempdir()?;
    let archive = staging.path().join(&asset.name);
    releases.download(asset, &archive)?;
    let extracted = mcaddon::extract(&archive)?;

    let bp = install::place(&pack::behavior_root(&installation.dev_pack_root), &extracted.behavior, force)?;
    let rp = install::place(&pack::resource_root(&installation.dev_pack_root), &extracted.resource, force)?;

    for placed in [&bp, &rp] {
        match (placed.changed, placed.from) {
            (false, _) => out.line(format!("  {} already current", placed.dir.display())),
            (true, Some(from)) => out.line(format!(
                "  {} {} → {}",
                placed.dir.display(),
                manifest::version_string(from),
                manifest::version_string(placed.to)
            )),
            (true, None) => out.line(format!(
                "  {} installed at {}",
                placed.dir.display(),
                manifest::version_string(placed.to)
            )),
        }
    }
    if bp.preserved > 0 {
        out.line(format!("  kept {} imported structure(s)", bp.preserved));
    }

    // Everything below this line is the `--world` half.
    let mut level_dat_error = None;
    let mut beta_apis = None;
    if let Some(world) = world {
        worldpacks::upsert(
            &worldpacks::behavior_path(world),
            worldpacks::PackRef { pack_id: pack::CONSTRUCT_BP_UUID.to_string(), version: bp.to },
        )?;
        worldpacks::upsert(
            &worldpacks::resource_path(world),
            worldpacks::PackRef { pack_id: pack::CONSTRUCT_RP_UUID.to_string(), version: rp.to },
        )?;
        out.line(format!("  enabled in {}", world.display_name));

        let level = world.path.join("level.dat");
        // The packs are already in place; from here a failure is partial, not total.
        match backup::file(&level, &world.qualified(), backups)
            .and_then(|_| leveldat::apply_beta_apis(&level, true))
        {
            Ok(change) => {
                beta_apis = Some(change.after);
                out.line("  Beta APIs on");
            }
            Err(e) => {
                out.warn(format!("could not turn Beta APIs on: {e}"));
                level_dat_error = Some(e.to_string());
            }
        }
    }
    out.line("Reload the world before Construct appears.");

    out.emit(Payload {
        version: manifest::version_string(bp.to),
        tag: release.tag,
        behavior: bp.dir.display().to_string(),
        resource: rp.dir.display().to_string(),
        preserved: bp.preserved,
        world: world.map(|w| w.display_name.clone()),
        beta_apis,
        level_dat_error: level_dat_error.clone(),
    });

    // §11: a failed level.dat write exits 5 with the packs installed, naming
    // the remaining manual step — not total failure. Exiting here rather than
    // returning an error keeps the success payload above intact, the same way
    // main.rs already handles the `-o` usage error.
    if level_dat_error.is_some() {
        eprintln!(
            "\nThe packs are installed. Turn Beta APIs on yourself, in the world's settings \
             under Experiments, or with:\n  construct experiment {} --beta-apis on",
            world.map(|w| w.display_name.as_str()).unwrap_or("<world>")
        );
        std::process::exit(5);
    }
    Ok(())
}
```

- [ ] **Step 5: Wire it in**

In `main.rs`, build the client from the environment:

```rust
        Command::Install { version, world } => {
            let w = world.as_deref().map(resolve_world).transpose()?;
            let installation = match &w {
                Some(w) => discovery::installation::for_world(&installations, w)?,
                None => discovery::installation::choose(
                    &installations,
                    std::env::var("CONSTRUCT_INSTALLATION").ok().as_deref(),
                    loaded.config.default_installation.as_deref(),
                )?,
            };
            let token = std::env::var("CONSTRUCT_GITHUB_TOKEN")
                .or_else(|_| std::env::var("GITHUB_TOKEN"))
                .ok();
            let client = match std::env::var("CONSTRUCT_GITHUB_API") {
                Ok(base) => releases::GitHub::with_base(base, token),
                Err(_) => releases::GitHub::new(token),
            };
            commands::install::run(
                &client, version.as_deref(), w.as_ref(), installation,
                &loaded.config.backups, cli.force, out,
            )
        }
```

- [ ] **Step 6: Run and commit**

```bash
cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add crates/construct-cli
git commit -m "install Construct"
```

---

### Task 19: `status`

**Files:**
- Create: `crates/construct-cli/src/commands/status.rs`
- Modify: `crates/construct-cli/src/commands/mod.rs`, `src/cli.rs`, `src/main.rs`
- Modify: `crates/construct-cli/tests/cli.rs`

**Interfaces:**
- Consumes: `pack::find_by_uuid`, `worldpacks::read`, `install::releases::Releases`, `discovery::enumerate`.
- Produces: `commands::status::run(releases: &dyn Releases, installation: &Installation, worlds: &[World], out: &mut Out) -> Result<()>`

§10: "`status` reports installed version, latest available, and which worlds have Construct
enabled." Reading pack lists is a few small JSON files per world; **no database is opened**.

**Offline is not a failure.** `status` reports what it can see locally and warns that the latest
version could not be checked. A user on a plane still deserves to know what they have installed.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn status_reports_the_installed_version_and_which_worlds_have_it() {
    let root = world_with_construct(&[]);
    let world = root.path().join("minecraftWorlds/Test");
    std::fs::write(
        world.join("world_behavior_packs.json"),
        r#"[{"pack_id":"8c0c0153-d8b9-482a-889f-aef922b8fe58","version":[1,2,0]}]"#,
    )
    .unwrap();

    let out = bin()
        .env("CONSTRUCT_GITHUB_API", "http://127.0.0.1:1")
        .args(["status", "--json", "--com-mojang", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success(), "offline must not fail: {}", String::from_utf8_lossy(&out.stderr));

    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["installed"], "1.2.0");
    assert_eq!(v["latest"], serde_json::Value::Null);
    assert_eq!(v["enabled_worlds"][0], "Test");
    assert!(
        v["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("latest")),
        "the payload must say why latest is missing: {v}"
    );
}

#[test]
fn status_without_construct_points_at_install() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("minecraftWorlds")).unwrap();
    let out = bin()
        .env("CONSTRUCT_GITHUB_API", "http://127.0.0.1:1")
        .args(["status", "--com-mojang", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&out.stderr).contains("construct install"));
}

#[test]
fn a_world_without_construct_enabled_is_not_listed() {
    let root = world_with_construct(&[]);
    // No world_behavior_packs.json at all.
    let out = bin()
        .env("CONSTRUCT_GITHUB_API", "http://127.0.0.1:1")
        .args(["status", "--json", "--com-mojang", root.path().to_str().unwrap()])
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["enabled_worlds"].as_array().unwrap().len(), 0);
}
```

- [ ] **Step 2: Run and watch it fail**

```bash
cargo test -p construct-cli --test cli status
```

- [ ] **Step 3: Implement**

```rust
pub fn run(
    releases: &dyn Releases,
    installation: &Installation,
    worlds: &[World],
    out: &mut Out,
) -> Result<()> {
    let bp_root = pack::behavior_root(&installation.dev_pack_root);
    let installed = pack::find_by_uuid(&bp_root, pack::CONSTRUCT_BP_UUID)
        .ok_or_else(|| CoreError::ConstructNotInstalled { searched: vec![bp_root.clone()] })?;
    let version = manifest::version_string(installed.manifest.version);

    // Offline is not a failure: report what is here and say what could not be
    // checked. Both channels get it — stderr now, payload for a caller.
    let latest = match releases.release(None) {
        Ok(r) => Some(r.tag),
        Err(e) => {
            out.warn(format!("could not check the latest version: {e}"));
            None
        }
    };

    // Which worlds have it enabled: a few small JSON files, no database.
    let enabled: Vec<String> = worlds
        .iter()
        .filter(|w| w.installation == installation.name)
        .filter(|w| {
            worldpacks::read(&worldpacks::behavior_path(w))
                .map(|packs| packs.iter().any(|p| p.pack_id == pack::CONSTRUCT_BP_UUID))
                .unwrap_or(false)
        })
        .map(|w| w.display_name.clone())
        .collect();

    out.line(format!("Construct {version} at {}", installed.dir.display()));
    match &latest {
        Some(tag) if tag.trim_start_matches('v') == version => out.line("  up to date"),
        Some(tag) => out.line(format!("  {tag} is available: construct install")),
        None => {}
    }
    if enabled.is_empty() {
        out.line("  enabled in no worlds");
    } else {
        out.line(format!("  enabled in: {}", enabled.join(", ")));
    }

    out.emit(Payload {
        installation: installation.name.clone(),
        installed: version,
        pack: installed.dir.display().to_string(),
        latest,
        enabled_worlds: enabled,
    });
    Ok(())
}
```

Wire `Command::Status` into `main.rs` using the same installation resolution and releases client
as `install`.

- [ ] **Step 4: Run and commit**

```bash
cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --all
git add crates/construct-cli
git commit -m "status: what is installed, what is available, where it is on"
```

---

### Task 20: Documentation and the checklist

**Files:**
- Modify: `README.md`
- Modify: `docs/manual-verification.md`
- Rewrite: `docs/carried-forward.md`

**Interfaces:** none — this task ships no code.

- [ ] **Step 1: Extend the README**

The README leads with the workflow being replaced and must now cover the second one. Add, in the
project's existing voice:

- `construct install [--world W]` — what it does, and that it turns Beta APIs on.
- `construct import <file> [--world W] [--name N]`, `copy`, and `delete --source pack`, each with
  the "reload the world" caveat.
- `construct status` and `construct experiment <world> --beta-apis [on|off]`.
- The environment variables this stage adds or uses: `CONSTRUCT_INSTALLATION`,
  `CONSTRUCT_GITHUB_TOKEN` (falling back to `GITHUB_TOKEN`), and `CONSTRUCT_GITHUB_API` as a
  testing and enterprise-proxy escape hatch.
- One honest sentence that `delete` currently handles pack structures only, and that removing a
  structure from a world's database is stage 4.

- [ ] **Step 2: Add the manual-verification items**

`docs/manual-verification.md` gains the §16 items this stage makes checkable, unticked:

```markdown
- [ ] **Does Construct pick up an imported structure after a world reload?**
      `construct import <file> --world W`, reload, and look for it in Construct's list.
- [ ] **Does the level.dat Beta APIs flip register in-game?**
      `construct experiment <world> --beta-apis on`, then check the world's Experiments
      settings. The command already proves the file round-trips; only the game proves it
      is honoured.
- [ ] **Does a .mcstructure in `structures/<Namespace>/` load as `<namespace>:<name>`?**
      The flat form is settled by Construct's own source (§17); the nested form is inferred
      from one shipped pack and is what `import --name ns:name` writes.
- [ ] **Does `construct install` produce a working Construct?**
      Install into a scratch world, load it, and run `/construct`.
- [ ] *(Windows)* **Do dev packs in `Users\Shared` apply to a world owned by a specific account?**
```

- [ ] **Step 3: Rewrite `docs/carried-forward.md`**

Everything the stage-1 file listed under "Correctness, narrow" and "Quality and performance" is
either fixed by Task 1 or superseded. Replace the file with what **stage 2** carries forward,
and keep the one entry that is still live:

- The stage-4 prerequisite: `guard_test_path` belongs inside `BedrockStore::open`, and moving it
  needs the parent canonicalized first. Stage 2 adds no leveldb write, so it is still deferred —
  but it is now the last thing standing between stage 4 and a live-world open.
- Anything the review loop parked while executing this plan, with its ruling.

- [ ] **Step 4: Commit**

```bash
git add README.md docs
git commit -m "Document the Construct integration"
```

---

### Task 21: Structures nest deeper than one level

Added mid-execution. `docs/bedrock-mcstructure-files.md` — a local, untracked copy of
tryashtar's third-party documentation of the Bedrock `.mcstructure` format and its loading
rules, published on GitHub (github.com/tryashtar) — settles §17 and corrects an assumption Task 5 shipped.

**The rule, from that document:**

| Path under the pack | Identifier |
|---|---|
| `structures/house.mcstructure` | `mystructure:house` |
| `structures/dungeon/entrance.mcstructure` | `dungeon:entrance` |
| `structures/stuff/towers/diamond.mcstructure` | `stuff:towers/diamond` |

**The first subfolder is the namespace; every folder after it is part of the name.** Task 5's
`structures::list` walks exactly one level and silently ignores anything deeper, on the reasoning
that nothing deeper was addressable in-game. That reasoning is now known to be wrong, so a
structure at `structures/stuff/towers/diamond.mcstructure` is invisible to `list` and unreachable
by `export`, `copy`, and `delete`.

**Files:**
- Modify: `crates/construct-core/src/pack/structures.rs`
- Modify: `crates/construct-core/tests/pack.rs`

**Interfaces:** unchanged. `list`, `path_for`, `write`, `remove`, and `derive_name` keep their
signatures; only `list`'s depth behaviour and the id it derives change.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_structure_nested_below_the_namespace_folder_is_addressable() {
    let root = tempfile::tempdir().unwrap();
    let pack = root.path().join("P");
    touch(&pack.join("structures/stuff/towers/diamond.mcstructure"), b"x");

    let found = structures::list(&pack);
    assert_eq!(found.len(), 1);
    // First subfolder is the namespace; everything after it is part of the name.
    assert_eq!(found[0].id, "stuff:towers/diamond");
    assert_eq!(found[0].name, "stuff:towers/diamond");
}

#[test]
fn depth_does_not_change_the_flat_or_one_level_rules() {
    let root = tempfile::tempdir().unwrap();
    let pack = root.path().join("P");
    touch(&pack.join("structures/house.mcstructure"), b"x");
    touch(&pack.join("structures/Understudy/players.mcstructure"), b"x");
    touch(&pack.join("structures/a/b/c/d.mcstructure"), b"x");

    let ids: Vec<String> = structures::list(&pack).into_iter().map(|s| s.id).collect();
    assert!(ids.contains(&"mystructure:house".to_string()));
    assert!(ids.contains(&"understudy:players".to_string()));
    assert!(ids.contains(&"a:b/c/d".to_string()));
}

#[test]
fn only_the_namespace_segment_is_lowercased() {
    // Minecraft namespaces are lowercase, but the path after the namespace is
    // part of the name and is left exactly as it sits on disk.
    let root = tempfile::tempdir().unwrap();
    let pack = root.path().join("P");
    touch(&pack.join("structures/Stuff/Towers/Diamond.mcstructure"), b"x");
    assert_eq!(structures::list(&pack)[0].id, "stuff:Towers/Diamond");
}

#[test]
fn writing_still_refuses_a_separator_in_a_name() {
    // Reading and writing stay asymmetric on purpose (§17): `list` reports whatever
    // depth exists, but nothing this tool writes creates a nested path, because the
    // character that would enable it is the one that makes traversal possible.
    assert!(structures::path_for(Path::new("/p"), "stuff:towers/diamond").is_err());
    assert!(structures::derive_name("towers/diamond").is_err());
}
```

- [ ] **Step 2: Run them and watch them fail**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -p construct-core --test pack
```

Expected: the first three fail — today `list` never descends past one level, so the nested files
are simply absent from its output. The fourth should already pass; it is there to pin the
asymmetry so a later change cannot quietly relax `path_for` while widening `list`.

- [ ] **Step 3: Make `list` walk to full depth**

Replace the two-pass structure with a recursive walk that carries the path relative to
`structures/`. For each `.mcstructure` file found at a relative path:

- no directory component → `mystructure:<stem>`
- one or more components → `<first component lowercased>:<rest joined with '/' >/<stem>`, i.e. the
  first component is the namespace and the remainder of the path, including the file stem, is the
  name.

Keep the existing sort by id, keep `size_bytes` from the entry metadata, and keep `name` as
`key::display_name(&id)` so the `mystructure:` prefix is still stripped for display and any other
namespace stays visible.

On Windows the relative path's components must be joined with `/` regardless of the platform
separator, since the identifier is a Minecraft id and not a filesystem path.

- [ ] **Step 4: Run the tests**

```bash
cargo test -p construct-core --test pack
cargo test --workspace
```

Every earlier `structures::list` test must still pass unchanged — the flat and one-level rules are
unchanged, and this only adds depth below them.

- [ ] **Step 5: Commit**

```bash
git add crates/construct-core
git commit -m "Structures nest deeper than one level"
```

---

## Self-Review

Run against the spec after the plan was written.

**Spec coverage.** §14 stage 2 names `pack` (Tasks 2, 3, 5), `catalog` (Tasks 1, 6),
`install` (Tasks 14-18), `status` (19), `experiment` (12, 13), `import` (8), `copy` (9), and
`delete --source pack` (10). §8's `level.dat` backup requirement lands in Task 11 — §14's stage
list does not mention `backup`, but §8 requires it for `level.dat` writes, so it is in scope
here rather than stage 4. §17's naming question is settled in the Global Constraints and
Task 5, with the unverified half on the §16 checklist (Task 20).

**Two gaps in the spec, ruled on rather than deferred.** The spec names neither an HTTP client
nor a zip library. `ureq` 3.4 and `zip` 8.6 (minimal features) were chosen, resolved, and built
against this toolchain while writing this plan — no async runtime, pure Rust, both recorded in
Task 14. If a reviewer prefers different crates, that is a decision to make before Task 14, not
after.

**One deliberate deviation.** `delete` ships with its world-source form refused at exit 2. §5
describes `delete <world> <structure>` without qualification, but §14 puts the leveldb write in
stage 4. Shipping the pack half now with an honest refusal is what §14's own note asks for:
"deletion arrives in stage 2 for pack structures, so users get the capability long before the
risky path exists."

**Type consistency.** `Entry` gains `path: Option<PathBuf>` in Task 1 and every later task that
constructs one passes it. `Source` gains `Ord` in Task 1 because `catalog::sort` needs it.
`commands::catalog::for_world` (CLI) and `pack::for_world` (core) share a name in different
modules — always call them module-qualified.

**Task 21 was added mid-execution**, after `docs/bedrock-mcstructure-files.md` — tryashtar's
third-party `.mcstructure` documentation on GitHub, kept locally but not committed — settled §17
and showed Task 5's one-level-deep assumption to be wrong. It is listed last because it corrects
a shipped behaviour rather than blocking anything after it.

**Task 7 writes logic that Task 9 extracts.** That is intentional: the second caller is what
earns the extraction, and doing it earlier would be designing an interface for one user.
