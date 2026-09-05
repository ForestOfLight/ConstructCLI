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

## The array-tag defect in `nbtx` is patched around, not fixed

Task 1 fixed only the empty-list bug. `nbtx` still turns `ByteArray`, `IntArray`, and
`LongArray` into `List` on parse, so those tags cannot survive a round trip. This was left
alone deliberately: no array tag appears in any of the 13 real `.mcstructure` files measured,
and fixing it properly is not a small patch — `Value`'s `Serialize` impl routes all three
through `serialize_seq`, and serde's data model has no way to tell them apart on the way out.

The exposure is `level.dat`, not `.mcstructure`. The fidelity gate in `leveldat.rs` catches it
there by comparing re-serialized length, and one test now depends on an array tag being
unfaithful — if `nbtx` ever gains real array support, that test's premise disappears the same
way the empty-list one just did.

## Merge holds the whole result in memory

`merge` allocates two `i32` vectors covering the union bounding box, so a merge of pieces far
apart costs 8 bytes per block of mostly-empty space. `MergeOptions::max_volume` caps this at 64
million blocks (~512 MB) and refuses beyond it. A sparse representation would lift the cap, but
nothing observed needs it: the largest real structure measured is 81×81×81.

## `--merge` does not check that the pieces came from one build

Nothing verifies the structures are pieces of the same build rather than unrelated saves that
happen to have distinct origins. The identical-origin refusal catches the common mistake; a
merge of genuinely unrelated structures produces a mostly-air result the user did not want,
with no warning beyond the size one. Filling gaps with air raised the stakes here: the unwanted
result no longer merely fails to place blocks between the pieces, it clears that space. The
size warning fires for a union past 64x256x64, which is the case for any two structures far
enough apart to matter, but it says "placing may be slow" rather than "this will clear a
corridor".

## The `--on-overlap last` test does not discriminate

In `crates/construct-core/tests/merge.rs`,
`block_position_data_follows_the_block_that_won_the_overlap` still passes if
`block_position_data` is inserted unconditionally instead of only for the cell's owner,
because pieces are walked in argument order and `Last`'s winner is the last piece anyway. The
property is pinned solely by the `OnOverlap::First` variant of that test. Not a code defect —
the property is covered — but the `Last` test looks stronger than it is.

## One merge test is still vacuous for entities

`a_merged_structure_encodes_and_decodes_back_equal` is a real round-trip check for
`block_position_data`, but neither of its pieces sets `entities`, so that half asserts
nothing. Entity round-tripping is covered non-vacuously in
`crates/construct-core/tests/mcstructure.rs`, so there is no coverage gap overall.

## The blit's overflow guard is not proof against wrapping arithmetic

`crates/construct-core/src/merge.rs` uses `checked_add`/`checked_sub` helpers so an untrusted
origin cannot overflow. The test asserts only that merging does not panic, so replacing those
helpers' internals with `wrapping_add`/`wrapping_sub` breaks no test. Judged acceptable: the
plausible accidental regression is raw `+`, which IS caught, and a reviewer could not
construct a realistic input where wrapping produces a silently misplaced block rather than a
dropped one.

## `decode`'s module doc overclaims

`crates/construct-core/src/mcstructure/decode.rs` said its validation "mirrors the load-time
rules the game itself enforces, documented in `docs/bedrock-mcstructure-files.md`", but two of
its checks are ours rather than the documentation's: rejecting negative `size` dimensions, and
rejecting a `block_position_data` key whose index is at or past the volume. Fixed in the code
comment as part of this task — a one-line honesty correction, not a behaviour change — and
recorded here because the two checks themselves remain deliberate additional strictness beyond
what the reference documentation specifies.

## §11's codec error contract is two-thirds delivered

§11 asks codec failures to "report file, field, and byte offset." `mcstructure/nbt.rs`'s
errors name the file and the field but never a byte offset, because `nbtx` does not surface
one anywhere in its error type — there is nothing to plumb through. Fixing this properly means
either patching `nbtx` to track and report a position (a nontrivial addition next to the
empty-list and array-tag patches already carried) or abandoning `nbtx`'s own error type for a
lower-level parse that tracks offsets itself. Left as documentation debt rather than fixed,
since no failure observed so far has been ambiguous enough to need the offset to diagnose.

## `format_version` is read but never validated

`decode.rs` reads `format_version` off every structure but never checks it against anything,
and `merge` silently adopts the first piece's value for the merged output with no comparison
to the others. §15's risk register claims the mitigation for a format mismatch is "Version
checked on decode; explicit error" — that mitigation does not exist; the register overstates
what ships. Adding a real check needs a decision this branch never made: which versions are
compatible with which, and what to do when a merge mixes them (refuse, warn, or silently take
the max) — that belongs to whichever future work first needs to distinguish structure format
versions.

## `scripts/setup-deps.sh` is not idempotent across patch-set changes

`clone_and_patch` skips cloning and patching entirely when the checkout directory under
`third_party/checkouts/` already exists. Anyone who ran the script before this branch added
patch `0004` (the deeply-nested-NBT stack guard, from `a4f699f`) keeps an `nbtx` checkout
patched only with `0003`. The build still succeeds — nothing about the missing patch is a
compile error — but
`deeply_nested_nbt_is_refused_rather_than_overflowing_the_stack` then hits the real stack
overflow the patch was meant to prevent and aborts the test binary instead of failing it
normally. The script's own skip message does say "delete it to re-create," so this is a
documented trap rather than a silent break, but nothing detects the drift automatically. The
real fix is patch-set drift detection — hashing the applied patch set and re-applying when it
changes — which is its own task, not a one-line fix here.

## §12's fixture deviation was never recorded

§12 asks for five committed fixture files, one per documented shape. The branch instead builds
four of the five programmatically in `tests/support/mod.rs` (`Build`, described in its own
module doc) and commits one real file, `tests/fixtures/construct.mcstructure`. This was a
deliberate choice, and the better one: a builder call states its fixture's shape in the test
that uses it, where a committed binary blob cannot be read at all without a hex dump, and the
one real file still proves the codec agrees with what the game itself writes — the thing a
builder can never prove. It is nonetheless a deviation from what §12 literally asks for, and
it was never written down until now.
