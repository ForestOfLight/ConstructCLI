# ConstructCLI — Design

**Date:** 2026-08-31
**Status:** Approved design, ready for implementation planning

## 1. Problem

Moving structures between Minecraft Bedrock worlds currently requires uploading a whole
world to a website and shuffling files by hand. Construct's own README documents the
workflow:

- To **export** structures from a world: upload the world to <https://holoprint-mc.github.io/>
  and use its "Extract From World" feature.
- To **import** a structure: manually move `Construct[BP]` into
  `com.mojang/development_behavior_packs`, drop the `.mcstructure` into
  `Construct[BP]/structures`, and restart the world.

ConstructCLI replaces both with local commands. It is also structured so that a larger
GUI can be built on it later without rewriting the logic.

Construct is the addon this tool serves: <https://github.com/ForestOfLight/Construct>,
MIT, a JavaScript Bedrock addon for survival building.

## 2. Goals and non-goals

**Goals**

- Install and upgrade Construct from its GitHub releases.
- List, export, import, copy, and delete structures across worlds.
- Export several structures merged into a single `.mcstructure`.
- Work on macOS, Linux, and Windows.
- Keep all logic in a library that a future GUI can consume.

**Non-goals**

- Driving Construct's in-game features (its `construct:` command namespace is unrelated).
- Editing world terrain, chunks, entities, or anything outside structure storage.
- Managing packs other than Construct.
- A GUI in this project.

## 3. Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Relationship to StructureChest | Supersedes it | Clean reset; StructureChest is reference material for leveldb key conventions only |
| Language | Rust | Sum types and `Result` fit binary-format parsing; single static binary; only candidate whose core can also target WASM for a future web UI |
| Layout | Cargo workspace: `construct-core` lib + `construct-cli` bin | A future GUI links the library rather than reimplementing it |
| LevelDB backend | `bedrock_level` from `bedrock-rs` (FFI to the Mojang leveldb fork), behind a `StructureStore` trait | Writes go through the same C++ implementation Minecraft uses rather than a reimplementation; the trait leaves room for a pure-Rust backend later |
| NBT | `nbtx` 3.0.1 (little-endian) | Published to crates.io; little-endian is `.mcstructure`'s encoding |
| Platforms | macOS, Linux, Windows; auto-discovery plus overrides | Windows is where most Construct users are |
| Merge semantics | Reassemble at each structure's recorded `structure_world_origin` | Pieces of one build reassemble into that build |
| Write safety | Snapshot `db/` before every write; refuse when the world is in use | The leveldb write path is young relative to the blast radius |
| Install scope | Packs, optional `--world` enable, attempt Beta APIs flip, separate experiment command | Removes the most friction without hiding a `level.dat` write inside an install |
| Upgrade UX | Idempotent `install` plus a `status` command | One code path; no ambiguity about repeat installs |
| Binary name | `construct` | The addon's commands are in-game and namespaced; no practical collision |
| License | MIT | Matches Construct; compatible with the Apache-2.0 dependency |

### Backend selection, in more detail

Two Rust options were compared:

| | `bedrock-leveldb` | `bedrock-rs` / `bedrock_level` |
|---|---|---|
| Repo | <https://github.com/BE-Community-Dev/bedrock-leveldb> | <https://github.com/bedrock-crustaceans/bedrock-rs> |
| Stars / forks | 3 / 0 | 186 / 28 |
| Created | Apr 2026 | May 2024 |
| License | NOASSERTION | Apache-2.0 |
| LevelDB | Pure-Rust reimplementation | FFI to the Mojang leveldb fork, vendored in `leveldb-sys` |
| Published | crates.io, semver | Unpublished, untagged |
| Build | Pure Rust | CMake + C++ toolchain |

`bedrock_level` wins on the axis that matters most: its writes go through Mojang's own
leveldb rather than a four-month-old reimplementation of LevelDB's table and manifest
writers. Its `DatabaseAccess` trait (`open` / `get` / `insert` / `remove`) operates on raw
byte keys, which is exactly what `structuretemplate_` keys need — its chunk-oriented `Key`
type is bypassed entirely. `bedrock-leveldb`'s unresolved license is a separate blocker for
a distributed tool.

`bedrock-leveldb` retains one future advantage worth preserving the trait for: its
read-only mode never acquires the leveldb LOCK, and it compiles to WASM.

## 4. Architecture

