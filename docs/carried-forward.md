# Carried forward from stage 2

Things found during stage 2 that were deliberately not fixed, with enough detail to act
on. Nothing here blocks stage 2; each was triaged and consciously carried. Stage 1's own
entries are gone from this file — most were fixed by Task 1 during this stage; the one
still live is repeated below under its own heading.

## ~~Prerequisite for stage 4 (writing to a world)~~ — closed in stage 4

**`guard_test_path` lived in the caller, not the primitive.** The guard is now inside
`BedrockStore::open_copy`, so no caller can forget it, and it was renamed `guard_copy_path`
to say what it actually asserts.

The move was not the straight one this entry anticipated, because stage 4's writer genuinely
has to open a real world path and no path guard can allow that. The split is by type instead:
`open_copy(&Path)` is guarded and serves every read; `open_live(&World)` takes a `World`
rather than a path, so it cannot be reached by handing the wrong path to a general-purpose
opener, and it has one call site. `BedrockStore::open` was removed so the ambiguous spelling
does not survive.

The macOS `/var/folders` canonicalization concern this entry raised is untouched and still
latent — `guard_copy_path` still falls back to the uncanonicalized path when `canonicalize`
fails. Nothing in stage 4 made it worse or better; it has simply never fired.

## Carried out of stage 4 (the world-delete write path)

**`delete`'s grammar changed after stage 4 landed.** It was
`delete <world> <structures…>`, where a bare invocation removed every copy the world saw —
including the shared Construct's. It is now `delete <structures…>` with an optional
`--world`, mirroring `import`: absence targets the shared copy, `--world W` targets that
world and **cannot reach the shared copy at all**.

That closed a sharp edge nobody had named. Under the old grammar the shortest world-scoped
command silently unlinked a file every other world using the shared copy depended on; the
warning said so, but the removal happened either way. Now the two scopes are two invocations
and neither can reach the other's files — a name found only in the shared copy reports as
missing from the world instead of being deleted from under everyone.

`--pack` became a usage error on `delete` in the same change, because `--world` now expresses
scope and `--world W --pack shared` asks for two contradictory things at once. It is no longer
declared on `delete` at all — `--source` and `--pack` stopped being globals and are now
per-command args on the commands that read them — so clap makes the refusal rather than a
hand-rolled check in `main.rs`.



**There is no undo for a world delete.** Deliberate, and a reversal of what §8 originally
specified: the db backup it called for cost a full copy of `db/` — gigabytes — on every world
delete, and leveldb's own crash-safety means it was never really protecting the database.
What it did protect against is deleting the wrong structure, and nothing replaces that.
`export` before `delete` is the workaround, and the README says so. If this turns out to
matter, the cheap fix is to write the removed structure's own bytes (kilobytes, not
gigabytes) to the backup directory via the existing `backup::file`, which needs no new
retention knob.

**~~`delete` is self-blocking for ten seconds.~~ Fixed, along with a live false negative it
uncovered.** The in-use check was write-recency against `db/`, and a delete writes `db/` — so
a second `construct delete` within the window was refused, reporting that Minecraft had the
world open when the recent write was this tool's own.

Investigating it against a live session (2026-09-05, mcpelauncher flatpak / Linux 1.26.45.1)
found the more serious problem. Autosave gaps on this build reach **10s** — `5 5 5 10 5 5 5
10 5 5 5 5 10 5` across 90 seconds — where `ACTIVITY_WINDOW` was also 10s and `looks_in_use`
compares with `<`. Every time the game left a full gap, a world it definitely had open read
as free. The 10s window came from a 5s max gap measured on *macOS*; this build saves half as
often. That was a live false negative on the dangerous side, present before stage 4 and
unrelated to it.

Both are fixed. `ACTIVITY_WINDOW` is now 20s (2x the new measured max), and the check has
three phases: the instant recency test only *suspects*; `writemark::left_by_us` then answers
"that write was ours" instantly from a recorded mtime; and only if neither settles it does
`confirm_in_use` watch for a further write.

The mark is what removes the wait rather than merely shortening it. An earlier revision
stopped at two phases, which was correct but paid the full 20s watch in exactly the case that
was never a problem — this tool's own previous write. Measured end to end: a second `delete`
straight after the first now takes **0.033s**, against 20s with the watch alone and a
refusal before any of this. (Written when that refusal was exit 4; it is exit 1 with
`error.kind: world-in-use` since exit codes 3-5 were retired.)

Verified against a live session on 2026-09-05: a live world is confirmed in 2-3.5s (the watch
bails the moment it sees a write), a world closed for hours is cleared instantly, and a
*deliberately planted* mark goes stale within one autosave gap and does not mask the open
world. That last one is the corrupting direction and has both a unit test and an
`#[ignore]`d test against the real game.

