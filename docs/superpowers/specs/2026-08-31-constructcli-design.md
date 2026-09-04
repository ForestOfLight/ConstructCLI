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
| Backend delivery | Patched forks of `bedrock-rs` and `leveldb-sys`, pinned by commit; fixes offered upstream | Measured: the upstream pins do not compile on macOS at all, or on any non-x86_64 target (§3, "What the spike found") |
| Reading a world | Always copy `db/` first; never open the original | Measured: opening a leveldb database rewrites it (§8) |
| NBT | `nbtx` 3.0.1 (little-endian) | Published to crates.io; little-endian is `.mcstructure`'s encoding |
| Platforms | macOS, Linux, Windows; auto-discovery plus overrides | Windows is where most Construct users are |
| Merge semantics | Reassemble at each structure's recorded `structure_world_origin` | Pieces of one build reassemble into that build |
| Write safety | Snapshot `db/` before every write; refuse when the world is in use | The leveldb write path is young relative to the blast radius |
| Install scope | Packs, optional `--world` enable, attempt Beta APIs flip, separate experiment command | Removes the most friction without hiding a `level.dat` write inside an install |
| Upgrade UX | Idempotent `install` plus a `status` command | One code path; no ambiguity about repeat installs |
| Structure namespace | One namespace over both sources, `--source` to disambiguate | Construct presents a single in-game list; two CLI lists would model it worse |
| Merge overlap | Last argument wins; `--on-overlap` overrides | Overlapping saves of one build are common and often deliberate |
| Collisions | Refuse, `--force` overwrites — uniformly | One rule to learn; nothing silently destroyed |
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

`bedrock-leveldb` retains one advantage worth preserving the trait for: its read-only mode
never acquires the leveldb LOCK, and it compiles to WASM. §8 promotes this from a future
nicety to the thing that would remove a full `db/` copy from every read.

### What the spike found

Stage 0 ran against a real world and returned a split verdict.

Byte transparency holds. A structure read out of a database, written back under a new key,
and read again is byte-identical, and values begin `0x0a` — little-endian NBT. §4's central
claim is now measured rather than argued, and the staging in §14 rests on solid ground.

The dependency does not build. Four independent defects, all in portability handling:

| Defect | Location | Breaks |
|---|---|---|
| `rustc-link-lib=dylib=stdc++` hardcoded under `#[cfg(unix)]` | `leveldb-sys/build.rs` | every macOS target — Apple ships `libc++` |
| `is_x86_feature_detected!`, `target_feature(avx2)`, `std::arch::x86_64`, none behind `cfg(target_arch)` | `bedrock-rs/crates/level/src/greedy.rs` | every non-x86_64 target, at compile time |
| `LEVELDB_PLATFORM_POSIX_SSE` defined unconditionally, then x86 intrinsics under an `#elif __GNUC__` that Apple clang satisfies | vendored `port_posix_sse.cc` | Apple Silicon |
| zlib's `TARGET_OS_MAC` `fdopen` macro clobbering the SDK header; missing `unistd.h` | vendored zlib | modern macOS SDKs |

§13's release matrix names macOS arm64 and x86_64. Both were broken; Linux and Windows
were not. The fixes are small and mechanical, which is why forking beats switching
backends — `bedrock-leveldb`'s licence is still `NOASSERTION`, which no amount of patching
resolves.

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
| `pack` | Locate Construct, parse `manifest.json`, read/write `structures/` | `serde_json` |
| `catalog` | Unify world and pack structures into one namespace; resolve references | `store`, `pack` |
| `install` | Releases lookup, download, unzip, place packs, world enablement, `level.dat` flip | `pack`, `backup` |
| `config` | Load and merge configuration | fs |

`manifest.json` is read with a small local serde struct rather than `bedrock_addon`. It is
roughly twenty lines — StructureChest already demonstrated as much — and it keeps the
unpublished git dependency surface down to `bedrock_level` alone.

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
| `worlds`, `list`, `export`, `status` | yes | — | — |
| `import` | — | — | pack `structures/` |
| `copy` | yes (source) | — | pack `structures/` |
| `install` | — | — | packs, pack JSON, `level.dat` |
| `experiment` | — | — | `level.dat` |
| `delete --source pack` | — | — | unlink one `.mcstructure` |
| **`delete --source world`** | yes | **yes** | — |