```
ConstructCLI/
├── Cargo.toml              # workspace
├── crates/
│   ├── construct-core/     # all logic; no CLI concepts
│   └── construct-cli/      # binary `construct`
```

`construct-core` never references arguments, stdout, or exit codes. It returns typed values
and typed errors (`thiserror`) and never panics. `construct-cli` owns `clap`, formatting,
and exit codes, and uses `anyhow` for context chaining.

### Core modules

| Module | Responsibility | Depends on |
|---|---|---|
| `discovery` | Resolve installations, world roots, and dev-pack roots per platform; enumerate worlds | fs |
| `store` | `StructureStore` trait + `bedrock_level` backend; key encode/decode; LOCK detection | `bedrock_level` |
| `backup` | Snapshot a world's `db/` to a timestamped directory; retention | fs |
| `mcstructure` | Codec: bytes ⇄ `Structure` | `nbtx` |
| `merge` | Origin-based reassembly of N `Structure` into one | `mcstructure` |
| `pack` | Locate Construct, parse `manifest.json`, read/write `structures/` | `bedrock_addon` |
| `install` | Releases lookup, download, unzip, place packs, world enablement, `level.dat` flip | `pack`, `backup` |
| `config` | Load and merge configuration | fs |

### The byte-transparency property

A structure's leveldb *value* is byte-identical to a `.mcstructure` *file*. Therefore
`export`, `import`, `copy`, and `delete` never parse anything — only `merge` needs the
codec.

This confines the riskiest code (NBT parsing, palette unification) to a single feature. A
codec bug can only make `merge` wrong; every other command still works, and every other
command can ship before the codec exists.

### Risk surface by command

| Command | leveldb read | leveldb write | Other writes |
|---|---|---|---|
| `worlds`, `list`, `export` | yes | — | — |
| `status` | — | — | — |
| `import` | — | — | pack `structures/` |
| `copy` | yes (source) | — | pack `structures/` |
| `install` | — | — | packs, pack JSON, `level.dat` |
| `experiment` | — | — | `level.dat` |
| **`delete`** | yes | **yes** | — |

`delete` is the only command that writes to a leveldb.

## 5. Command surface

```
construct worlds                                    # enumerate discovered worlds
construct status                                    # Construct: installed version, latest, enabled worlds
construct list <world>                              # structures in a world
construct export <world> <structure> [-o FILE]
construct export <world> <s1> <s2>... --merge -o FILE
construct import <file> [--world W]                 # into Construct's structures/
construct copy <src-world> <structure> <dst-world>
construct delete <world> <structure>
construct install [--version V] [--world W]
construct experiment <world> --beta-apis <on|off>
```

Global flags: `--com-mojang <path>` (repeatable), `--path <dir>` (address a world directly),
`--json` where output is structured.

Naming rules. A single `export` without `-o` writes `<structure-name>.mcstructure` into the
current directory; `--merge` requires `-o`. `import` derives the structure name from the
file stem, so `house.mcstructure` becomes `house`. Structure references are bare names,
meaning the `mystructure` prefix, or explicit `prefix:name`.

`import` and `copy` write into Construct's `structures/` folder — never into a leveldb.
With `--world W` they target `<world>/behavior_packs/Construct[BP]/structures/` when that
world has a local copy of Construct, otherwise the installation's shared
`development_behavior_packs` copy, saying which was chosen. Both commands state that the
world must be reloaded before Construct sees the structure, and both fail clearly when the
destination world has no Construct, pointing at `construct install --world <dst>`.

## 6. Discovery and multiple roots

Discovery resolves a list of *installations*. Each has one dev-pack root and zero or more
world roots. Roots that do not exist are absent, with no error.

| Installation | Dev-pack root | World roots |
|---|---|---|
| `release` (GDK) | `%appdata%\Minecraft Bedrock\Users\Shared\games\com.mojang` | `%appdata%\Minecraft Bedrock\Users\*\games\com.mojang\minecraftWorlds` |
| `preview` (GDK) | same under `Minecraft Bedrock Preview` | same under `Minecraft Bedrock Preview` |
| `legacy` (UWP) | `%localappdata%\Packages\Microsoft.MinecraftUWP_8wekyb3d8bbwe\LocalState\games\com.mojang` | that root's `minecraftWorlds` |
| `mcpelauncher` | `~/Library/Application Support/mcpelauncher/games/com.mojang` (macOS), `~/.local/share/mcpelauncher/games/com.mojang` (Linux) | that root's `minecraftWorlds` |