**What the mark costs.** `delete` is now stateful: it writes one small file per world under
`writemarks/` in the platform data directory (or `CONSTRUCT_DATA_DIR`). State can be stale or wrong in ways a
pure observation cannot, which is why phase 3 stays underneath it rather than being replaced
— every way the mark can fail falls through to the watch. The file is written
atomically (write-then-rename, pid-tagged temporary) because two processes marking one world
concurrently could otherwise expose a half-written stamp to a reader. Nothing prunes these;
they are one line each, one per world ever deleted from.

**One wait remains.** A recent write that is *not* ours and is not followed by another still
costs the full `CONFIRM_WATCH` before the command proceeds — a world just copied in, restored
from a backup, or touched by a file manager. Rare, correct, and it errs toward waiting rather
than toward writing, so it was left alone.

**The exact answers were deliberately not taken.** `/proc/*/fd` does resolve to a real host
path through the flatpak sandbox — measured, the fd target came back as the true
`~/.var/app/.../minecraftWorlds/<world>/db/000017.log` and the holder as
`mcpelauncher-client` — so a Linux-only check could be exact in both directions rather than
heuristic. It was rejected on the grounds that a detector precise on one platform and absent
on the others is worse than one that behaves identically everywhere. The same reasoning rules
out an `flock` probe, which is moot here regardless: a sweep of 10 real worlds found no
`db/LOCK` at all, confirming the 2026-09-04 finding.

**A power loss immediately after a delete loses the removal.** The FFI shim builds its
`leveldb::WriteOptions` with defaults, so `sync = false`, and neither `bedrock_level` nor the
shim exposes a knob (`third_party/checkouts/leveldb-sys/ffi/ffi.cpp`, `struct Database`). The
deletion reaches the WAL through a buffered write and is durable once the OS flushes it, so a
crashed *process* is fine and a lost-power *machine* may not be. Fixing it properly means
patching the shim to take a sync flag. Not judged worth it: the failure re-runs the command.

**A world whose `db/` cannot be opened refuses a bare `delete` entirely**, even when the name
only ever existed in a pack. This is deliberate — degrading to packs-only would report a
structure deleted while a copy of it survived in the database — and `--source pack` is the
escape hatch, which the `CoreError::Db` hint in `report` now names. It does mean an
unopenable database blocks a pack unlink that has nothing to do with it.

**`delete` is an explicit exception to §8's read-via-copy rule**, and the only one. Its
catalog — the entry list it resolves names against, the same one `structures` renders — is built
from a live open rather than a snapshot. That is not a shortcut: the command is about to
write, so a copy would be discarded unwritten. But it does mean the invariant "opening a
world's database happens only in `delete`" now carries all the weight that "reads only open
copies" used to share.

**A real-leveldb fixture that inserts keys looks in use.** `fixture_world_with_construct`
writes through the leveldb API, which leaves `db/` freshly modified — inside
`inuse::ACTIVITY_WINDOW` — so any delete test against it must call the `close_world` helper
first or get a confusing in-use refusal. The tarball's own mtimes are old enough, so this only bites
fixtures that pass `extra_world_structures`. A helper that backdated automatically would be
tidier, but the in-use test needs the un-backdated form.

**`CoreError::InsufficientSpace` is declared and never constructed.** Pre-existing, noticed
while working next to it. §8 says explicitly there is no pre-flight free-space check, so the
variant describes an error nothing can produce. Either wire it up or delete it; stage 4 did
neither because it touches the snapshot path, which stage 4 does not use.