`delete --source world` is the only command that writes to a leveldb *deliberately*. Every
"leveldb read" in the table above is a read of a **copy**, never of the world itself,
because opening a database modifies it (§8). Deleting a pack structure is an ordinary file
unlink and carries none of the ceremony in §8.

## 5. Command surface

```
construct worlds                                    # enumerate discovered worlds
construct status                                    # Construct: installed version, latest, enabled worlds
construct list <world>                              # structures in a world, both sources
construct export <world> <structure> [-o FILE]
construct export <world> <s1> <s2>...                # one file each; -o is a usage error
construct export <world> <s1> <s2>... --merge -o FILE
construct import <file> [--world W] [--name N]      # into Construct's structures/
construct copy <src-world> <structure> <dst-world>
construct delete <world> <structure>
construct install [--version V] [--world W]
construct experiment <world> --beta-apis [on|off]   # value omitted prints current state
```

Global flags: `--com-mojang <path>` (repeatable), `--json`, `--force`, `--source <world|pack>`.

### One structure namespace, two sources

A structure lives either in a world's leveldb (saved in-game by a structure block or
`/structure`) or as a file in Construct's `structures/` folder. **Construct itself presents
these as a single in-game list**, so the CLI does too — presenting two lists would be a
worse model than the thing it drives.

```
$ construct list release/Survival
NAME              SOURCE   SIZE
house             world     12 KB
barn              world      4 KB
imported_tower    pack      31 KB
```

Every structure-taking command accepts a bare name and searches both sources. A name present
in both is an error naming the qualified forms, with `--source world|pack` as the
disambiguator — the same never-guess rule §6 applies to world references. `--source` also
works as a plain filter on `list`.

This gives `delete` a useful asymmetry: `--source pack` is an `unlink` with none of §8's
ceremony, while `--source world` is the only leveldb write in the tool.

### Output

`--json` is supported by `worlds`, `list`, and `status`, and by the result summary of every
write command. Each payload carries a `"schema": 1` field. Versioning the output from the
first release is cheap insurance given that a GUI consuming this library is a stated goal.

Under `--json`, **stdout carries exactly one JSON document and nothing else** — a consumer
can parse it without scanning for a payload. Warnings and progress (`world in use — reading
from a 2.4 GB snapshot…`, overlap counts, which pack root was chosen) go to stderr as plain
text, and the ones a caller needs to act on are *also* present as a `"warnings"` array in the
payload. The duplication is deliberate: a GUI linking `construct-core` gets them as typed
values, a terminal user gets them as they happen, and neither is required to read the other
stream.

### Naming and collisions

A single `export` without `-o` writes `<structure-name>.mcstructure` into the current
directory. Several structures without `--merge` write one file each under those same derived
names, and `-o` is then a usage error, since it names a single file rather than several — the
collision rule below applies per file, so one existing target refuses the whole command
before anything is written. `--merge` requires `-o`. World-source structure references are
bare names, meaning the `mystructure` prefix, or explicit `prefix:name`.

`import` derives the structure name from the file stem: lowercased, spaces to `_`, allowing
`[a-z0-9_.-]`. Anything outside that set is **rejected rather than silently mangled**, since
a mangled name is one Construct will not list. The derived name is always printed, and
`--name N` overrides it.

**One collision rule everywhere:** `export -o`, `import`, and `copy` refuse when the target
already exists; `--force` overwrites. `--force` governs file collisions only — it never
relaxes the LOCK refusal in §8. Construct's own pack files during an upgrade are a separate
path and not governed by this rule, and `structures/` is never touched by an upgrade at all.

`import` and `copy` write into Construct's `structures/` folder — never into a leveldb. With
`--world W` they target `<world>/behavior_packs/Construct[BP]/structures/` when that world
has a local copy of Construct, otherwise the installation's shared
`development_behavior_packs` copy, saying which was chosen. Without `--world`, the
installation resolves by the same rule `install` and `status` use in §10 —
`default_installation`, failing that the sole installation, failing that an error listing the
candidates. Both state that the world must be reloaded before Construct sees the structure,
and both fail clearly when the destination world has no Construct, pointing at
`construct install --world <dst>`.

## 6. Discovery and multiple roots

Discovery resolves a list of *installations*. Each has one dev-pack root and zero or more
world roots. Roots that do not exist are absent, with no error.

A world's last-played time comes from `level.dat`'s `LastPlayed` field, falling back to
directory mtime when that is unreadable — the two disagree often enough to matter, and
`--json` reports which was used.

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