Windows moved from UWP to GDK in Minecraft **1.21.120**. Under GDK, worlds live per Xbox
account and development packs live in `Users\Shared` — so worlds and dev packs are in
*different* roots, and there may be several world roots on one machine. Legacy UWP is still
probed for worlds, because pre-migration worlds are exactly the ones worth mining, but
Construct itself requires `min_engine_version [1,26,40]` and so only ever runs on GDK.

Sources: [GDK Migration on Windows](https://learn.microsoft.com/en-us/minecraft/creator/documents/gdkpcprojectfolder?view=minecraft-bedrock-stable),
[com.mojang — Minecraft Wiki](https://minecraft.wiki/w/Com.mojang).

### World references

A world reference is a display name (from `levelname.txt`) or a folder name. Qualification
is only needed when ambiguous, and the error supplies the qualified form:

```
$ construct list "Test World"
error: "Test World" matches 3 worlds

  release/Shared/Ssu8ww1SFbM=         4 MB   3 weeks ago
  release/2533274801234567/aBc0=     12 MB   yesterday
  preview/Shared/XyZ9=                1 MB   2 months ago

Use a qualified reference:
  construct list release/Shared/Ssu8ww1SFbM=
```

Qualified form is `<installation>/<account>/<world>`, each segment optional from the left.
The account segment is omitted from output when an installation has only one world root, so
macOS and Linux never display it. Folder names are matched before display names. Ambiguity
is always an error, never a silent pick.

Cross-root operations are first-class:
`construct copy legacy/OldWorld house release/My Survival`. Source and destination resolve
independently. `install --world W` derives the installation from W and deploys to *that*
installation's dev-pack root, so `preview` and `release` are never mixed.

## 7. Configuration

`~/.config/constructcli/config.toml` and platform equivalents via the `directories` crate:

```toml
default_installation = "release"
extra_roots = ["D:/MinecraftBackups/com.mojang"]

[backups]
dir = "/Volumes/Spare/construct-backups"   # optional; defaults to the platform data dir
keep = 10
```

Precedence: CLI flag → environment variable → config file → auto-discovery. An absent file
means all defaults, so there is no init step. Unknown keys warn rather than fail.

## 8. Write safety

Reads (`worlds`, `list`, `export`) open the database directly. LevelDB's C++ API acquires
`db/LOCK` on every open — there is no read-only open — so a world open in Minecraft cannot
be read directly. On a lock error, read-only commands automatically copy `db/` to a
temporary directory and open the copy. This is not gated behind a flag, but it announces
itself and checks free space first, because a large survival world's `db/` can reach several
gigabytes:

```
world in use — reading from a 2.4 GB snapshot…
```

LevelDB writes (`delete` only) follow a fixed sequence:

1. Resolve the world.
2. Open the database. **If the LOCK is held, stop.** No `--force`; a concurrent writer is
   how worlds get corrupted.
3. Snapshot `db/` to the backup directory.
4. Mutate.
5. Drop the handle to flush.
6. Report the backup path in the success message.

Backups live outside the world folder, in the configured backup directory, with retention
of the last `keep` per world. A `db.backup-*` folder inside a world directory would confuse
Minecraft, bloat the world, and ride along into any world export.

`level.dat` writes (`install --world`, `experiment`) copy the file to the same backup
directory before modifying it. Minecraft's own `level.dat_old` is not a substitute — it is
overwritten by the game on its own schedule.

## 9. The `.mcstructure` codec and merge

`Structure` models the format: `format_version`, `size`, `structure_world_origin`, two
`block_indices` layers, `block_palette`, `block_position_data`, and entities. Encoding is
little-endian NBT.

Merge:

1. Fetch N values and decode.
2. Union the bounding boxes derived from each `structure_world_origin` and `size`.
3. Unify palettes by deduplicating `(name, states, version)`, building a per-source remap.
4. Blit each piece's two index layers into the output grid.
5. Translate `block_position_data` keys and entity positions into the new frame.
6. Encode.

Gaps between pieces are index **-1** (structure void), not air. Void leaves existing terrain
untouched on placement; air would carve holes in whatever the structure is placed over.

Merge refuses when every `structure_world_origin` is zero or identical — the pieces carry no
usable origin and would stack in one spot. It also refuses when the union bounding box is
large enough to exhaust memory; this is an allocation guard, not a game limit. Minecraft
loads structures beyond structure-block dimensions without trouble, so oversized results
produce a **performance warning**, not a refusal.

## 10. Install

1. Query the releases API for `latest`, or `tags/v<version>` with `--version`.
2. Match the `Construct-v*.mcaddon` asset by pattern.
3. Download to a temporary directory.
4. Unzip; identify BP versus RP by reading each `manifest.json`'s module types.
5. Place into `development_behavior_packs` / `development_resource_packs`, matching any
   existing install **by header UUID rather than folder name**.
6. With `--world`, upsert both pack IDs into that world's `world_behavior_packs.json` and
   `world_resource_packs.json`.
7. Attempt the Beta APIs flip in that world's `level.dat`.

**Upgrading must not delete `Construct[BP]/structures/`.** That folder holds the user's
imported structures — the very data this tool exists to put there. A naive delete-and-unzip
would destroy it. Install preserves it across upgrades and reports what it carried over.

`install` is idempotent: it detects an existing Construct by UUID, reports `v1.1.0 → v1.2.0`,
and no-ops when already current. `status` reports installed version, latest available, and
which worlds have Construct enabled.

A failed `level.dat` write exits 5 with the packs already installed, naming the remaining
manual step, rather than reporting total failure.

Known Construct coordinates: BP header UUID `8c0c0153-d8b9-482a-889f-aef922b8fe58`,
RP dependency UUID `375ec465-3dc1-429f-8b4c-a337889e1ed4`, `min_engine_version [1,26,40]`,
Beta APIs experiment required.

## 11. Error handling

Exit codes: `0` success · `1` failure · `2` usage error · `3` not found · `4` world in use ·
`5` partial success.

Every message names the thing, says why, and gives the next action.

| Failure | Handling |
|---|---|
| No `com.mojang` found | List every path probed; point at `--com-mojang` |
| World not found | Suggest near matches |
| World name ambiguous | Disambiguation table with qualified references |
| World in use | Reads snapshot and continue; `delete` stops hard |
| Structure not found | Suggest near matches from the prefix scan already in hand |
| Structure prefix | Bare `name` means `mystructure:name`; `prefix:name` accepted explicitly |
| Codec failure | Report file, field, and byte offset |
| Degenerate merge origins | Refuse, explaining that origins are unusable |
| Oversized merge | Warn about performance; refuse only on allocation limits |
| Construct not installed | Point at `install` |
| Two Construct copies | State which was chosen and why |
| GitHub rate limit | Name the 60/hour unauthenticated limit; optional token via env var |
| No asset for `--version` | List available assets |
| `level.dat` write fails | Exit 5; packs installed; name the manual step |

## 12. Testing

**Pure logic, no I/O.** Codec, palette unification, merge geometry, key encode/decode,
world-reference resolution, and version comparison are pure functions over bytes and structs.
Committed `.mcstructure` fixtures cover: a single block, a multi-block with block entities, a
waterlogged block (second index layer), one with entities, and one with void gaps.

**Two invariants as properties:**

- *Byte transparency* — `export` → `import` → `export` yields identical bytes. If this fails,
  the claim that most commands avoid the codec is false.
- *Merge identity* — merging one structure yields it unchanged; and for any world coordinate,
  the merged result holds whichever source had a non-void block there.

Merge results are compared **semantically** on decoded structures. NBT key ordering is not
guaranteed stable, so golden-file byte comparison would produce meaningless failures.

**Database tests run on a disposable copy** — a tarred fixture world extracted per test, the
pattern `bedrock-rs` uses for `crates/level/tests/level.tar.gz`. A guard helper refuses any
database path outside the temp directory. Plus lock detection, snapshot-before-delete, and
restore-from-snapshot.

**Install is fully mockable** with a stubbed releases endpoint and a synthetic `.mcaddon`.
The highest-priority test is upgrade-preserves-`structures/`. Alongside it: UUID matching
over folder names, pack-JSON upsert idempotence, and `level.dat` failure producing exit 5.

**Discovery is table-driven over synthetic trees** built in temp directories: GDK with two
accounts, GDK with Preview alongside release, legacy UWP, mcpelauncher. Assert resolved
installations, ambiguity errors, and qualified-reference forms.

CI runs macOS, Linux, and Windows. CMake is present on all three runners, so the
`leveldb-sys` build is exercised everywhere from the start.

**Spike first.** Before anything else is built: point `bedrock_level` at a copy of a real
world, enumerate `structuretemplate_` keys, and round-trip one structure out and back into
the copy. This validates the core dependency choice in about an hour. If it fails, revisit
the backend decision before building on top of it.

## 13. Build and distribution

Rust stable, edition 2024 (matching `bedrock-rs`), plus **CMake and a C++ compiler** for
`leveldb-sys`. `leveldb-sys` vendors the leveldb C++ source in-repo under `ffi/leveldb/`
rather than as a submodule, so a plain clone builds — confirm on first clone.

`bedrock_level` is a git dependency **pinned to an explicit commit hash**, never a floating
branch, because `bedrock-rs` has no tags or crates.io release. Upgrades are deliberate and
re-run the spike. Apache-2.0 permits vendoring if the project stalls; the `StructureStore`
trait keeps a backend swap from being a rewrite. `nbtx` comes from crates.io.

Releases are built by a GitHub Actions matrix: macOS arm64 and x86_64, Windows x86_64 (MSVC),
Linux x86_64 (glibc — musl plus C++ is not worth the fight). The C++ leveldb links
statically, so each artifact is a single file of a few megabytes. `cargo install --git` also
works for anyone with CMake.

Scaffolding includes `git init`, the workspace, CI, and a README leading with the workflow
being replaced.

## 14. Implementation sequencing

The scope here is larger than one sitting, and the byte-transparency property makes it
cleanly separable. Suggested stages, each independently useful:

0. **Spike** — `bedrock_level` against a copy of a real world. Throwaway. Gates everything.
1. **Read path** — workspace scaffolding, `config`, `discovery` (including multi-root and
   world references), `store` reads, and `worlds` / `list` / `export`. Delivers the half of
   the tool that replaces holoprint, and touches nothing destructive.
2. **Construct integration** — `pack`, `install`, `status`, `experiment`, `import`, `copy`.
   Delivers the other documented manual workflow. Writes files and `level.dat`, but no
   leveldb.
3. **Merge** — `mcstructure` codec and `merge`, behind `export --merge`. The only stage
   needing the codec; fully testable against fixtures.
4. **Delete** — `backup` plus `store` writes and the `delete` command. Deliberately last:
   the only leveldb write, and the only stage that can damage a world.

Stage 4 could be dropped entirely without affecting anything else, which is worth
remembering if `bedrock_level`'s write path disappoints in the spike.

## 15. Risk register

| Risk | Severity | Mitigation |
|---|---|---|
| Construct upgrade wipes `structures/` | **Data loss** | Preserve across upgrade; highest-priority test |
| `delete` corrupts a world db | **Data loss** | Mojang's leveldb via FFI; snapshot before write; hard refusal on LOCK |
| `bedrock-rs` churns or stalls | High | Commit pin; `StructureStore` trait; Apache-2.0 permits vendoring |
| Windows GDK assumptions unverified | High | Manual checklist on real hardware before release |
| CMake/C++ toolchain friction | Medium | CI on all three platforms; prebuilt binaries for users |
| Construct asset naming changes | Medium | Pattern match; clear error listing available assets |
| GitHub rate limit | Low | Named in the error; optional token |
| `.mcstructure` format version changes | Low | Version checked on decode; explicit error |

The two data-loss rows hold the release. Everything else degrades into a bad afternoon.

## 16. Manual verification checklist

These cannot be settled by automated tests and must be confirmed in the game:

1. Does a merged structure beyond structure-block dimensions load and place correctly?
2. Does Construct pick up an imported structure after a world reload?
3. Does the `level.dat` Beta APIs flip register in-game?
4. Do dev packs in `Users\Shared` apply to a world owned by a specific account? *(Windows)*
5. Do files written into the GDK folder by an ordinary process read back in-game? *(Windows)*

Items 4 and 5 need real Windows hardware. Item 5 is expected to be a non-issue: the ACL
problem was specific to UWP's `LocalState` inside an AppContainer sandbox, and GDK stores
data in ordinary `AppData\Roaming`.

## 17. Assumptions to verify

- Microsoft documents `Users\Shared\...\development_behavior_packs` as the creator
  deployment location, which implies the game reads dev packs from `Shared` regardless of
  the signed-in account. The design depends on this.
- `leveldb-sys` vendors leveldb rather than using a submodule (observed in its file tree).
- A structure's leveldb value is byte-identical to a `.mcstructure` file. Corroborated by
  StructureChest's working implementation and asserted as a test property.