**Nothing tests the `CoreError::Internal` arms.** Both are unreachable by construction —
a world entry with no open database, a pack entry with no path — which is why they are
checked refusals rather than `unwrap`s. Unreachable code that cannot be tested is still
unreachable code.

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
- ~~`delete`'s unreachable `path == None` backstop returns `NotImplemented`~~ — closed in
  stage 4. `CoreError::Internal { what }` replaced `NotImplemented` in the enum (`delete`
  was the latter's only producer), and the backstop's meaning inverted along the way: world
  entries legitimately have no path now, so the impossible case is a *pack* entry without
  one.
- ~~`delete <world> <name>` with no `--source`, for a name that exists only in the
  database, reports `StructureNotFound`~~ — closed in stage 4, though not the way this
  entry predicted. There is no snapshot copy: `delete` opens the world's own database
  directly, because it is about to write to it and a copy would be discarded unwritten.

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
- The `structures` human-output test asserts substring presence rather than table structure, so
  a formatting regression would pass. Carried from stage 1 and still true.

## Process notes

- Task 3's report presented a predicted compiler failure as TDD evidence rather than a
  captured run. The code was independently verified; the ordering was not. Later dispatches
  required pasted output, and Task 17's implementer honestly disclosed that its brief
  supplied the code verbatim so no red run ever existed.

## Found by the final review's own re-review

Two things surfaced after the fix wave, judged not worth another round.

- **`install --world` still backs up `level.dat` unconditionally**, before checking whether Beta
  APIs is already on — the same shape that was fixed in `enable-beta-apis`. Lower impact there,
  since `install --world` is not something a user repeats in a loop the way `enable-beta-apis`
  might be, but it is the same class of backup churn and the fix is the same one.
- **No test proves an *ordinary* 403 takes the generic network path** rather than being reported as
  a rate limit. The `x-ratelimit-remaining` check that separates them was confirmed by reading the
  code, not by a test. The rate-limited 403 and the 404 both have tests.

## The pack-enable step is still unguarded against a live world

`install --world` now refuses up front when Minecraft appears to have the
world open (`construct_core::inuse`, `error.kind: world-in-use`), which covers the `level.dat`
flip that prompted it. The pack-enable step writes
`world_behavior_packs.json` / `world_resource_packs.json`, and the game
rewrites *those* from memory on world exit too — observed directly on
2026-09-04, mtime moving at 11:09:15 as a session ended.

Deliberately not guarded, on the grounds that `install --world` refuses
before it does anything, so the only way to reach the unguarded write is to
open the world during the seconds the command is running. If that turns out
to matter, the fix is to re-check `inuse::looks_in_use` immediately before
`worldpacks::upsert` and treat a positive as a partial install
(`error.kind: partial-install`), not to move the up-front check.

## The in-use window now rests on two measurements, and they disagreed

`inuse::ACTIVITY_WINDOW` is 20 seconds, twice the longest gap measured between autosaves of
a live world. Two builds have been measured and they did not agree:

| Build | Writes / 90s | Longest gap |
| --- | --- | --- |
| mcpelauncher / macOS 1.26.45.1 | 19 | 5s |
| mcpelauncher flatpak / Linux 1.26.45.1 | 14 | **10s** |

The manual-checklist item for re-measuring per platform did its job: the Linux figure is
double the macOS one, and the window derived from macOS alone was too small for Linux (see
the delete self-blocking entry above). A third build could be slower still, and the same
false negative would return for it — but the two-phase check limits the damage now, since a
build that saves *rarely* would have to stay silent for a full 20-second watch to slip
through, not merely for the gap between two writes.

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

## `export`'s world moved from a positional to `--world`, which breaks the old spelling

`construct export <world> <structure…>` is now `construct export <structure…>
[--world W]`, and `--pack` is gone from the command. The old spelling does not
error usefully: `construct export "My Survival" house` parses cleanly and reads
`My Survival` as a *structure name*, so it fails with "structure not found"
rather than anything that points at the grammar change. There is no deprecation
shim and no alias.

This was deliberate — it closes seam 1 for `export` and makes it `import`'s
mirror image, absence meaning the shared copy for both — but it is the second
breaking change to a read command's argument order on this branch, after
`copy <src> <dst> <s…>`. Anything scripted against the old form needs editing,
and nothing detects it for the user.

`--pack` fell out rather than being removed for its own sake: under `--world`
the shared copy is out of scope, so a world-scoped `export` sees exactly one
pack and has nothing left to choose between.

## `structures`'s world moved to `--world` too, and took `--pack` with it

`construct structures <world>` is now `construct structures [--world W]`. Unlike
`export`, the old spelling does fail loudly — `structures` has no positionals
left, so clap reports an unexpected argument and exit 2 — but it is still a
breaking change with no shim and no alias, the third to a read command's
grammar on this branch.

The reason is the one that moved `export` and `delete`: absence means the shared
copy of Construct, and a flag is what says otherwise. `structures` already read
that way — a bare invocation listed the shared copy alone — so the positional
was the odd spelling of a sense the rest of the surface expresses with `--world`.

`--pack` went for a *different* reason than it did on `export` and `delete`, and
the difference is worth keeping straight. There it was contradictory: `--world W
--pack shared` asks for the copy `--world` exists to exclude. Here it was merely
redundant. A world listing is that world's whole view by definition, both packs
included, and the SOURCE column already labels every row `pack:world` or
`pack:shared` — so `--pack` narrowed a listing that was never ambiguous, and
narrowed it destructively, dropping world-database rows along with the other
pack's. Anyone who was using it as a filter now reads the SOURCE column, or
pipes `--json` through a filter on `scope`.

`--pack` is therefore declared on `copy` alone, which is the only command left
that must resolve one name out of the two packs a source world sees. The
`--source`/`--pack` overloading of the token `world` (seam 2 in
`docs/cli-surface.md`) narrows with it, but is not gone.