**Any world reference may also be a filesystem path.** If an argument resolves on disk to a
directory containing `level.dat`, it is treated as a path; otherwise it parses as a name or
qualified reference. Filesystem check first, so resolution is deterministic. This is why
there is no `--path` flag: a single global flag could only ever describe one world, while
`copy` takes two.

```
construct copy ./OldWorldBackup house release/Survival
```

Cross-root operations are first-class:
`construct copy legacy/OldWorld house release/My Survival`. Source and destination resolve
independently. `install --world W` derives the installation from W and deploys to *that*
installation's dev-pack root, so `preview` and `release` are never mixed.

## 7. Configuration

`~/.config/constructcli/config.toml` and platform equivalents via the `directories` crate:

```toml
default_installation = "release"

[[roots]]                                  # extra roots to probe; named so they can be
name = "backup"                            # addressed in qualified references
path = "D:/MinecraftBackups/com.mojang"

[backups]
dir = "/Volumes/Spare/construct-backups"   # optional; defaults to the platform data dir
keep = 10                                  # default 10
```

Extra roots are a table array rather than a bare path list precisely so each one carries a
name — an unnamed root cannot appear in the `<installation>/<account>/<world>` grammar.

Precedence: CLI flag → environment variable → config file → auto-discovery. An absent file
means all defaults, so there is no init step. Unknown keys warn rather than fail.

Environment variables: `CONSTRUCT_COM_MOJANG`, `CONSTRUCT_INSTALLATION`, `CONSTRUCT_CONFIG`,
and `CONSTRUCT_GITHUB_TOKEN` (falling back to `GITHUB_TOKEN`, which is what CI environments
already set).

## 8. Write safety

**Opening a leveldb database is a write.** This was measured, not assumed: the stage 0
spike opened a real world read-only — never calling `insert` — and the world's `db/`
directory came back changed. `000014.log` and `MANIFEST-000012` were gone, replaced by
`000018.ldb`, `000019.log`, and `MANIFEST-000017`. No data was lost, and every file outside
`db/` was untouched, but the on-disk file set was rewritten.

This is ordinary leveldb behaviour, not a bug in the binding: `DB::Open` runs recovery,
replaying the write-ahead log into a fresh table, compacting, and writing a new manifest.
`bedrock_level::db::Database::open` exposes no read-only or no-recovery option —
`open<P: AsRef<str>>(path) -> Result<Self>` is the entire API. There is no way to ask this
backend to look without touching.

Therefore: **read commands never open a world's database. They copy `db/` to a temporary
directory and open the copy — always, not only when the world is in use.**

```
reading from a 2.4 GB snapshot…
```

The copy is unconditional and not gated behind a flag. It announces itself, because it is
not free: `construct list` on a large survival world copies gigabytes before printing a
dozen lines. That cost buys the guarantee that a read cannot alter a save, which is worth
more than the seconds — a tool that silently rewrites a world it was only asked to inspect
has no business being pointed at anyone's survival world.

A world open in Minecraft reads fine as a result, because the copy never touches the
LOCK; being in use stops mattering for reads.

**There is no pre-flight free-space check.** Every read needs room for a full `db/`, and
if the filesystem cannot provide it the copy fails part-way with the operating system's
own error, which names the real problem. A pre-flight check would turn a slow failure on
a multi-gigabyte world into an immediate one, but `std` exposes no portable way to ask,
and adding a dependency for a message-quality improvement is not worth it in this stage.
Recorded here rather than implied, because an earlier draft of this section promised a
check the code did not perform.

The prize this forfeits is a cheap read. A backend offering a genuine read-only open — one
that neither recovers nor locks — would let `list` and `export` skip the copy entirely.
`bedrock-leveldb` claims exactly that (§3), which is why `StructureStore` is a trait: the
day a read-only open exists, reads stop copying, and no command above the trait changes.

LevelDB writes (`delete --source world` only) follow a fixed sequence:

1. Resolve the world.
2. Snapshot `db/` to the backup directory. **Before opening**, because opening is itself a
   write — a snapshot taken afterwards preserves an already-modified database.
3. Open the database. **If the LOCK is held, stop.** No `--force`; a concurrent writer is
   how worlds get corrupted.
4. Mutate.
5. Drop the handle to flush.
6. Report the backup path in the success message.

There is no `--force` for the LOCK refusal. The `--force` flag in §5 governs file collisions
only and never applies here.

