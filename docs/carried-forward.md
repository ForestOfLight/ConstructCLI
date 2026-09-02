# Carried forward from stage 1

Things found during stage 1 that were deliberately not fixed, with enough detail to act
on. Nothing here blocks stage 1; each was triaged and consciously carried.

## Prerequisite for stage 4 (writing to a world)

**`guard_test_path` lives in the caller, not the primitive.** `snapshot::open_via_snapshot`
calls it immediately before `BedrockStore::open`, which enforces §8's promise that a read
only ever opens a copy. But `BedrockStore::open` is `pub`, so stage 4's writer — or a GUI
linking `construct-core` — can open a live world directly with nothing to stop it.

Move the assertion into `BedrockStore::open` before stage 4 adds a write path. It is not a
straight move: `guard_test_path` falls back to the *uncanonicalized* path when
`canonicalize` fails, and on macOS `/var/folders` versus `/private/var/folders` makes that
fallback fail for a temp path that does not exist yet. Canonicalize the parent first.

## Correctness, narrow

- **`store::get` re-qualifies an already-complete id.** A structure key with no namespace
  (`structuretemplate_foo`) is shown by `list` but cannot be fetched by `export`, because
  `get("foo")` looks for `structuretemplate_mystructure:foo`. A real `list`/`export`
  inconsistency, reachable only in worlds Minecraft did not write — the game always emits
  `mystructure:`.
- **An unreadable installation vanishes silently.** `platform::resolve` returns
  `Vec<Installation>` with no `Result`, and both `read_dir` errors and `is_dir()` collapse
  to "absent". §6 excuses roots that are *missing*, not roots that exist and cannot be
  read, so a permissions problem reads as "no worlds found". Fixing it is a signature
  change.
- **Reserved config root names are matched case-sensitively**, so `Release` is accepted.
  It shadows nothing — reference resolution is also case-sensitive — but a user who typed
  it would wonder why their root does nothing.

## Quality and performance

- **`catalog::from_world` reads every structure in full to populate a size column.**
  Measured on a real 910-structure world: 63.5 MB read, ~1.6 s. Giving `StructureStore` a
  cheaper `fn size(&self, id: &str) -> Result<u64>` would remove it.
- **Near-match suggestions use substring containment only**, so the commonest typos —
  transposition and deletion, e.g. `Amelx` for `Amelix CMP` — produce no suggestion at
  all. §11 promises "suggest near matches"; edit distance would deliver it.
- **`catalog::from_world` sorts by name alone.** `Vec::sort_by` is stable and input order
  is deterministic, so nothing flaps today, but when stage 2 makes `Source::Pack`
  reachable, two entries can share a display name and the order between them would be
  inherited from concatenation rather than stated. Add `.then(a.source.cmp(&b.source))`.
- **`Out::emit`'s non-object `"value"` fallback is unreachable** with today's payloads and
  would silently change the JSON shape if a later command emitted a non-object.
- **`AmbiguousStructure` says "exists in both a world and a pack"** and advises `--source`,
  but `catalog::resolve` raises it for any multi-match. If two entries in the same source
  ever collide, the advice cannot be followed.
- **Config `default_installation` / `CONSTRUCT_INSTALLATION` are parsed but unused** in
  stage 1. Stage 2's `install`/`status` consume them; until then they are silently ignored.

## Test gaps

- `size_counts_the_db_directory` asserts `>= 1024` where `== 1024` is equally
  deterministic and stronger.
- The `list` human-output test asserts substring presence, not table structure, so a
  formatting regression would pass.

## Edge cases in hostile or unusual worlds

Structure names come from a world file the user may not have authored, and stage 1 already
refuses path traversal and sanitizes characters illegal on Windows. Two remain, both only
reachable in worlds Minecraft did not write:

- A key of exactly `structuretemplate_mystructure:` yields an empty display name. Export
  refuses it; `list` still shows a blank name.
- Names differing only by characters that sanitize to the same filename (`a:b` and `a_b`)
  collide on export. The collision rule catches it — the second is refused as an existing
  target — so nothing is overwritten, but the error does not explain the cause.
