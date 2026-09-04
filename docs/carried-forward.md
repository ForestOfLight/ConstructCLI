# Carried forward from stage 2

Things found during stage 2 that were deliberately not fixed, with enough detail to act
on. Nothing here blocks stage 2; each was triaged and consciously carried. Stage 1's own
entries are gone from this file — most were fixed by Task 1 during this stage; the one
still live is repeated below under its own heading.

## Prerequisite for stage 4 (writing to a world)

**`guard_test_path` lives in the caller, not the primitive.** `snapshot::open_via_snapshot`
calls it immediately before `BedrockStore::open`, which enforces §8's promise that a read
only ever opens a copy. But `BedrockStore::open` is `pub`, so stage 4's writer — or a GUI
linking `construct-core` — can open a live world directly with nothing to stop it.

Move the assertion into `BedrockStore::open` before stage 4 adds a write path. It is not a
straight move: `guard_test_path` falls back to the *uncanonicalized* path when
`canonicalize` fails, and on macOS `/var/folders` versus `/private/var/folders` makes that
fallback fail for a temp path that does not exist yet. Canonicalize the parent first.

Stage 2 adds no leveldb write, so this was deferred again — but it is now the last thing
standing between stage 4 and a live-world open.

## Security, narrow

- **The install staging sentinel is a fixed, predictable filename.** An adversarial
  `.mcaddon` could ship a file at that exact path to pre-seed a false "stage complete"
  marker, which would only matter in combination with a crash timed precisely inside
  `copy_dir` before the rest of the pack lands. Strictly narrower than the design it
  replaced, which an ordinary crash defeated with no adversarial input at all. Closing it
  is cheap: bind the sentinel's contents to the staging directory's own unique name and
  verify that on recovery.

## Correctness, narrow

- `catalog::resolve` does not deduplicate `sources`, so a collision within one source
  would render "exists in: world, world". Not reachable today.
- `derive_name(".foo")` is accepted and produces a dotfile inside `structures/`.
- `tag_for("")` yields `"v"`, and the leading-`v` check is a naive `starts_with`.
- A failed rename in `leveldat::write` leaves a stray `.level.dat.construct-tmp` beside the
  world's `level.dat`. The original is untouched; only the debris is left.
- `delete`'s unreachable `path == None` backstop returns `NotImplemented`, which describes
  the situation less well than an internal-invariant error would — but `construct-core`
  must not panic.
- `delete <world> <name>` with no `--source`, for a name that exists only in the database,
  reports `StructureNotFound` (exit 3) rather than the more accurate `NotImplemented`
  (exit 2). Resolving across both sources would mean snapshot-copying the world's database
  for a file unlink, which §8 forbids. Stage 4 removes the refusal and this imperfection
  with it.

## Edge cases in `structures/`

Found while giving `pack::structures`' id derivation its full-depth walk; both reachable
only in a `structures/` tree this tool or Construct did not build entirely itself.

- **A non-UTF-8 path component is dropped, not skipped.** The id derivation filters out
  any path component it cannot render as `&str`, so two structures whose paths differ only
  by such a component can collapse onto the same id. Can only shorten an id, never insert
  `..` or an absolute path.
- **A `foo.mcstructure` that is itself a symlink to a directory is listed as a leaf**, not
  recursed into. Pre-existing behaviour, unchanged by the depth walk — noted in passing,
  not a new regression.

## Error shapes

- `CoreError::BadPack` is reused for the install-time UUID mismatch, where the pack is not
  malformed but simply is not Construct. The `reason` string carries the meaning.
- `CoreError::Network { reason }` bakes formatted text rather than carrying a status code.
- `leveldat::to_bytes` fills `path: PathBuf::new()` and relies on `write` to substitute the
  real path — a latent trap for any future direct caller.

## Quality and performance

- `encode_exact` duplicates `encode`'s body; `manifest::read` reconstructs `BadPack` inline
  rather than reusing `parse`'s closure; `installation::for_world` repeats `choose`'s
  `names()` one-liner; `commands/catalog.rs` and `commands/copy.rs` each phrase their own
  `also_at` warning.
- `import` reads the whole source file before validating the derived name or resolving the
  target pack, so it fails slower than it needs to on a bad name.
- Near-match suggestions still use substring containment only, so transposition and
  deletion typos produce no suggestion. Carried from stage 1 and still true.

## Test gaps

- No test covers `import`'s two-Construct-copies (`also_at`) warning or its
  namespaced-`--name` warning.
- No test asserts `status`'s human-readable output text, only its JSON payload and exit
  code.
- A world whose `world_behavior_packs.json` is malformed is silently omitted from
  `status`'s enabled list, with no warning — asymmetric with how the same command explains
  an unreachable GitHub.
- `size_counts_the_db_directory` asserts `>= 1024` where `== 1024` is equally
  deterministic and stronger. Carried from stage 1 and still true.
- The `list` human-output test asserts substring presence rather than table structure, so
  a formatting regression would pass. Carried from stage 1 and still true.

## Process notes

- Task 3's report presented a predicted compiler failure as TDD evidence rather than a
  captured run. The code was independently verified; the ordering was not. Later dispatches
  required pasted output, and Task 17's implementer honestly disclosed that its brief
  supplied the code verbatim so no red run ever existed.

## Found by the final review's own re-review

Two things surfaced after the fix wave, judged not worth another round.

- **`install --world` still backs up `level.dat` unconditionally**, before checking whether Beta
  APIs is already on — the same shape that was fixed in `experiment`. Lower impact there, since
  `install --world` is not something a user repeats in a loop the way `experiment` might be, but it
  is the same class of backup churn and the fix is the same one.
- **No test proves an *ordinary* 403 takes the generic network path** rather than being reported as
  a rate limit. The `x-ratelimit-remaining` check that separates them was confirmed by reading the
  code, not by a test. The rate-limited 403 and the 404 both have tests.

## The pack-enable step is still unguarded against a live world

`install --world` now refuses up front when Minecraft appears to have the
world open (`construct_core::inuse`, exit 4), which covers the `level.dat`
flip that prompted it. The pack-enable step writes
`world_behavior_packs.json` / `world_resource_packs.json`, and the game
rewrites *those* from memory on world exit too — observed directly on
2026-09-04, mtime moving at 11:09:15 as a session ended.

Deliberately not guarded, on the grounds that `install --world` refuses
before it does anything, so the only way to reach the unguarded write is to
open the world during the seconds the command is running. If that turns out
to matter, the fix is to re-check `inuse::looks_in_use` immediately before
`worldpacks::upsert` and treat a positive as a partial failure (exit 5), not
to move the up-front check.

## The in-use window rests on a single measurement

`inuse::ACTIVITY_WINDOW` is 10 seconds, twice the longest gap measured
between autosaves of a live world (19 writes in 90s, max gap 5s) on
mcpelauncher/macOS 1.26.45.1. Nothing confirms other Minecraft builds save as
often. A build that saves less frequently gets a false negative, and the
silent-revert bug returns for it; there is a manual-checklist item for
re-measuring per platform.

Detection is by write recency because `db/LOCK` is unusable here: the
mcpelauncher build creates no LOCK file even with a world open, and the only
LOCK on the test machine was a stale leftover from an unclean exit. If a
platform is found where the game does hold an flock'd LOCK, adding that as a
definitive positive alongside the heuristic would be a strict improvement.