Backups live outside the world folder, in the configured backup directory, with retention
of the last `keep` per world (default 10). A `db.backup-*` folder inside a world directory
would confuse Minecraft, bloat the world, and ride along into any world export. Retention is
keyed on the
path-sanitized qualified reference `<installation>/<account>/<folder>`, not the folder name
alone, which is not unique across roots.

`level.dat` writes (`install --world`, `experiment`) copy the file to the same backup
directory before modifying it. Minecraft's own `level.dat_old` is not a substitute — it is
overwritten by the game on its own schedule.

## 9. The `.mcstructure` codec and merge

`Structure` models the format: `format_version`, `size`, `structure_world_origin`, two
`block_indices` layers, `block_palette`, `block_position_data`, and entities. Encoding is
little-endian NBT.

Merge:

1. Fetch N values and decode.
2. Union the bounding boxes derived from each `structure_world_origin` and `size`. The
   merged output's own `structure_world_origin` is the **min corner of that union**.
3. Unify palettes by deduplicating `(name, states, version)`, building a per-source remap.
4. Blit each piece's two index layers into the output grid.
5. Translate `block_position_data` keys and entity positions into the new frame.
6. Encode.

Gaps between pieces are index **-1** (structure void), not air. Void leaves existing terrain
untouched on placement; air would carve holes in whatever the structure is placed over.

### Overlap

Pieces may overlap — overlapping saves of one build are common and often deliberate.
Resolution is **per position, per layer**: a piece contributes only where its index is
non-void, and where two pieces both contribute, the one appearing later in the argument list
wins. `--on-overlap <last|first|error>` controls this, defaulting to `last`. Any overlap
emits a warning naming the count and the pieces:

```
warning: 1,204 blocks overlapped between "north_wing" and "tower"
```

**`block_position_data` must follow the winning block.** Otherwise a chest's contents survive
from a block that lost the overlap and end up attached to the wrong thing.

### Refusals and warnings

Merge refuses when every `structure_world_origin` is *identical* — including all-zero — since
the pieces would stack in one spot. It does **not** refuse the mixed case where some pieces
sit at `[0,0,0]` and others do not: `[0,0,0]` is indistinguishable from a legitimate save at
world origin, so refusing would be wrong about as often as it was right. That case proceeds
with a warning naming the pieces whose origins may be unset.

Merge also refuses when the union bounding box is large enough to exhaust memory. This is an
allocation guard, not a game limit — Minecraft loads structures beyond structure-block
dimensions without trouble, so oversized results produce a **performance warning**, not a
refusal.

### The encoder is not yet usable

Measured against three real `.mcstructure` files and a synthetic case: **`nbtx` 3.0.1 cannot
serialize an empty list.** It writes the `TAG_List` id and the name, then omits both the
element-type byte and the four-byte length — five bytes short — and the result is NBT that
`nbtx` itself refuses to parse.

```
Compound { "empty": List([]) }
  → 0a 00 00  09  05 00 "empty"  00
                        ^ the element type and length are simply missing
```

Every `.mcstructure` examined carries at least one empty list, so this is not an edge case:
`bomber` has five, `construct` and `bubble_column` one each, and each file re-serializes
exactly five bytes shorter per empty list.

The same crate also converts `ByteArray`, `IntArray`, and `LongArray` into `List` on parse,
so those tags cannot survive a round-trip either.

Decoding is unaffected — every file above parses correctly — so `list`, `export`, `import`,
and `copy` are untouched. **Stage 3 cannot encode with `nbtx` as it stands.** The options,
in the order they should be tried: patch the fork (the project already carries patched
dependencies, and this is a few lines in one serializer), or write the encoder directly,
which is a bounded amount of code given the format is already modelled. Whichever is chosen,
the round-trip property in §12 is the test that proves it.

## 10. Install

1. Query the releases API for `latest`, or `tags/v<version>` with `--version`.
2. Match the `Construct-v*.mcaddon` asset by pattern.
3. Download to a temporary directory.
4. Unzip; identify BP versus RP by reading each `manifest.json`'s module types.
5. Place into `development_behavior_packs` / `development_resource_packs`, matching any
   existing install **by header UUID rather than folder name**.
6. With `--world`, upsert both pack IDs into that world's `world_behavior_packs.json` and
   `world_resource_packs.json`.
7. Attempt the Beta APIs flip in that world's `level.dat`, then re-read and verify.

With no `--world`, `install` and `status` act on `default_installation` from config; failing
that, the sole installation if only one exists; failing that, they error listing the
candidates. Same never-guess rule as world references.

### The world pack JSON files

Measured on a real world with Construct enabled:

```json
[ { "pack_id" : "8c0c0153-d8b9-482a-889f-aef922b8fe58", "version" : [ 1, 0, 0 ] } ]
```

`world_behavior_packs.json` and `world_resource_packs.json` are flat arrays of
`{ pack_id, version }`. On that world the recorded version is `[1, 0, 0]` while the installed
pack is `v1.2.0` — the game matches a development pack by UUID and does not rewrite the entry
when the pack is upgraded. The upsert therefore keys on `pack_id` alone and writes the
installed pack's real header version over a stale one.

`world_behavior_pack_history.json` sits beside them and is Minecraft's own record. It is
never written.

Files in the wild mix tab-indented entries with hand-edited ones, so they are parsed leniently
and rewritten whole. Formatting is not preserved: the game parses JSON, not whitespace.

### The Beta APIs flip

Measured against a real world with Beta APIs enabled. `level.dat` carries an `experiments`
compound, and the enabled state is three `Byte` entries:

```
Compound experiments
  Byte experiments_ever_used            = 1
  Byte gametest                         = 1
  Byte saved_with_toggled_experiments   = 1
```

`gametest` is the Beta APIs toggle itself. The other two are companion flags Minecraft
maintains alongside it, and writing the toggle without them is the silent-failure case — the
write succeeds and the game ignores it.

**Enabling** sets all three to `1`, creating the `experiments` compound if the world has
none. **Disabling** sets `gametest` to `0` and leaves both companion flags at `1`: they are
historical records of the world having once used experiments, not mirrors of the current
state, and clearing them would misrepresent the save. *(The disable direction is inferred
rather than measured — worth confirming against a world that has had experiments turned back
off.)*

**The compound is read-modify-written, never replaced.** Worlds carry other experiment keys
— data-driven biomes, upcoming creator features, and so on — and rewriting the compound
wholesale would silently disable whatever else the world had enabled.

After writing, re-read and verify the value actually changed; treat "write succeeded, value
unchanged" as a failure. `experiment` is also readable — `construct experiment <world>
--beta-apis` with no value prints the current state — which makes the flip verifiable without
launching the game and gives `status` something to report.

### Writing `level.dat` safely

`nbtx` 3.0.1 does not round-trip every NBT tag (§9): array tags come back as lists, one byte
longer each, and an empty list re-serializes five bytes short into something unparseable.
Either defect, applied to a `level.dat`, corrupts a save.

The five real `level.dat` files on the development machine each re-serialize to a payload of
*identical length* whose value compares equal, which is the evidence that none of them carries
an array tag or an empty list. That is a property of those files, not a promise about anyone
else's world.

Every `level.dat` write therefore runs a **fidelity gate** first: re-serialize the *unmodified*
parsed root and require the result to be exactly as long as the original payload. Both defects
change the length, so a mismatch refuses the write before anything is touched, naming the file
and saying the world is untouched. Failing loudly beats silently corrupting a save.

Key order is not preserved — `nbtx` parses a compound into a `HashMap` — so a rewritten
`level.dat` lists its keys in a different order than Minecraft wrote them. NBT compounds are
unordered and the game rewrites the file on its own schedule, so this is cosmetic.

The write itself is a temporary file in the same directory followed by a rename, so the file is
never left partially written, and §8's backup copy is taken before any of it.

**Upgrading must not delete `Construct[BP]/structures/`.** That folder holds the user's
imported structures — the very data this tool exists to put there. A naive delete-and-unzip
would destroy it. Install preserves it across upgrades and reports what it carried over.

The addon ships one of its own — `Construct[BP]/structures/construct.mcstructure` is inside the
`.mcaddon` — so an upgrade **merges** rather than skips: files the new version ships are
written, every other file already in `structures/` is kept, and the number preserved is
reported.

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

Ambiguity is exit `2`, not `3`. The target exists — the *reference* was underspecified. `3`
is reserved for things that genuinely are not there.

Every message names the thing, says why, and gives the next action.

| Failure | Handling | Exit |
|---|---|---|
| No `com.mojang` found | List every path probed; point at `--com-mojang` | 3 |
| World not found | Suggest near matches | 3 |
| World reference ambiguous | Disambiguation table with qualified references | 2 |
| World in use, read command | Irrelevant — reads always work from a copy | 0 |
| No room for the snapshot a read needs | The copy fails part-way with the OS error; there is no pre-flight check (§8) | 1 |
| World in use, `delete --source world` | Stop hard; no `--force` | 4 |
| Structure not found | Suggest near matches from the catalog already in hand | 3 |
| Structure name in both sources | Name both qualified forms; point at `--source` | 2 |
| Structure prefix | Bare `name` means `mystructure:name`; `prefix:name` accepted explicitly | — |
| Target file already exists | Refuse; point at `--force` | 1 |
| Unusable name from file stem | Reject rather than mangle; point at `--name` | 2 |
| Codec failure | Report file, field, and byte offset | 1 |
| Identical merge origins | Refuse, explaining the pieces would stack | 1 |
| Mixed merge origins | Proceed; warn which pieces may have unset origins | 0 |
| Merge overlap | Resolve per `--on-overlap`; warn with counts | 0 |
| Oversized merge | Warn about performance; refuse only on allocation limits | 0 / 1 |
| Construct not installed | Point at `install` | 3 |
| Two Construct copies | State which was chosen and why | — |
| Installation ambiguous (no `--world`) | List candidates; point at `default_installation` | 2 |
| GitHub rate limit | Name the 60/hour unauthenticated limit; point at `CONSTRUCT_GITHUB_TOKEN` | 1 |
| No asset for `--version` | List available assets | 3 |
| `level.dat` write fails or does not take | Packs installed; name the manual step | 5 |

## 12. Testing

**Pure logic, no I/O.** Codec, palette unification, merge geometry, key encode/decode,
world-reference resolution, and version comparison are pure functions over bytes and structs.
Committed `.mcstructure` fixtures cover: a single block, a multi-block with block entities, a
waterlogged block (second index layer), one with entities, and one with void gaps.

**Two invariants as properties:**

- *Byte transparency* — `export` → `import` → `export` yields identical bytes. If this fails,
  the claim that most commands avoid the codec is false.
- *Merge placement* — merging one structure yields it unchanged; and for any world coordinate
  and layer, the merged result holds the block from the **last** contributing piece in
  argument order, or void where none contributed. Stated in terms of argument order rather
  than "whichever source had a block," which was ambiguous under overlap.

Overlap gets its own tests: two pieces disagreeing at a coordinate resolve per
`--on-overlap`, and `block_position_data` follows the winning block rather than the losing
one. The chest-contents-attached-to-the-wrong-block failure is invisible in a block-only
comparison, so it is asserted explicitly.

Reference resolution is its own table-driven suite: a name in only `world`, only `pack`, both
(error naming both forms), neither (error suggesting near matches), plus `--source` filtering
and the `prefix:name` form. Name derivation from file stems gets cases for spaces, capitals,
and characters outside `[a-z0-9_.-]` — asserting rejection, not mangling. Collisions are
asserted for `export -o`, `import`, and `copy`: refuse by default, overwrite under `--force`.

Merge results are compared **semantically** on decoded structures. NBT key ordering is not
guaranteed stable, so golden-file byte comparison would produce meaningless failures.

**Database tests run on a disposable copy** — a tarred fixture world extracted per test, the
pattern `bedrock-rs` uses for `crates/level/tests/level.tar.gz`. A guard helper refuses any
database path outside the temp directory. Plus lock detection, snapshot-before-delete, and
restore-from-snapshot.

**Install is fully mockable** with a stubbed releases endpoint and a synthetic `.mcaddon`.
The highest-priority test is upgrade-preserves-`structures/`. Alongside it: UUID matching
over folder names, pack-JSON upsert idempotence, and `level.dat` failure producing exit 5.

The `experiments` compound gets its own fixtures: a world with the compound absent (enabling
must create it), one with all three flags already set (enabling is a no-op), and one carrying
unrelated experiment keys alongside `gametest` — asserting those siblings survive, since a
wholesale rewrite would silently disable them.

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

`bedrock_level` and `leveldb-sys` are git dependencies on **patched forks**, each pinned to
an explicit commit hash, never a floating branch — `bedrock-rs` has no tags or crates.io
release, and the upstream pins do not compile on macOS or on any non-x86_64 target (§3).
The four fixes are offered upstream as pull requests; if they land, the forks are retired
and the pins move back. Upgrades are deliberate and re-run the spike. Apache-2.0 permits
both the forking and the vendoring; the `StructureStore` trait keeps a backend swap from
being a rewrite. `nbtx` comes from crates.io.

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
1. **Read path** — workspace scaffolding, `config`, `discovery` (including multi-root, world
   references, and paths-as-references), `store` reads, and `worlds` / `list` / `export`.
   Delivers the half of the tool that replaces holoprint, and touches nothing destructive.
   `list` is world-source only until stage 2 adds the pack source.
2. **Construct integration** — `pack`, `catalog`, `install`, `status`, `experiment`,
   `import`, `copy`, and `delete --source pack`. Delivers the other documented manual
   workflow and unifies the namespace. Writes files and `level.dat`, but no leveldb. Settle
   the pack-structure naming question (§17) here, once there is tooling to check it with.
3. **Merge** — `mcstructure` codec and `merge`, behind `export --merge`. The only stage
   needing the codec; fully testable against fixtures.
4. **Delete from a world** — `backup` plus `store` writes and `delete --source world`.
   Deliberately last: the only leveldb write, and the only stage that can damage a world.

Note that deletion arrives in stage 2 for pack structures, so users get the capability long
before the risky path exists. Stage 4 could be dropped entirely without affecting anything
else, which is worth remembering if `bedrock_level`'s write path disappoints in the spike.

## 15. Risk register

| Risk | Severity | Mitigation |
|---|---|---|
| Construct upgrade wipes `structures/` | **Data loss** | Preserve across upgrade; highest-priority test |
| `delete` corrupts a world db | **Data loss** | Mojang's leveldb via FFI; snapshot before write; hard refusal on LOCK |
| `bedrock-rs` churns or stalls | High | Commit pin; `StructureStore` trait; Apache-2.0 permits vendoring |
| Upstream never merges the portability fixes | Medium | **Realized.** Forks are pinned and self-sufficient; upstream merging is an improvement, not a dependency |
| A read silently rewrites a world | **Data loss** | **Realized in the spike.** Reads always operate on a copy (§8); asserted by a test that hashes `db/` before and after |
| Windows GDK assumptions unverified | High | Manual checklist on real hardware before release |
| Beta APIs flip writes the wrong NBT and fails silently | Low | Keys measured from a real world (§10); read-modify-write preserves sibling experiments; verify by re-reading after write |
| CMake/C++ toolchain friction | Medium | CI on all three platforms; prebuilt binaries for users |
| Construct asset naming changes | Medium | Pattern match; clear error listing available assets |
| GitHub rate limit | Low | Named in the error; optional token |
| `.mcstructure` format version changes | Low | Version checked on decode; explicit error |
| `nbtx` corrupts a `level.dat` it rewrites | **Data loss** | **Measured** (§9). Fidelity gate refuses the write when re-serialization changes length (§10) |
| `nbtx` cannot encode `.mcstructure` (empty lists) | High | **Measured** (§9). Decoding is unaffected; blocks stage 3 only, and the fix is a patch or a small encoder |

The two data-loss rows hold the release. Everything else degrades into a bad afternoon.

## 16. Manual verification checklist

These cannot be settled by automated tests and must be confirmed in the game:

1. Does a merged structure beyond structure-block dimensions load and place correctly?
2. Does Construct pick up an imported structure after a world reload?
3. Does the `level.dat` Beta APIs flip register in-game? (`construct experiment <world>
   --beta-apis` confirms the file round-trips; only the game confirms it is honored.)
4. Do dev packs in `Users\Shared` apply to a world owned by a specific account? *(Windows)*
5. Do files written into the GDK folder by an ordinary process read back in-game? *(Windows)*
6. Does a `.mcstructure` in `structures/<Namespace>/` load as `<namespace>:<name>`, and does a
   deeper `structures/<ns>/<a>/<b>.mcstructure` load as `<ns>:<a>/<b>`? Both forms are now
   documented (§17) rather than inferred; this confirms the documentation against the shipping
   game.

Items 4 and 5 need real Windows hardware. Item 5 is expected to be a non-issue: the ACL
problem was specific to UWP's `LocalState` inside an AppContainer sandbox, and GDK stores
data in ordinary `AppData\Roaming`.

## 17. Assumptions to verify

- Microsoft documents `Users\Shared\...\development_behavior_packs` as the creator
  deployment location, which implies the game reads dev packs from `Shared` regardless of
  the signed-in account. The design depends on this.
- **How a `.mcstructure` sitting directly in `structures/` is addressed in-game** — as bare
  `name`, or as `namespace:name` requiring a subdirectory. Construct's README says to drop
  files directly into the folder, which suggests the bare form, but this determines what
  `list` displays for pack-source structures and what `import` should name them. Deferred to
  stage 2, when the tooling to check it exists.

  Two shipped packs found on a real machine use *different* layouts, which is the strongest
  evidence available short of testing in-game:

  | Pack | Path under the pack | Implied reference |
  |---|---|---|
  | Canopy | `structures/bubble_column.mcstructure` | `mystructure:bubble_column` |
  | Understudy | `structures/Understudy/players.mcstructure` | `understudy:players` |

  The working hypothesis is therefore that **a subdirectory under `structures/` supplies the
  namespace, and a file directly in `structures/` falls back to `mystructure:`** — which
  would make both forms valid rather than one of them wrong, and would mean `import` has a
  namespace choice to make rather than a fixed rule to follow.

  **The flat half is settled by Construct's own source.** `Construct[BP]` v1.2.0 enumerates
  structures like this:

  ```js
  const packIds = [...new Set(structureManager.getPackStructureIds()
      .map(id => id.replace('mystructure:', '')))];
  ```

  Pack structure ids therefore arrive from the game already carrying `mystructure:`, so a file
  sitting directly in `structures/` is `mystructure:<stem>`. That is what `import` writes by
  default, and what `list` displays. The subdirectory form remains inferred — from
  `behavior_packs/Understudy/structures/Understudy/players.mcstructure` in a world-local pack
  copy — and is item 6 on the §16 checklist rather than something `import` defaults to.

  The same code shows two behaviours the CLI deliberately does **not** copy. Construct's list
  lets a pack structure *shadow* a world structure of the same name, and it drops world
  structures outside `mystructure:` entirely. §5 refuses an ambiguous name rather than picking
  a winner, and `list` shows every namespace — but the ambiguity message says which copy
  Construct would show in-game, since that is the question the user is really asking.

  **Settled, and wider than assumed.** `docs/bedrock-mcstructure-files.md` — a local copy of
  tryashtar's third-party documentation of the Bedrock `.mcstructure` format and its loading
  rules, published on GitHub (github.com/tryashtar, not committed to this repository) — resolves
  the remaining half and corrects an assumption this design made:

  | Path under the pack | Identifier |
  |---|---|
  | `structures/house.mcstructure` | `mystructure:house` |
  | `structures/dungeon/entrance.mcstructure` | `dungeon:entrance` |
  | `structures/stuff/towers/diamond.mcstructure` | `stuff:towers/diamond` |

  **The first subfolder is the namespace; every folder after it is part of the name.** Nesting is
  therefore not limited to one level, which is what an earlier reading of the evidence assumed —
  `list` must walk `structures/` to its full depth, and an id may legitimately contain `/` after
  the colon.

  Two further behaviours are documented there and worth recording:

  - A file directly in `structures/` and one in an explicit `structures/mystructure/` folder
    collide. The `mystructure` folder wins, the root file is ignored, and the game logs
    `There was a conflict loading a structure in the default namespace`.
  - **A world's embedded structures take priority over a pack's when the game loads one.** This is
    the opposite direction from Construct's in-game *list*, which lets the pack copy shadow the
    world copy. Both are true: the addon's list and the engine's loader disagree, so the CLI says
    which copy Construct displays rather than claiming which one the game would place.

  Reading and writing stay asymmetric on purpose. `list` shows whatever depth is on disk, but
  `import --name` still refuses a `/` in the name: creating nested paths is a capability nobody has
  asked for, and the character that would enable it is the one that makes traversal possible.
- The **disable** direction of the Beta APIs flip (§10). The enabled state is measured; that
  the companion flags stay at `1` when turning it off is inferred.
- ~~`leveldb-sys` vendors leveldb rather than using a submodule~~ — **confirmed**: no
  `.gitmodules`, full source under `ffi/leveldb/`, so a plain clone builds.
- ~~A structure's leveldb value is byte-identical to a `.mcstructure` file~~ — **confirmed
  by the stage 0 spike**: round-trip is byte-identical and values begin `0x0a`.
- The **Windows** lock-error path. The stage 0 spike measured the POSIX message
  (`IO error: lock <path>/LOCK: already held by process`) from an in-process double-open;
  leveldb's `env_win.cc` is a different implementation with a different message, and no
  external-process holder has been tested on any platform. §8 therefore never matches on
  the message text.
