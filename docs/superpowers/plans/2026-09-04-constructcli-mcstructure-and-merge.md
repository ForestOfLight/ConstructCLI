# `.mcstructure` Codec and Merge Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Decode and encode `.mcstructure` files, and merge several structures into one that reassembles them at their recorded world origins, exposed as `construct export <world> <s1> <s2>... --merge -o FILE`.

**Architecture:** A `Structure` model mirrors the measured NBT shape exactly, with `decode`/`encode` between it and little-endian NBT bytes. `merge` is a pure function over decoded `Structure`s: union the bounding boxes, unify the palettes, blit each piece's two index layers into the output grid, and carry `block_position_data` alongside the block that wins each cell. Encoding becomes possible at all only because Task 1 patches `nbtx`, whose empty-list serializer is broken.

**Tech Stack:** Rust 2024 (floor 1.88), `nbtx` 3.0.1 (patched, little-endian NBT), `thiserror` in core, `clap` + `anyhow` in the CLI.

**Spec:** `docs/superpowers/specs/2026-08-31-constructcli-design.md` (§9 codec and merge, §5 command surface, §11 error handling, §12 testing, §14 stage 3)

**Reference:** `docs/bedrock-mcstructure-files.md` — third-party format documentation by tryashtar, published on GitHub (github.com/tryashtar). Currently untracked in this repo. It is the authority for the field layout, the ZYX index order, and the game's load-time validation rules. **Do not edit it.**

## Global Constraints

- `construct-core` never prints, never panics, never reads argv, never sets exit codes. It returns typed values and `CoreError`. The CLI owns all formatting, exit codes, and `anyhow`.
- Rust edition 2024, toolchain floor 1.88. `cargo` is at `~/.cargo/bin` — every shell needs `export PATH="$HOME/.cargo/bin:$PATH"`.
- `cargo clippy --all-targets -- -D warnings` and `cargo fmt --all --check` must both pass before any commit.
- Exit codes (§11): `0` success · `1` failure · `2` usage error · `3` not found · `4` world in use · `5` partial success.
- Under `--json`, **stdout carries exactly one JSON document and nothing else**, carrying `"schema": 1`. Warnings go to stderr as plain text *and* into the payload's `"warnings"` array.
- Reads never open a world's own database — `store::open_world_store` copies `db/` and opens the copy. Stage 3 adds no leveldb write and no leveldb read beyond what `export` already does.
- One collision rule everywhere: `export -o` refuses when the target exists; `--force` overwrites.
- Merge results are compared **semantically** on decoded structures, never by golden-file byte comparison — `nbtx` stores compounds in a `HashMap`, so key order is not stable (§12).

### Measured facts this plan depends on

All measured this session against 13 real `.mcstructure` files (289 B to 4.25 MB) exported from the developer's own worlds:

- The root compound has exactly four keys: `format_version` (Int, always 1), `size` (List of 3 Int), `structure` (Compound), `structure_world_origin` (List of 3 Int).
- `structure` has three keys: `block_indices` (List of exactly 2 Lists of Int), `entities` (List of Compound), `palette` (Compound).
- `palette` contains exactly one key in every observed file: `default`. **`block_palette` and `block_position_data` live under `palette.default`**, not directly under `structure`. The spec's §9 prose omits this nesting level.
- `block_position_data` is a Compound whose keys are **decimal strings of the flattened block index** (`"0"`, `"143"`, `"524880"`). Each value is a Compound that may carry `block_entity_data` and/or `tick_queue_data`.
- Index order is **ZYX**: `index = SZ*SY*X + SZ*Y + Z`, and back: `(x, y, z) = (i/SZ/SY, i/SZ%SY, i%SZ)`.
- `-1` means structure void — existing terrain is left untouched on placement.
- Verified on all 13 files: `len(layer0) == len(layer1) == SX*SY*SZ`, every `block_position_data` key parses to an index inside `0..volume`, and every palette index is within `-1..palette_len`.
- **No `ByteArray`, `IntArray`, or `LongArray` tag occurs in any of the 13 files.** `nbtx` cannot round-trip those tags (it turns them into Lists), but the defect is unreachable for `.mcstructure` in practice.
- With Task 1's patch applied, all 13 files decode and re-encode to a payload of **identical length** that **reparses** and compares **semantically equal**. None is byte-identical, because `nbtx` emits compound keys in `HashMap` order.

### Entity positions need no translation — and why

`docs/bedrock-mcstructure-files.md` states: *"An entity's new absolute position is equal to its old position, minus these values [`structure_world_origin`], plus the origin of the structure's loading position."*

So a piece `P` with origin `O_p` holding an entity at stored position `Pos` places that entity, when loaded at `L_p`, at `Pos - O_p + L_p`. In a merged structure `M` with origin `O_m` loaded at `L`, the same entity lands at `Pos - O_m + L`. For the merged result to reproduce the original layout, piece `P` must effectively load at `L_p = L + (O_p - O_m)`, which gives `Pos - O_p + L + O_p - O_m = Pos - O_m + L`. The two agree.

**Therefore entities are carried into the merged structure with their `Pos` unchanged.** Spec §9 step 5 says "Translate `block_position_data` keys and entity positions into the new frame"; for entities that is wrong, and Task 9 amends the spec. `block_position_data` keys genuinely do need recomputing, because they index a grid whose dimensions changed.

---

## File Structure

**Created:**

- `third_party/patches/0003-nbtx-empty-list-serialization.patch` — the `nbtx` fix.
- `crates/construct-core/src/mcstructure/mod.rs` — the `Structure` model and the public `decode`/`encode` entry points.
- `crates/construct-core/src/mcstructure/nbt.rs` — small typed accessors over `nbtx::Value` (`as_compound`, `field`, `as_int_list`, …) so decode reads as field extraction rather than nested matches.
- `crates/construct-core/src/mcstructure/geometry.rs` — `Size`, `Origin`, `BoundingBox`, index↔coordinate conversion.
- `crates/construct-core/src/mcstructure/decode.rs` — NBT bytes → `Structure`.
- `crates/construct-core/src/mcstructure/encode.rs` — `Structure` → NBT bytes.
- `crates/construct-core/src/merge.rs` — the merge algorithm and its options/report types.
- `crates/construct-core/tests/mcstructure.rs` — codec integration tests.
- `crates/construct-core/tests/merge.rs` — merge integration tests.
- `crates/construct-core/tests/support/mod.rs` — the `Build` fixture builder shared by both test files.
- `crates/construct-core/tests/fixtures/construct.mcstructure` — one real committed fixture, taken from Construct's own behaviour pack.

**Modified:**

- `Cargo.toml` — `[patch.crates-io]` redirect for `nbtx`.
- `scripts/setup-deps.sh` — a third `clone_and_patch` call.
- `crates/construct-core/src/lib.rs` — register `mcstructure` and `merge`.
- `crates/construct-core/src/error.rs` — new variants for codec and merge failures.
- `crates/construct-core/src/leveldat.rs` — re-base one test fixture off an array tag.
- `crates/construct-cli/src/cli.rs` — `--merge` and `--on-overlap` on `Export`.
- `crates/construct-cli/src/commands/export.rs` — the merge branch.
- `crates/construct-cli/src/main.rs` — usage errors and error help arms.
- `crates/construct-cli/tests/cli.rs` — end-to-end tests.
- `docs/superpowers/specs/2026-08-31-constructcli-design.md` — §9 amendments.
- `docs/carried-forward.md` — deferred findings.
- `README.md` — the `--merge` usage.

---

### Task 1: Vendor and patch `nbtx`

Encoding is impossible until this lands: `nbtx` 3.0.1 omits an empty list's element-type byte and length entirely, writing five bytes too few and producing NBT it cannot itself parse. Every real `.mcstructure` examined carries at least one empty list.

**Files:**
- Create: `third_party/patches/0003-nbtx-empty-list-serialization.patch`
- Modify: `scripts/setup-deps.sh`, `Cargo.toml`, `crates/construct-core/src/leveldat.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: a workspace where `nbtx::to_le_bytes` round-trips empty lists. Every later task depends on this.

- [ ] **Step 1: Clone the upstream crate at the pinned revision**

```bash
export PATH="$HOME/.cargo/bin:$PATH"
cd "$(git rev-parse --show-toplevel)"
git clone https://github.com/bedrock-crustaceans/bedrockrs-nbt third_party/checkouts/nbtx
git -C third_party/checkouts/nbtx checkout bd28e77
```

`third_party/checkouts/` is git-ignored. Confirm the crate is version 3.0.1: `grep '^version' third_party/checkouts/nbtx/Cargo.toml`.

- [ ] **Step 2: Write a failing test proving the defect**

Add to `third_party/checkouts/nbtx/src/test.rs`:

```rust
#[test]
fn an_empty_list_round_trips() {
    let mut map = std::collections::HashMap::new();
    map.insert("empty".to_string(), crate::Value::List(vec![]));
    let value = crate::Value::Compound(map);

    let bytes = crate::to_le_bytes(&value).expect("empty list must serialize");
    let parsed: crate::Value =
        crate::from_le_bytes(&mut bytes.as_slice()).expect("output must parse back");
    assert_eq!(parsed, value);
}
```

- [ ] **Step 3: Run it to confirm it fails**

Run: `cd third_party/checkouts/nbtx && cargo test an_empty_list_round_trips`
Expected: FAIL at `from_le_bytes` — the five missing bytes make the output unparseable.

- [ ] **Step 4: Apply the fix**

In `third_party/checkouts/nbtx/src/nbt/ser.rs`.

The bug: `SerializeSeq::serialize_element` writes the element-type byte and the 4-byte length lazily, on the first element, guarded by `if self.len != 0`. An empty sequence never calls `serialize_element`, and `end()` writes nothing. `self.len` cannot distinguish the two states — it reads 0 both for an empty sequence and for one whose header is already out — so a separate flag is required.

Add the field to `struct Serializer` (near `len: usize`):

```rust
    /// Whether a sequence has been opened whose header (element type and
    /// length) has not been written yet. `len` cannot answer this: it reads 0
    /// both for an empty sequence and for one whose header is already out.
    seq_header_pending: bool,
```

Initialise it beside `is_initial: true,` in the constructor:

```rust
            seq_header_pending: false,
```

Arm it when a sequence opens — in `serialize_seq`, after `self.len = len;`, and in `serialize_tuple`, after `self.len = len;`:

```rust
            self.seq_header_pending = true;
```

Disarm it where the header is actually written. In **both** `SerializeSeq::serialize_element` and `SerializeTuple::serialize_element`, inside the `if self.len != 0 { … }` block, immediately after `self.len = 0;`:

```rust
            self.seq_header_pending = false;
```

Then replace **both** `end()` bodies. In `impl SerializeSeq for &mut Serializer<W, F>` the endianness parameter is `F`; in `impl SerializeTuple for &mut Serializer<W, M>` it is `M` — use the right one in each:

```rust
    fn end(self) -> Result<(), Error> {
        // An empty sequence never reaches `serialize_element`, so without this
        // its header is never written at all: five bytes short (one element
        // type, four length) and unparseable. NBT writes `TAG_End` with a
        // length of zero for an empty list, which is what every empty list in
        // a real `.mcstructure` carries.
        if self.seq_header_pending {
            self.writer.write_u8(FieldType::End as u8)?;
            match F::AS_ENUM {
                Variant::BigEndian => self.writer.write_i32::<BigEndian>(0),
                Variant::LittleEndian => self.writer.write_i32::<LittleEndian>(0),
                Variant::NetworkEndian => self.writer.write_i32_varint(0),
            }?;
            self.seq_header_pending = false;
        }
        Ok(())
    }
```

- [ ] **Step 5: Confirm the fix and check for regressions**

Run: `cd third_party/checkouts/nbtx && cargo test`
Expected: PASS, including the 16 tests upstream already had.

- [ ] **Step 6: Capture the patch**

```bash
cd third_party/checkouts/nbtx
git diff > ../../patches/0003-nbtx-empty-list-serialization.patch
git -C . stash list >/dev/null   # sanity: we did not commit inside the checkout
```

Verify the patch applies cleanly from scratch:

```bash
cd "$(git rev-parse --show-toplevel)"
rm -rf /tmp/nbtx-verify
git clone --quiet https://github.com/bedrock-crustaceans/bedrockrs-nbt /tmp/nbtx-verify
git -C /tmp/nbtx-verify checkout --quiet bd28e77
git -C /tmp/nbtx-verify apply third_party/patches/0003-nbtx-empty-list-serialization.patch
echo "patch applies cleanly"
```

- [ ] **Step 7: Wire it into the dependency script**

In `scripts/setup-deps.sh`, after the `bedrock-rs` call:

```bash
clone_and_patch nbtx \
  https://github.com/bedrock-crustaceans/bedrockrs-nbt \
  bd28e77 \
  0003-nbtx-empty-list-serialization.patch
```

- [ ] **Step 8: Redirect the workspace at the patched checkout**

In the root `Cargo.toml`, extend the existing patch section. `nbtx` comes from crates.io, so it needs `[patch.crates-io]` rather than a URL key:

```toml
# nbtx 3.0.1 cannot serialize an empty list — it omits the element type and
# length, five bytes short, producing NBT it cannot parse back. Every real
# .mcstructure carries at least one empty list, so encoding is impossible
# without this. See third_party/patches/ and spec §9.
[patch.crates-io]
nbtx = { path = "third_party/checkouts/nbtx" }
```

Run `cargo build` and confirm `Cargo.lock` now resolves `nbtx` to the local path.

- [ ] **Step 9: Run the existing suite and expect exactly one failure**

Run: `cargo test`
Expected: `leveldat::tests::a_file_that_cannot_round_trip_refuses_to_be_written` FAILS; everything else passes.

That test builds its unfaithful fixture out of an empty list, which is precisely what Task 1 fixed. The file is now legitimately faithful and writable, so the gate correctly declines to refuse it. The gate itself still matters — `nbtx` still turns `IntArray` into `List`, one byte longer — so the fixture is re-based rather than the test deleted.

- [ ] **Step 10: Re-base that test on an array tag**

In `crates/construct-core/src/leveldat.rs`, replace the body of `a_file_that_cannot_round_trip_refuses_to_be_written` with:

```rust
    fn a_file_that_cannot_round_trip_refuses_to_be_written() {
        // nbtx parses TAG_Int_Array into Value::List, which re-serializes as a
        // list — one byte longer, since a list carries an element-type byte an
        // array does not. The length check catches it. (Before Task 1 this
        // fixture used an empty list; that defect is fixed, so an array tag is
        // now the reachable way to be unfaithful. No array tag appears in any
        // real .mcstructure examined, but level.dat is a different file and
        // this gate is what stands between a stray one and a corrupted save.)
        //
        // { "gaps": IntArray([7]) }
        let payload: Vec<u8> = vec![
            0x0a, 0x00, 0x00, // TAG_Compound, root name ""
            0x0b, // TAG_Int_Array
            0x04, 0x00, b'g', b'a', b'p', b's', // name "gaps"
            0x01, 0x00, 0x00, 0x00, // one element
            0x07, 0x00, 0x00, 0x00, // the element: 7
            0x00, // TAG_End of compound
        ];
        let mut bytes = 10i32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&(payload.len() as i32).to_le_bytes());
        bytes.extend_from_slice(&payload);

        let dat = parse(&bytes, Path::new("level.dat")).unwrap();

        assert!(!dat.is_faithful());
        assert!(matches!(
            dat.to_bytes(),
            Err(CoreError::UnwritableLevelDat { .. })
        ));

        // A gate that returns an error but writes anyway is as dangerous as no
        // gate at all: the write must refuse, and the file on disk must be
        // untouched, byte for byte.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("level.dat");
        std::fs::write(&path, &bytes).unwrap();

        let err = write(&dat, &path).unwrap_err();
        assert!(matches!(err, CoreError::UnwritableLevelDat { .. }));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
```

- [ ] **Step 11: Add a test proving the patch reached this workspace**

Also in `crates/construct-core/src/leveldat.rs`'s test module — this is the regression guard that fails if the `[patch.crates-io]` entry is ever dropped:

```rust
    #[test]
    fn the_patched_nbtx_round_trips_an_empty_list() {
        // Guards the [patch.crates-io] redirect in the root Cargo.toml. With
        // stock nbtx 3.0.1 this fails, and with it failing the whole codec is
        // unusable — so the failure should point straight at the dependency
        // rather than at a hundred confusing codec errors.
        let mut map = HashMap::new();
        map.insert("empty".to_string(), nbtx::Value::List(vec![]));
        let value = nbtx::Value::Compound(map);

        let bytes = nbtx::to_le_bytes(&value).expect("patched nbtx must serialize an empty list");
        let parsed: nbtx::Value = nbtx::from_le_bytes(&mut bytes.as_slice())
            .expect("patched nbtx must parse its own empty list back");
        assert_eq!(parsed, value);
    }
```

- [ ] **Step 12: Verify everything is green**

```bash
cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --all --check
```
Expected: all tests pass (246 before this task's two changes; the count rises by one).

- [ ] **Step 13: Document the third dependency**

In `README.md`, wherever `scripts/setup-deps.sh` is described, add `nbtx` to the list of patched checkouts with a one-line reason ("cannot serialize an empty list; every `.mcstructure` has one").

- [ ] **Step 14: Commit**

```bash
git add third_party/patches/0003-nbtx-empty-list-serialization.patch scripts/setup-deps.sh \
        Cargo.toml Cargo.lock crates/construct-core/src/leveldat.rs README.md
git commit -m "Patch nbtx so it can serialize an empty list"
```

---

### Task 2: Geometry — sizes, origins, and the ZYX index

Pure arithmetic, no NBT. Every later task depends on getting the index order right, and it is the single easiest thing to get subtly wrong.

**Files:**
- Create: `crates/construct-core/src/mcstructure/geometry.rs`, `crates/construct-core/src/mcstructure/mod.rs`
- Modify: `crates/construct-core/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub struct Size { pub x: i32, pub y: i32, pub z: i32 }` with `pub fn volume(&self) -> i64`, `pub fn index_of(&self, c: Coord) -> Option<usize>`, `pub fn coord_of(&self, i: usize) -> Option<Coord>`
  - `pub struct Coord { pub x: i32, pub y: i32, pub z: i32 }` (also used for origins; `Copy`, `Eq`, `Hash`)
  - `pub struct BoundingBox { pub min: Coord, pub max_exclusive: Coord }` with `pub fn of(origin: Coord, size: Size) -> Self`, `pub fn union(boxes: &[BoundingBox]) -> Option<BoundingBox>`, `pub fn size(&self) -> Size`

- [ ] **Step 1: Write the failing tests**

Create `crates/construct-core/src/mcstructure/geometry.rs` with only the test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_index_order_is_zyx() {
        // From docs/bedrock-mcstructure-files.md: index = SZ*SY*X + SZ*Y + Z.
        // A 2x3x4 structure: z varies fastest, then y, then x.
        let s = Size { x: 2, y: 3, z: 4 };
        assert_eq!(s.index_of(Coord { x: 0, y: 0, z: 0 }), Some(0));
        assert_eq!(s.index_of(Coord { x: 0, y: 0, z: 1 }), Some(1));
        assert_eq!(s.index_of(Coord { x: 0, y: 1, z: 0 }), Some(4));
        assert_eq!(s.index_of(Coord { x: 1, y: 0, z: 0 }), Some(12));
        assert_eq!(s.index_of(Coord { x: 1, y: 2, z: 3 }), Some(23));
    }

    #[test]
    fn every_index_round_trips_through_coordinates() {
        let s = Size { x: 3, y: 5, z: 7 };
        for i in 0..s.volume() as usize {
            let c = s.coord_of(i).expect("index inside the volume must convert");
            assert_eq!(s.index_of(c), Some(i), "index {i} did not round-trip");
        }
    }

    #[test]
    fn coordinates_outside_the_size_have_no_index() {
        let s = Size { x: 2, y: 2, z: 2 };
        assert_eq!(s.index_of(Coord { x: 2, y: 0, z: 0 }), None);
        assert_eq!(s.index_of(Coord { x: 0, y: -1, z: 0 }), None);
        assert_eq!(s.coord_of(8), None);
    }

    #[test]
    fn a_bounding_box_spans_origin_to_origin_plus_size() {
        let b = BoundingBox::of(Coord { x: 10, y: 0, z: -5 }, Size { x: 2, y: 3, z: 4 });
        assert_eq!(b.min, Coord { x: 10, y: 0, z: -5 });
        assert_eq!(b.max_exclusive, Coord { x: 12, y: 3, z: -1 });
        assert_eq!(b.size(), Size { x: 2, y: 3, z: 4 });
    }

    #[test]
    fn a_union_covers_every_box_including_negative_coordinates() {
        let a = BoundingBox::of(Coord { x: 0, y: 0, z: 0 }, Size { x: 2, y: 2, z: 2 });
        let b = BoundingBox::of(Coord { x: -3, y: 5, z: 1 }, Size { x: 1, y: 1, z: 1 });
        let u = BoundingBox::union(&[a, b]).unwrap();
        assert_eq!(u.min, Coord { x: -3, y: 0, z: 0 });
        assert_eq!(u.max_exclusive, Coord { x: 2, y: 6, z: 2 });
        assert_eq!(u.size(), Size { x: 5, y: 6, z: 2 });
    }

    #[test]
    fn a_union_of_nothing_is_nothing() {
        assert_eq!(BoundingBox::union(&[]), None);
    }

    #[test]
    fn a_volume_that_overflows_i32_is_still_computed_in_i64() {
        // 2000^3 is 8e9, far past i32. Sizes come from a file we did not write,
        // so the arithmetic must not wrap silently.
        let s = Size { x: 2000, y: 2000, z: 2000 };
        assert_eq!(s.volume(), 8_000_000_000);
    }
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p construct-core geometry`
Expected: FAIL to compile — `Size`, `Coord`, `BoundingBox` do not exist.

- [ ] **Step 3: Implement**

Prepend to `crates/construct-core/src/mcstructure/geometry.rs`:

```rust
//! Sizes, origins, and the flattened block index.
//!
//! A `.mcstructure` stores its blocks in one flat list per layer, in **ZYX**
//! order: `index = SZ*SY*X + SZ*Y + Z`. Getting this order wrong produces a
//! structure that loads without error and is transposed, so it is isolated
//! here and tested exhaustively rather than open-coded at each use.

/// A block coordinate, also used for world origins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Coord {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

/// A structure's dimensions in blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl Size {
    /// Total block count. Computed in `i64`: sizes come from a file this tool
    /// did not write, and a wrapped `i32` would silently under-allocate.
    pub fn volume(&self) -> i64 {
        i64::from(self.x.max(0)) * i64::from(self.y.max(0)) * i64::from(self.z.max(0))
    }

    /// The flattened index of a coordinate, or `None` if it lies outside.
    pub fn index_of(&self, c: Coord) -> Option<usize> {
        if c.x < 0 || c.y < 0 || c.z < 0 || c.x >= self.x || c.y >= self.y || c.z >= self.z {
            return None;
        }
        let i = i64::from(self.z) * i64::from(self.y) * i64::from(c.x)
            + i64::from(self.z) * i64::from(c.y)
            + i64::from(c.z);
        usize::try_from(i).ok()
    }

    /// The coordinate a flattened index refers to, or `None` if out of range.
    pub fn coord_of(&self, i: usize) -> Option<Coord> {
        let i = i64::try_from(i).ok()?;
        if i < 0 || i >= self.volume() {
            return None;
        }
        let sz = i64::from(self.z);
        let sy = i64::from(self.y);
        Some(Coord {
            x: (i / sz / sy) as i32,
            y: (i / sz % sy) as i32,
            z: (i % sz) as i32,
        })
    }
}

/// A half-open box in world space: `min` inclusive, `max_exclusive` exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoundingBox {
    pub min: Coord,
    pub max_exclusive: Coord,
}

impl BoundingBox {
    pub fn of(origin: Coord, size: Size) -> Self {
        Self {
            min: origin,
            max_exclusive: Coord {
                x: origin.x.saturating_add(size.x),
                y: origin.y.saturating_add(size.y),
                z: origin.z.saturating_add(size.z),
            },
        }
    }

    pub fn size(&self) -> Size {
        Size {
            x: self.max_exclusive.x - self.min.x,
            y: self.max_exclusive.y - self.min.y,
            z: self.max_exclusive.z - self.min.z,
        }
    }

    /// The smallest box containing all of `boxes`, or `None` when empty.
    pub fn union(boxes: &[BoundingBox]) -> Option<BoundingBox> {
        let mut it = boxes.iter();
        let first = *it.next()?;
        Some(it.fold(first, |acc, b| BoundingBox {
            min: Coord {
                x: acc.min.x.min(b.min.x),
                y: acc.min.y.min(b.min.y),
                z: acc.min.z.min(b.min.z),
            },
            max_exclusive: Coord {
                x: acc.max_exclusive.x.max(b.max_exclusive.x),
                y: acc.max_exclusive.y.max(b.max_exclusive.y),
                z: acc.max_exclusive.z.max(b.max_exclusive.z),
            },
        }))
    }
}
```

- [ ] **Step 4: Create the module and register it**

`crates/construct-core/src/mcstructure/mod.rs`:

```rust
//! The `.mcstructure` format: model, codec, and geometry.
//!
//! The field layout, the ZYX index order, and the game's load-time validation
//! rules come from `docs/bedrock-mcstructure-files.md` (third-party format
//! documentation by tryashtar, github.com/tryashtar), cross-checked against 13
//! real files exported from the developer's own worlds.

pub mod geometry;

pub use geometry::{BoundingBox, Coord, Size};
```

In `crates/construct-core/src/lib.rs`, add `pub mod mcstructure;` in alphabetical order (after `pub mod leveldat;`).

- [ ] **Step 5: Verify**

Run: `cargo test -p construct-core geometry && cargo clippy --all-targets -- -D warnings && cargo fmt --all --check`
Expected: 7 tests pass, clippy and fmt clean.

- [ ] **Step 6: Commit**

```bash
git add crates/construct-core/src/mcstructure crates/construct-core/src/lib.rs
git commit -m "Add .mcstructure geometry: sizes, origins, and the ZYX index"
```

---

### Task 3: The `Structure` model and `decode`

**Files:**
- Create: `crates/construct-core/src/mcstructure/nbt.rs`, `crates/construct-core/src/mcstructure/decode.rs`
- Modify: `crates/construct-core/src/mcstructure/mod.rs`, `crates/construct-core/src/error.rs`

**Interfaces:**
- Consumes: `Size`, `Coord` from Task 2.
- Produces:
  - `pub struct Structure { pub format_version: i32, pub size: Size, pub origin: Coord, pub layers: [Vec<i32>; 2], pub palette: Vec<BlockState>, pub block_position_data: BTreeMap<usize, nbtx::Value>, pub entities: Vec<nbtx::Value> }`
  - `pub struct BlockState { pub name: String, pub states: nbtx::Value, pub version: i32 }` — `Eq + Hash + Clone`
  - `pub const VOID: i32 = -1;`
  - `pub fn decode(bytes: &[u8], what: &str) -> Result<Structure>`
  - `CoreError::BadStructureFile { what: String, reason: String }`

- [ ] **Step 1: Write the failing tests**

Create `crates/construct-core/tests/support/mod.rs` — the fixture builder both test files use:

```rust
//! A builder for `.mcstructure` NBT, so tests can state a fixture's shape
//! instead of hand-assembling bytes.
//!
//! Fixtures are built rather than committed as binaries: the five shapes spec
//! §12 asks for are then readable in the test that uses them, and no opaque
//! blob has to be trusted. One real committed file
//! (`tests/fixtures/construct.mcstructure`) covers what a builder cannot —
//! that the codec agrees with what the game actually writes.

use std::collections::HashMap;

pub fn compound(pairs: Vec<(&str, nbtx::Value)>) -> nbtx::Value {
    nbtx::Value::Compound(
        pairs
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect::<HashMap<_, _>>(),
    )
}

pub fn int_list(v: &[i32]) -> nbtx::Value {
    nbtx::Value::List(v.iter().copied().map(nbtx::Value::Int).collect())
}

/// One entry of `block_palette`.
pub fn block(name: &str) -> nbtx::Value {
    compound(vec![
        ("name", nbtx::Value::String(name.to_string())),
        ("states", compound(vec![])),
        ("version", nbtx::Value::Int(18163713)),
    ])
}

pub struct Build {
    pub size: [i32; 3],
    pub origin: [i32; 3],
    pub layer0: Vec<i32>,
    pub layer1: Vec<i32>,
    pub palette: Vec<nbtx::Value>,
    pub block_position_data: Vec<(String, nbtx::Value)>,
    pub entities: Vec<nbtx::Value>,
    pub format_version: i32,
}

impl Build {
    /// A structure of `size` filled with palette entry 0 on layer 0 and void
    /// on layer 1 — the shape the overwhelming majority of real files have.
    pub fn solid(size: [i32; 3], origin: [i32; 3], name: &str) -> Self {
        let volume = (size[0] * size[1] * size[2]) as usize;
        Self {
            size,
            origin,
            layer0: vec![0; volume],
            layer1: vec![-1; volume],
            palette: vec![block(name)],
            block_position_data: vec![],
            entities: vec![],
            format_version: 1,
        }
    }

    pub fn nbt(&self) -> nbtx::Value {
        let palette_default = compound(vec![
            ("block_palette", nbtx::Value::List(self.palette.clone())),
            (
                "block_position_data",
                nbtx::Value::Compound(
                    self.block_position_data
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect::<HashMap<_, _>>(),
                ),
            ),
        ]);
        let structure = compound(vec![
            (
                "block_indices",
                nbtx::Value::List(vec![int_list(&self.layer0), int_list(&self.layer1)]),
            ),
            ("entities", nbtx::Value::List(self.entities.clone())),
            ("palette", compound(vec![("default", palette_default)])),
        ]);
        compound(vec![
            ("format_version", nbtx::Value::Int(self.format_version)),
            ("size", int_list(&self.size)),
            ("structure", structure),
            ("structure_world_origin", int_list(&self.origin)),
        ])
    }

    pub fn bytes(&self) -> Vec<u8> {
        nbtx::to_le_bytes(&self.nbt()).expect("fixture must serialize")
    }
}
```

Create `crates/construct-core/tests/mcstructure.rs`:

```rust
mod support;

use construct_core::mcstructure::{self, VOID};
use support::{Build, block, compound, int_list};

#[test]
fn a_single_block_structure_decodes() {
    let b = Build::solid([1, 1, 1], [10, 64, -3], "minecraft:stone");
    let s = mcstructure::decode(&b.bytes(), "test").unwrap();

    assert_eq!(s.format_version, 1);
    assert_eq!(s.size, mcstructure::Size { x: 1, y: 1, z: 1 });
    assert_eq!(s.origin, mcstructure::Coord { x: 10, y: 64, z: -3 });
    assert_eq!(s.layers[0], vec![0]);
    assert_eq!(s.layers[1], vec![VOID]);
    assert_eq!(s.palette.len(), 1);
    assert_eq!(s.palette[0].name, "minecraft:stone");
    assert_eq!(s.palette[0].version, 18163713);
    assert!(s.block_position_data.is_empty());
    assert!(s.entities.is_empty());
}

#[test]
fn the_second_layer_carries_waterlogging() {
    // A waterlogged block: the block itself on layer 0, water on layer 1.
    let mut b = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:oak_fence");
    b.palette.push(block("minecraft:water"));
    b.layer1 = vec![1];
    let s = mcstructure::decode(&b.bytes(), "test").unwrap();

    assert_eq!(s.layers[0], vec![0]);
    assert_eq!(s.layers[1], vec![1]);
    assert_eq!(s.palette[1].name, "minecraft:water");
}

#[test]
fn void_gaps_decode_as_negative_one() {
    let mut b = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:stone");
    b.layer0 = vec![0, VOID];
    let s = mcstructure::decode(&b.bytes(), "test").unwrap();
    assert_eq!(s.layers[0], vec![0, VOID]);
}

#[test]
fn block_position_data_decodes_keyed_by_index() {
    let mut b = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:chest");
    b.block_position_data = vec![(
        "1".to_string(),
        compound(vec![(
            "block_entity_data",
            compound(vec![("id", nbtx::Value::String("Chest".into()))]),
        )]),
    )];
    let s = mcstructure::decode(&b.bytes(), "test").unwrap();

    assert_eq!(s.block_position_data.len(), 1);
    assert!(s.block_position_data.contains_key(&1usize));
}

#[test]
fn entities_decode_untouched() {
    let mut b = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:air");
    b.entities = vec![compound(vec![
        ("identifier", nbtx::Value::String("minecraft:pig".into())),
        (
            "Pos",
            nbtx::Value::List(vec![
                nbtx::Value::Float(1.5),
                nbtx::Value::Float(64.0),
                nbtx::Value::Float(-2.5),
            ]),
        ),
    ])];
    let s = mcstructure::decode(&b.bytes(), "test").unwrap();
    assert_eq!(s.entities.len(), 1);
    assert_eq!(s.entities[0], b.entities[0]);
}

#[test]
fn the_real_construct_fixture_decodes() {
    // The one committed real file: proves the model agrees with what the game
    // actually writes, which no builder can establish on its own.
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/construct.mcstructure"
    ))
    .unwrap();
    let s = mcstructure::decode(&bytes, "construct.mcstructure").unwrap();
    assert_eq!(s.size, mcstructure::Size { x: 7, y: 7, z: 7 });
    assert_eq!(s.layers[0].len(), 343);
    assert_eq!(s.layers[1].len(), 343);
    assert_eq!(s.palette.len(), 6);
}

// --- the game's own load-time validation rules, from the reference doc ---

#[test]
fn a_missing_required_field_is_refused() {
    let b = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let nbtx::Value::Compound(mut root) = b.nbt() else {
        unreachable!()
    };
    root.remove("size");
    let bytes = nbtx::to_le_bytes(&nbtx::Value::Compound(root)).unwrap();

    let err = mcstructure::decode(&bytes, "test").unwrap_err();
    assert!(format!("{err}").contains("size"), "error must name the field: {err}");
}

#[test]
fn block_indices_with_other_than_two_layers_is_refused() {
    let b = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let nbtx::Value::Compound(mut root) = b.nbt() else { unreachable!() };
    let nbtx::Value::Compound(mut structure) = root["structure"].clone() else { unreachable!() };
    structure.insert("block_indices".into(), nbtx::Value::List(vec![int_list(&[0])]));
    root.insert("structure".into(), nbtx::Value::Compound(structure));
    let bytes = nbtx::to_le_bytes(&nbtx::Value::Compound(root)).unwrap();

    let err = mcstructure::decode(&bytes, "test").unwrap_err();
    assert!(format!("{err}").contains('2'), "error must say two are required: {err}");
}

#[test]
fn layers_of_different_lengths_are_refused() {
    let mut b = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:stone");
    b.layer1 = vec![VOID];
    let err = mcstructure::decode(&b.bytes(), "test").unwrap_err();
    assert!(format!("{err}").contains("same"), "error must say they must match: {err}");
}

#[test]
fn a_layer_length_that_disagrees_with_size_is_refused() {
    let mut b = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:stone");
    b.layer0 = vec![0, 0, 0];
    b.layer1 = vec![VOID, VOID, VOID];
    let err = mcstructure::decode(&b.bytes(), "test").unwrap_err();
    assert!(format!("{err}").contains("size"), "error must blame size: {err}");
}

#[test]
fn a_missing_default_palette_is_refused() {
    // The doc: "If the `default` palette is not present, loading the structure
    // results in no blocks being placed." Silently producing nothing is worse
    // than refusing.
    let b = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let nbtx::Value::Compound(mut root) = b.nbt() else { unreachable!() };
    let nbtx::Value::Compound(mut structure) = root["structure"].clone() else { unreachable!() };
    structure.insert("palette".into(), compound(vec![]));
    root.insert("structure".into(), nbtx::Value::Compound(structure));
    let bytes = nbtx::to_le_bytes(&nbtx::Value::Compound(root)).unwrap();

    let err = mcstructure::decode(&bytes, "test").unwrap_err();
    assert!(format!("{err}").contains("default"), "error must name it: {err}");
}

#[test]
fn a_negative_size_is_refused() {
    let b = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let nbtx::Value::Compound(mut root) = b.nbt() else { unreachable!() };
    root.insert("size".into(), int_list(&[-1, 1, 1]));
    let bytes = nbtx::to_le_bytes(&nbtx::Value::Compound(root)).unwrap();
    assert!(mcstructure::decode(&bytes, "test").is_err());
}

#[test]
fn bytes_that_are_not_nbt_at_all_are_refused_by_name() {
    let err = mcstructure::decode(b"not nbt", "broken.mcstructure").unwrap_err();
    assert!(
        format!("{err}").contains("broken.mcstructure"),
        "the error must name the file: {err}"
    );
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p construct-core --test mcstructure`
Expected: FAIL to compile — `mcstructure::decode` does not exist.

- [ ] **Step 3: Add the error variant**

In `crates/construct-core/src/error.rs`, after the `BadPack` variant:

```rust
    #[error("malformed structure {what}: {reason}")]
    BadStructureFile { what: String, reason: String },
```

- [ ] **Step 4: Write the NBT accessors**

Create `crates/construct-core/src/mcstructure/nbt.rs`:

```rust
//! Typed field access over `nbtx::Value`.
//!
//! Decoding is field extraction with a specific error per missing or
//! mistyped field. Written as nested `match` arms it becomes unreadable and
//! the errors collapse into one vague message, so the extraction lives here
//! and `decode.rs` reads as a list of fields.

use crate::error::{CoreError, Result};

pub(crate) fn bad(what: &str, reason: impl Into<String>) -> CoreError {
    CoreError::BadStructureFile {
        what: what.to_string(),
        reason: reason.into(),
    }
}

/// A named field of a compound, or an error naming it.
pub(crate) fn field<'a>(v: &'a nbtx::Value, name: &str, what: &str) -> Result<&'a nbtx::Value> {
    match v {
        nbtx::Value::Compound(m) => m
            .get(name)
            .ok_or_else(|| bad(what, format!("required field {name:?} is missing"))),
        _ => Err(bad(what, format!("expected a compound to read {name:?} from"))),
    }
}

pub(crate) fn as_int(v: &nbtx::Value, name: &str, what: &str) -> Result<i32> {
    match v {
        nbtx::Value::Int(i) => Ok(*i),
        _ => Err(bad(what, format!("field {name:?} is not an int"))),
    }
}

pub(crate) fn as_list<'a>(v: &'a nbtx::Value, name: &str, what: &str) -> Result<&'a Vec<nbtx::Value>> {
    match v {
        nbtx::Value::List(l) => Ok(l),
        _ => Err(bad(what, format!("field {name:?} is not a list"))),
    }
}

/// A list of exactly three ints — `size` and `structure_world_origin`.
pub(crate) fn as_triple(v: &nbtx::Value, name: &str, what: &str) -> Result<[i32; 3]> {
    let list = as_list(v, name, what)?;
    if list.len() != 3 {
        return Err(bad(
            what,
            format!("field {name:?} needs exactly 3 values, found {}", list.len()),
        ));
    }
    Ok([
        as_int(&list[0], name, what)?,
        as_int(&list[1], name, what)?,
        as_int(&list[2], name, what)?,
    ])
}

/// A list of ints. The reference doc notes the game treats non-int entries as
/// `0`; this refuses instead, because a file that vague is more likely damaged
/// than intentional and silently rewriting it as air would hide that.
pub(crate) fn as_int_vec(v: &nbtx::Value, name: &str, what: &str) -> Result<Vec<i32>> {
    as_list(v, name, what)?
        .iter()
        .map(|e| as_int(e, name, what))
        .collect()
}
```

- [ ] **Step 5: Write the model and decoder**

Create `crates/construct-core/src/mcstructure/decode.rs`:

```rust
//! Decoding `.mcstructure` bytes into [`Structure`].
//!
//! The validation here mirrors the load-time rules the game itself enforces,
//! documented in `docs/bedrock-mcstructure-files.md`: exactly two index
//! layers, both the same length, that length equal to the product of `size`,
//! and a `default` palette present. Refusing here turns a structure that
//! would fail to load — or load wrong, silently — into an error naming the
//! field.

use super::geometry::{Coord, Size};
use super::nbt::{as_int, as_int_vec, as_list, as_triple, bad, field};
use crate::error::Result;
use std::collections::BTreeMap;

/// The index meaning "no block here": existing terrain is left untouched.
pub const VOID: i32 = -1;

/// One entry of `block_palette`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BlockState {
    pub name: String,
    /// Kept as raw NBT: state values are strings, ints, or bytes depending on
    /// the property, and this tool never needs to interpret them — only to
    /// compare them for palette deduplication.
    pub states: nbtx::Value,
    pub version: i32,
}

/// A decoded `.mcstructure`.
#[derive(Debug, Clone)]
pub struct Structure {
    pub format_version: i32,
    pub size: Size,
    /// Where in the world this was saved. Merge reads this as the piece's
    /// position; entity positions are stored relative to it.
    pub origin: Coord,
    /// The two index layers, each `size.volume()` long. Layer 1 is usually all
    /// [`VOID`] except where a block is waterlogged.
    pub layers: [Vec<i32>; 2],
    pub palette: Vec<BlockState>,
    /// Extra per-block data, keyed by flattened block index. The value is the
    /// whole `<index>` compound — which may hold `block_entity_data`,
    /// `tick_queue_data`, or both — carried verbatim.
    pub block_position_data: BTreeMap<usize, nbtx::Value>,
    /// Entities as raw NBT, exactly as stored. `Pos` is an absolute world
    /// position at save time; see the merge module for why that means merge
    /// never rewrites it.
    pub entities: Vec<nbtx::Value>,
}

pub fn decode(bytes: &[u8], what: &str) -> Result<Structure> {
    let mut cursor = bytes;
    let root: nbtx::Value = nbtx::from_le_bytes(&mut cursor)
        .map_err(|e| bad(what, format!("not readable as little-endian NBT: {e}")))?;

    let format_version = as_int(field(&root, "format_version", what)?, "format_version", what)?;
    let [sx, sy, sz] = as_triple(field(&root, "size", what)?, "size", what)?;
    if sx < 0 || sy < 0 || sz < 0 {
        return Err(bad(what, format!("size has a negative dimension: [{sx}, {sy}, {sz}]")));
    }
    let size = Size { x: sx, y: sy, z: sz };
    let [ox, oy, oz] = as_triple(
        field(&root, "structure_world_origin", what)?,
        "structure_world_origin",
        what,
    )?;

    let structure = field(&root, "structure", what)?;

    let raw_layers = as_list(
        field(structure, "block_indices", what)?,
        "block_indices",
        what,
    )?;
    if raw_layers.len() != 2 {
        return Err(bad(
            what,
            format!(
                "block_indices needs exactly 2 layers, found {}",
                raw_layers.len()
            ),
        ));
    }
    let layer0 = as_int_vec(&raw_layers[0], "block_indices[0]", what)?;
    let layer1 = as_int_vec(&raw_layers[1], "block_indices[1]", what)?;
    if layer0.len() != layer1.len() {
        return Err(bad(
            what,
            format!(
                "the two block_indices layers must be the same length: {} and {}",
                layer0.len(),
                layer1.len()
            ),
        ));
    }
    let volume = usize::try_from(size.volume())
        .map_err(|_| bad(what, format!("size [{sx}, {sy}, {sz}] is too large to address")))?;
    if layer0.len() != volume {
        return Err(bad(
            what,
            format!(
                "block_indices has {} entries but size [{sx}, {sy}, {sz}] needs {volume}",
                layer0.len()
            ),
        ));
    }

    let palette_group = field(structure, "palette", what)?;
    let default = field(palette_group, "default", what).map_err(|_| {
        bad(
            what,
            "no `default` palette; the game places no blocks at all for such a file",
        )
    })?;

    let mut palette = Vec::new();
    for entry in as_list(field(default, "block_palette", what)?, "block_palette", what)? {
        palette.push(BlockState {
            name: match field(entry, "name", what)? {
                nbtx::Value::String(s) => s.clone(),
                _ => return Err(bad(what, "a block_palette entry's `name` is not a string")),
            },
            states: field(entry, "states", what)?.clone(),
            version: as_int(field(entry, "version", what)?, "version", what)?,
        });
    }

    let mut block_position_data = BTreeMap::new();
    if let nbtx::Value::Compound(m) = field(default, "block_position_data", what)? {
        for (key, value) in m {
            let index: usize = key.parse().map_err(|_| {
                bad(
                    what,
                    format!("block_position_data key {key:?} is not a block index"),
                )
            })?;
            if index >= volume {
                return Err(bad(
                    what,
                    format!("block_position_data key {key:?} is outside the structure"),
                ));
            }
            block_position_data.insert(index, value.clone());
        }
    }

    let entities = as_list(field(structure, "entities", what)?, "entities", what)?.clone();

    Ok(Structure {
        format_version,
        size,
        origin: Coord { x: ox, y: oy, z: oz },
        layers: [layer0, layer1],
        palette,
        block_position_data,
        entities,
    })
}
```

- [ ] **Step 6: Export it**

In `crates/construct-core/src/mcstructure/mod.rs`:

```rust
pub mod decode;
pub mod geometry;
pub(crate) mod nbt;

pub use decode::{BlockState, Structure, VOID, decode};
pub use geometry::{BoundingBox, Coord, Size};
```

- [ ] **Step 7: Add the real fixture**

Obtain `construct.mcstructure` from Construct's own behaviour pack — it ships inside the `.mcaddon` this tool downloads, at `Construct[BP]/structures/construct.mcstructure`. If a Construct install is present locally:

```bash
cp "$(construct status --json | python3 -c 'import json,sys; print(json.load(sys.stdin)["behavior"])')/structures/construct.mcstructure" \
   crates/construct-core/tests/fixtures/construct.mcstructure
```

Otherwise run `construct install` first. Append to `crates/construct-core/tests/fixtures/NOTICE`:

```
construct.mcstructure ships inside Construct's own behaviour pack
(Construct[BP]/structures/), from https://github.com/ForestOfLight/Construct.
It is committed here as the one real-world codec fixture: a builder can prove
the codec self-consistent, but only a file the game wrote can prove it agrees
with the game.
```

- [ ] **Step 8: Verify**

Run: `cargo test -p construct-core --test mcstructure && cargo clippy --all-targets -- -D warnings && cargo fmt --all --check`
Expected: 13 tests pass.

- [ ] **Step 9: Commit**

```bash
git add crates/construct-core/src/mcstructure crates/construct-core/src/error.rs \
        crates/construct-core/tests/mcstructure.rs crates/construct-core/tests/support \
        crates/construct-core/tests/fixtures
git commit -m "Decode .mcstructure files, enforcing the game's own load rules"
```

---

### Task 4: `encode` and the round-trip property

**Files:**
- Create: `crates/construct-core/src/mcstructure/encode.rs`
- Modify: `crates/construct-core/src/mcstructure/mod.rs`, `crates/construct-core/tests/mcstructure.rs`

**Interfaces:**
- Consumes: `Structure`, `BlockState`, `VOID` from Task 3.
- Produces: `pub fn encode(s: &Structure, what: &str) -> Result<Vec<u8>>`

- [ ] **Step 1: Write the failing tests**

Append to `crates/construct-core/tests/mcstructure.rs`:

```rust
// --- encoding ---

/// Decoding an encoded structure must give back an equal structure. Byte
/// equality is deliberately *not* asserted: nbtx stores compounds in a
/// HashMap, so key order is not stable, and a golden-file comparison would
/// fail for reasons that mean nothing (spec §12).
fn assert_round_trips(s: &construct_core::mcstructure::Structure) {
    let bytes = mcstructure::encode(s, "test").unwrap();
    let again = mcstructure::decode(&bytes, "test").unwrap();
    assert_eq!(again.format_version, s.format_version);
    assert_eq!(again.size, s.size);
    assert_eq!(again.origin, s.origin);
    assert_eq!(again.layers, s.layers);
    assert_eq!(again.palette, s.palette);
    assert_eq!(again.block_position_data, s.block_position_data);
    assert_eq!(again.entities, s.entities);
}

#[test]
fn a_single_block_structure_round_trips() {
    let b = Build::solid([1, 1, 1], [10, 64, -3], "minecraft:stone");
    assert_round_trips(&mcstructure::decode(&b.bytes(), "test").unwrap());
}

#[test]
fn a_waterlogged_structure_round_trips() {
    let mut b = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:oak_fence");
    b.palette.push(block("minecraft:water"));
    b.layer1 = vec![1];
    assert_round_trips(&mcstructure::decode(&b.bytes(), "test").unwrap());
}

#[test]
fn a_structure_with_void_gaps_round_trips() {
    let mut b = Build::solid([3, 1, 1], [0, 0, 0], "minecraft:stone");
    b.layer0 = vec![0, VOID, 0];
    assert_round_trips(&mcstructure::decode(&b.bytes(), "test").unwrap());
}

#[test]
fn a_structure_with_block_entities_round_trips() {
    let mut b = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:chest");
    b.block_position_data = vec![(
        "1".to_string(),
        compound(vec![(
            "block_entity_data",
            compound(vec![("id", nbtx::Value::String("Chest".into()))]),
        )]),
    )];
    assert_round_trips(&mcstructure::decode(&b.bytes(), "test").unwrap());
}

#[test]
fn a_structure_with_entities_round_trips() {
    let mut b = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:air");
    b.entities = vec![compound(vec![
        ("identifier", nbtx::Value::String("minecraft:pig".into())),
        (
            "Pos",
            nbtx::Value::List(vec![
                nbtx::Value::Float(1.5),
                nbtx::Value::Float(64.0),
                nbtx::Value::Float(-2.5),
            ]),
        ),
    ])];
    assert_round_trips(&mcstructure::decode(&b.bytes(), "test").unwrap());
}

#[test]
fn an_empty_entity_list_survives_encoding() {
    // This is the case that made stage 3 impossible before nbtx was patched:
    // a structure with no entities has an empty list, which stock nbtx wrote
    // five bytes short and could not read back.
    let b = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let s = mcstructure::decode(&b.bytes(), "test").unwrap();
    assert!(s.entities.is_empty());
    let bytes = mcstructure::encode(&s, "test").unwrap();
    assert!(mcstructure::decode(&bytes, "test").is_ok());
}

#[test]
fn the_real_construct_fixture_round_trips() {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/construct.mcstructure"
    ))
    .unwrap();
    let s = mcstructure::decode(&bytes, "construct.mcstructure").unwrap();
    assert_round_trips(&s);

    // The payload length must be preserved exactly. This is the assertion that
    // catches the empty-list defect regressing: each unwritten empty list
    // costs exactly five bytes.
    let re = mcstructure::encode(&s, "construct.mcstructure").unwrap();
    assert_eq!(
        re.len(),
        bytes.len(),
        "re-encoding changed the payload length"
    );
}

#[test]
fn encoding_refuses_a_structure_whose_layers_disagree_with_its_size() {
    // Guards against merge handing the encoder an inconsistent grid: the file
    // would be written and then fail to load in-game, far from the cause.
    let b = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:stone");
    let mut s = mcstructure::decode(&b.bytes(), "test").unwrap();
    s.layers[0].push(0);
    let err = mcstructure::encode(&s, "test").unwrap_err();
    assert!(format!("{err}").contains("size"), "{err}");
}

#[test]
fn encoding_refuses_a_palette_index_out_of_range() {
    let b = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let mut s = mcstructure::decode(&b.bytes(), "test").unwrap();
    s.layers[0] = vec![5]; // palette has one entry
    let err = mcstructure::encode(&s, "test").unwrap_err();
    assert!(format!("{err}").contains("palette"), "{err}");
}
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test -p construct-core --test mcstructure`
Expected: FAIL to compile — `mcstructure::encode` does not exist.

- [ ] **Step 3: Implement**

Create `crates/construct-core/src/mcstructure/encode.rs`:

```rust
//! Encoding [`Structure`] back to `.mcstructure` bytes.
//!
//! The same invariants `decode` enforces are checked again on the way out.
//! They are not redundant: `merge` builds a `Structure` in memory, and a grid
//! that disagrees with its own `size` would be written happily and then fail
//! to load in-game, with nothing pointing back at the merge that caused it.
//!
//! Byte-identical output is not a goal and is not achievable: `nbtx` stores
//! compounds in a `HashMap`, so key order varies between runs. NBT compounds
//! are unordered, so this is cosmetic — but it is why every round-trip test
//! compares decoded values rather than bytes (spec §12).

use super::decode::{Structure, VOID};
use super::nbt::bad;
use crate::error::Result;
use std::collections::HashMap;

fn int_list(v: &[i32]) -> nbtx::Value {
    nbtx::Value::List(v.iter().copied().map(nbtx::Value::Int).collect())
}

pub fn encode(s: &Structure, what: &str) -> Result<Vec<u8>> {
    let volume = usize::try_from(s.size.volume())
        .map_err(|_| bad(what, "size is too large to address"))?;
    for (i, layer) in s.layers.iter().enumerate() {
        if layer.len() != volume {
            return Err(bad(
                what,
                format!(
                    "layer {i} has {} entries but size [{}, {}, {}] needs {volume}",
                    layer.len(),
                    s.size.x,
                    s.size.y,
                    s.size.z
                ),
            ));
        }
    }
    let palette_len = i32::try_from(s.palette.len())
        .map_err(|_| bad(what, "palette is too large"))?;
    for (i, layer) in s.layers.iter().enumerate() {
        if let Some(bad_index) = layer.iter().find(|&&b| b != VOID && (b < 0 || b >= palette_len)) {
            return Err(bad(
                what,
                format!(
                    "layer {i} refers to palette entry {bad_index}, but the palette has {palette_len}"
                ),
            ));
        }
    }
    for &index in s.block_position_data.keys() {
        if index >= volume {
            return Err(bad(
                what,
                format!("block_position_data index {index} is outside the structure"),
            ));
        }
    }

    let block_palette = nbtx::Value::List(
        s.palette
            .iter()
            .map(|b| {
                nbtx::Value::Compound(HashMap::from([
                    ("name".to_string(), nbtx::Value::String(b.name.clone())),
                    ("states".to_string(), b.states.clone()),
                    ("version".to_string(), nbtx::Value::Int(b.version)),
                ]))
            })
            .collect(),
    );

    let block_position_data = nbtx::Value::Compound(
        s.block_position_data
            .iter()
            .map(|(i, v)| (i.to_string(), v.clone()))
            .collect::<HashMap<_, _>>(),
    );

    let default = nbtx::Value::Compound(HashMap::from([
        ("block_palette".to_string(), block_palette),
        ("block_position_data".to_string(), block_position_data),
    ]));

    let structure = nbtx::Value::Compound(HashMap::from([
        (
            "block_indices".to_string(),
            nbtx::Value::List(vec![int_list(&s.layers[0]), int_list(&s.layers[1])]),
        ),
        ("entities".to_string(), nbtx::Value::List(s.entities.clone())),
        (
            "palette".to_string(),
            nbtx::Value::Compound(HashMap::from([("default".to_string(), default)])),
        ),
    ]));

    let root = nbtx::Value::Compound(HashMap::from([
        (
            "format_version".to_string(),
            nbtx::Value::Int(s.format_version),
        ),
        (
            "size".to_string(),
            int_list(&[s.size.x, s.size.y, s.size.z]),
        ),
        ("structure".to_string(), structure),
        (
            "structure_world_origin".to_string(),
            int_list(&[s.origin.x, s.origin.y, s.origin.z]),
        ),
    ]));

    nbtx::to_le_bytes(&root).map_err(|e| bad(what, format!("could not write NBT: {e}")))
}
```

- [ ] **Step 4: Export it**

In `crates/construct-core/src/mcstructure/mod.rs` add `pub mod encode;` and extend the re-export line to `pub use encode::encode;`.

- [ ] **Step 5: Verify**

Run: `cargo test -p construct-core --test mcstructure && cargo clippy --all-targets -- -D warnings && cargo fmt --all --check`
Expected: 22 tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/construct-core/src/mcstructure crates/construct-core/tests/mcstructure.rs
git commit -m "Encode .mcstructure files, with the round-trip property under test"
```

---

### Task 5: Palette unification

**Files:**
- Create: `crates/construct-core/src/merge.rs`
- Modify: `crates/construct-core/src/lib.rs`

**Interfaces:**
- Consumes: `BlockState`, `Structure`.
- Produces: `pub(crate) fn unify_palettes(pieces: &[Structure]) -> (Vec<BlockState>, Vec<Vec<i32>>)` — the merged palette, plus one remap per piece mapping that piece's old index to the merged index.

- [ ] **Step 1: Write the failing tests**

Create `crates/construct-core/src/merge.rs` with only the test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcstructure::{BlockState, Coord, Size, Structure};
    use std::collections::BTreeMap;

    fn state(name: &str) -> BlockState {
        BlockState {
            name: name.to_string(),
            states: nbtx::Value::Compound(std::collections::HashMap::new()),
            version: 1,
        }
    }

    fn piece(palette: Vec<BlockState>) -> Structure {
        Structure {
            format_version: 1,
            size: Size { x: 1, y: 1, z: 1 },
            origin: Coord { x: 0, y: 0, z: 0 },
            layers: [vec![0], vec![-1]],
            palette,
            block_position_data: BTreeMap::new(),
            entities: vec![],
        }
    }

    #[test]
    fn identical_blocks_collapse_to_one_entry() {
        let a = piece(vec![state("minecraft:stone")]);
        let b = piece(vec![state("minecraft:stone")]);
        let (palette, remaps) = unify_palettes(&[a, b]);
        assert_eq!(palette.len(), 1);
        assert_eq!(remaps, vec![vec![0], vec![0]]);
    }

    #[test]
    fn different_blocks_each_get_an_entry_in_first_seen_order() {
        let a = piece(vec![state("minecraft:stone"), state("minecraft:dirt")]);
        let b = piece(vec![state("minecraft:dirt"), state("minecraft:oak_log")]);
        let (palette, remaps) = unify_palettes(&[a, b]);
        assert_eq!(
            palette.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
            vec!["minecraft:stone", "minecraft:dirt", "minecraft:oak_log"]
        );
        assert_eq!(remaps, vec![vec![0, 1], vec![1, 2]]);
    }

    #[test]
    fn blocks_differing_only_in_states_stay_distinct() {
        // A stair facing north and one facing south are the same `name` and the
        // same `version`. Collapsing them would silently rotate half a build.
        let mut open = state("minecraft:oak_door");
        open.states = nbtx::Value::Compound(std::collections::HashMap::from([(
            "open_bit".to_string(),
            nbtx::Value::Byte(1),
        )]));
        let shut = state("minecraft:oak_door");
        let (palette, remaps) = unify_palettes(&[piece(vec![open, shut])]);
        assert_eq!(palette.len(), 2);
        assert_eq!(remaps, vec![vec![0, 1]]);
    }

    #[test]
    fn blocks_differing_only_in_version_stay_distinct() {
        let mut older = state("minecraft:stone");
        older.version = 17879555;
        let newer = state("minecraft:stone");
        let (palette, _) = unify_palettes(&[piece(vec![older, newer])]);
        assert_eq!(palette.len(), 2);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p construct-core merge::tests`
Expected: FAIL to compile — `unify_palettes` does not exist.

- [ ] **Step 3: Implement**

Prepend to `crates/construct-core/src/merge.rs`:

```rust
//! Merging several structures into one that reassembles them at their
//! recorded world origins.
//!
//! The output's `structure_world_origin` is the min corner of the union of
//! every piece's bounding box, which is what makes entity positions need no
//! rewriting: an entity's placed position is `Pos - origin + load_position`,
//! so shifting the origin and the load position together cancels out. See
//! the plan's "Entity positions need no translation" note and §9.

use crate::mcstructure::{BlockState, Structure};
use std::collections::HashMap;

/// Builds one palette covering every piece, plus a per-piece remap from that
/// piece's own indices to the merged palette's.
///
/// Entries are deduplicated on the whole `(name, states, version)` triple.
/// Deduplicating on `name` alone would collapse an open door onto a shut one
/// and a stair facing north onto one facing south.
pub(crate) fn unify_palettes(pieces: &[Structure]) -> (Vec<BlockState>, Vec<Vec<i32>>) {
    let mut palette: Vec<BlockState> = Vec::new();
    let mut seen: HashMap<BlockState, i32> = HashMap::new();
    let mut remaps = Vec::with_capacity(pieces.len());

    for piece in pieces {
        let mut remap = Vec::with_capacity(piece.palette.len());
        for entry in &piece.palette {
            let index = match seen.get(entry) {
                Some(i) => *i,
                None => {
                    let i = palette.len() as i32;
                    palette.push(entry.clone());
                    seen.insert(entry.clone(), i);
                    i
                }
            };
            remap.push(index);
        }
        remaps.push(remap);
    }

    (palette, remaps)
}
```

`BlockState` must be `Eq + Hash`, which Task 3 already derives. `nbtx::Value` implements both.

In `crates/construct-core/src/lib.rs`, add `pub mod merge;` after `pub mod mcstructure;`.

- [ ] **Step 4: Verify**

Run: `cargo test -p construct-core merge && cargo clippy --all-targets -- -D warnings && cargo fmt --all --check`
Expected: 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/construct-core/src/merge.rs crates/construct-core/src/lib.rs
git commit -m "Unify merge palettes on the whole block state, not just the name"
```

---

### Task 6: Merge — geometry, blitting, and overlap

The core of the stage. Produces a merged `Structure` from N pieces.

**Files:**
- Modify: `crates/construct-core/src/merge.rs`, `crates/construct-core/src/error.rs`
- Create: `crates/construct-core/tests/merge.rs`

**Interfaces:**
- Consumes: `unify_palettes`, `Structure`, `BoundingBox`, `Size`, `Coord`, `VOID`.
- Produces:
  - `pub enum OnOverlap { Last, First, Error }` (`Default` = `Last`)
  - `pub struct MergeOptions { pub on_overlap: OnOverlap, pub max_volume: i64 }` with `pub const DEFAULT_MAX_VOLUME: i64 = 64_000_000;`
  - `pub struct Overlap { pub count: u64, pub pieces: Vec<String> }`
  - `pub struct MergeReport { pub structure: Structure, pub overlaps: Vec<Overlap>, pub warnings: Vec<String> }`
  - `pub fn merge(pieces: &[(String, Structure)], options: &MergeOptions) -> Result<MergeReport>`
  - `CoreError::MergeRefused { reason: String }`

- [ ] **Step 1: Write the failing tests**

Create `crates/construct-core/tests/merge.rs`:

```rust
mod support;

use construct_core::mcstructure::{self, Coord, Size, Structure, VOID};
use construct_core::merge::{self, MergeOptions, OnOverlap};
use support::{Build, block};

fn decode(b: &Build) -> Structure {
    mcstructure::decode(&b.bytes(), "test").unwrap()
}

fn named(name: &str, b: &Build) -> (String, Structure) {
    (name.to_string(), decode(b))
}

/// The block at a world coordinate on a layer, as a palette *name*, or None
/// for void. Stating assertions in world space keeps them readable and
/// independent of the merged grid's own indexing.
fn block_at(s: &Structure, layer: usize, world: Coord) -> Option<&str> {
    let local = Coord {
        x: world.x - s.origin.x,
        y: world.y - s.origin.y,
        z: world.z - s.origin.z,
    };
    let i = s.size.index_of(local)?;
    let index = s.layers[layer][i];
    if index == VOID {
        return None;
    }
    Some(s.palette[index as usize].name.as_str())
}

#[test]
fn merging_one_structure_returns_it_unchanged() {
    // Spec §12's merge-placement property, base case.
    let b = Build::solid([2, 3, 4], [10, 64, -8], "minecraft:stone");
    let one = decode(&b);
    let out = merge::merge(&[named("only", &b)], &MergeOptions::default())
        .unwrap()
        .structure;

    assert_eq!(out.size, one.size);
    assert_eq!(out.origin, one.origin);
    assert_eq!(out.layers, one.layers);
    assert_eq!(
        out.palette.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        one.palette.iter().map(|p| p.name.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn two_disjoint_pieces_land_at_their_world_positions() {
    let a = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let b = Build::solid([1, 1, 1], [3, 0, 0], "minecraft:dirt");
    let out = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default())
        .unwrap()
        .structure;

    assert_eq!(out.origin, Coord { x: 0, y: 0, z: 0 });
    assert_eq!(out.size, Size { x: 4, y: 1, z: 1 });
    assert_eq!(block_at(&out, 0, Coord { x: 0, y: 0, z: 0 }), Some("minecraft:stone"));
    assert_eq!(block_at(&out, 0, Coord { x: 3, y: 0, z: 0 }), Some("minecraft:dirt"));
}

#[test]
fn the_gap_between_pieces_is_void_not_air() {
    // Void leaves existing terrain alone; air would carve holes in whatever
    // the merged structure is placed over (§9).
    let a = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let b = Build::solid([1, 1, 1], [3, 0, 0], "minecraft:dirt");
    let out = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default())
        .unwrap()
        .structure;

    assert_eq!(block_at(&out, 0, Coord { x: 1, y: 0, z: 0 }), None);
    assert_eq!(block_at(&out, 0, Coord { x: 2, y: 0, z: 0 }), None);
    let void_count = out.layers[0].iter().filter(|&&i| i == VOID).count();
    assert_eq!(void_count, 2);
}

#[test]
fn the_merged_origin_is_the_minimum_corner_including_negatives() {
    let a = Build::solid([1, 1, 1], [5, 5, 5], "minecraft:stone");
    let b = Build::solid([1, 1, 1], [-2, 0, 3], "minecraft:dirt");
    let out = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default())
        .unwrap()
        .structure;
    assert_eq!(out.origin, Coord { x: -2, y: 0, z: 3 });
    assert_eq!(out.size, Size { x: 8, y: 6, z: 3 });
}

#[test]
fn a_piece_contributes_only_where_it_is_not_void() {
    // Piece b is void at its own x=0, so a's block there must survive even
    // though b comes later in argument order.
    let a = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:stone");
    let mut b = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:dirt");
    b.layer0 = vec![VOID, 0];
    let out = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default())
        .unwrap()
        .structure;

    assert_eq!(block_at(&out, 0, Coord { x: 0, y: 0, z: 0 }), Some("minecraft:stone"));
    assert_eq!(block_at(&out, 0, Coord { x: 1, y: 0, z: 0 }), Some("minecraft:dirt"));
}

#[test]
fn the_second_layer_merges_independently_of_the_first() {
    let mut a = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:oak_fence");
    a.palette.push(block("minecraft:water"));
    a.layer1 = vec![1];
    let b = Build::solid([1, 1, 1], [1, 0, 0], "minecraft:stone");
    let out = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default())
        .unwrap()
        .structure;

    assert_eq!(block_at(&out, 1, Coord { x: 0, y: 0, z: 0 }), Some("minecraft:water"));
    assert_eq!(block_at(&out, 1, Coord { x: 1, y: 0, z: 0 }), None);
}

// --- overlap ---

#[test]
fn the_last_piece_wins_an_overlap_by_default() {
    let a = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let b = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:dirt");
    // Distinct origins are required, so shift b's *size* to overlap instead.
    let mut a = a;
    a.size = [2, 1, 1];
    a.layer0 = vec![0, 0];
    a.layer1 = vec![VOID, VOID];
    let mut b = b;
    b.origin = [1, 0, 0];

    let report = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default()).unwrap();
    assert_eq!(
        block_at(&report.structure, 0, Coord { x: 1, y: 0, z: 0 }),
        Some("minecraft:dirt")
    );
    assert_eq!(report.overlaps.len(), 1);
    assert_eq!(report.overlaps[0].count, 1);
    assert_eq!(report.overlaps[0].pieces, vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn the_first_piece_wins_under_on_overlap_first() {
    let mut a = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:stone");
    a.layer0 = vec![0, 0];
    let mut b = Build::solid([1, 1, 1], [1, 0, 0], "minecraft:dirt");
    b.layer0 = vec![0];

    let options = MergeOptions {
        on_overlap: OnOverlap::First,
        ..MergeOptions::default()
    };
    let report = merge::merge(&[named("a", &a), named("b", &b)], &options).unwrap();
    assert_eq!(
        block_at(&report.structure, 0, Coord { x: 1, y: 0, z: 0 }),
        Some("minecraft:stone")
    );
    assert_eq!(report.overlaps[0].count, 1);
}

#[test]
fn on_overlap_error_refuses_and_names_the_pieces() {
    let mut a = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:stone");
    a.layer0 = vec![0, 0];
    let b = Build::solid([1, 1, 1], [1, 0, 0], "minecraft:dirt");

    let options = MergeOptions {
        on_overlap: OnOverlap::Error,
        ..MergeOptions::default()
    };
    let err = merge::merge(&[named("a", &a), named("b", &b)], &options).unwrap_err();
    let text = format!("{err}");
    assert!(text.contains('a') && text.contains('b'), "must name both: {text}");
}

#[test]
fn a_void_cell_is_not_an_overlap() {
    // Two pieces occupying the same space, but only one has a block there.
    // Reporting that as an overlap would cry wolf on every merge.
    let mut a = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:stone");
    a.layer0 = vec![0, VOID];
    let mut b = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:dirt");
    b.layer0 = vec![VOID, 0];
    b.origin = [0, 1, 0];

    let report = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default()).unwrap();
    assert!(report.overlaps.is_empty(), "{:?}", report.overlaps);
}

// --- refusals ---

#[test]
fn identical_origins_are_refused() {
    // The pieces would stack in one spot (§9).
    let a = Build::solid([1, 1, 1], [4, 4, 4], "minecraft:stone");
    let b = Build::solid([1, 1, 1], [4, 4, 4], "minecraft:dirt");
    let err = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default()).unwrap_err();
    assert!(format!("{err}").contains("origin"), "{err}");
}

#[test]
fn all_zero_origins_are_refused_too() {
    let a = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let b = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:dirt");
    assert!(merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default()).is_err());
}

#[test]
fn a_mix_of_zero_and_real_origins_proceeds_with_a_warning() {
    // [0,0,0] is indistinguishable from a legitimate save at world origin, so
    // refusing would be wrong about as often as it was right (§9).
    let a = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let b = Build::solid([1, 1, 1], [40, 0, 0], "minecraft:dirt");
    let report = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default()).unwrap();
    assert!(
        report.warnings.iter().any(|w| w.contains('a')),
        "expected a warning naming the piece at the origin: {:?}",
        report.warnings
    );
}

#[test]
fn a_union_too_large_to_allocate_is_refused() {
    let a = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let b = Build::solid([1, 1, 1], [100_000, 0, 0], "minecraft:dirt");
    let err = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default()).unwrap_err();
    assert!(format!("{err}").contains("large"), "{err}");
}

#[test]
fn an_oversized_but_allocatable_union_warns_rather_than_refusing() {
    // Minecraft loads structures past 64*256*64 without trouble, so this is a
    // performance warning, not a limit (§9).
    let a = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let b = Build::solid([1, 1, 1], [200, 0, 200], "minecraft:dirt");
    let report = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default()).unwrap();
    assert!(
        report.warnings.iter().any(|w| w.contains("large")),
        "expected a size warning: {:?}",
        report.warnings
    );
}

#[test]
fn merging_nothing_is_refused() {
    let err = merge::merge(&[], &MergeOptions::default()).unwrap_err();
    assert!(format!("{err}").contains("nothing") || format!("{err}").contains("no structures"), "{err}");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p construct-core --test merge`
Expected: FAIL to compile.

- [ ] **Step 3: Add the error variant**

In `crates/construct-core/src/error.rs`, after `BadStructureFile`:

```rust
    #[error("cannot merge: {reason}")]
    MergeRefused { reason: String },
```

- [ ] **Step 4: Implement**

Add to `crates/construct-core/src/merge.rs` (above the test module):

```rust
use crate::error::{CoreError, Result};
use crate::mcstructure::{BoundingBox, Coord, Size, Structure, VOID};
use std::collections::BTreeMap;

/// How to resolve two pieces both contributing a block at one position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OnOverlap {
    /// The piece appearing later in the argument list wins.
    #[default]
    Last,
    /// The piece appearing earlier in the argument list wins.
    First,
    /// Refuse the merge.
    Error,
}

#[derive(Debug, Clone)]
pub struct MergeOptions {
    pub on_overlap: OnOverlap,
    /// Refuse a union bounding box with more blocks than this. An allocation
    /// guard, not a game limit: each block costs 8 bytes across the two `i32`
    /// layers, so the default is roughly half a gigabyte of index data.
    pub max_volume: i64,
}

/// 64 million blocks — about 512 MB across both index layers.
pub const DEFAULT_MAX_VOLUME: i64 = 64_000_000;

/// Beyond this the result still loads, but placing it is slow. The vanilla
/// structure-block save limit, from the reference documentation.
const PERFORMANCE_WARN_VOLUME: i64 = 64 * 256 * 64;

impl Default for MergeOptions {
    fn default() -> Self {
        Self {
            on_overlap: OnOverlap::default(),
            max_volume: DEFAULT_MAX_VOLUME,
        }
    }
}

/// One pair of pieces that contended for at least one position.
#[derive(Debug, Clone)]
pub struct Overlap {
    pub count: u64,
    /// The names of the two contending pieces, loser first.
    pub pieces: Vec<String>,
}

#[derive(Debug)]
pub struct MergeReport {
    pub structure: Structure,
    pub overlaps: Vec<Overlap>,
    pub warnings: Vec<String>,
}

fn refused(reason: impl Into<String>) -> CoreError {
    CoreError::MergeRefused {
        reason: reason.into(),
    }
}

/// Merges `pieces` into one structure positioned at the min corner of the
/// union of their bounding boxes.
pub fn merge(pieces: &[(String, Structure)], options: &MergeOptions) -> Result<MergeReport> {
    if pieces.is_empty() {
        return Err(refused("no structures to merge"));
    }

    // §9: identical origins mean the pieces would stack in one spot. This
    // includes the all-zero case, which is what an unset origin looks like.
    if pieces.len() > 1 {
        let first = pieces[0].1.origin;
        if pieces.iter().all(|(_, s)| s.origin == first) {
            return Err(refused(format!(
                "every structure has the same origin [{}, {}, {}], so the pieces would stack \
                 in one spot rather than reassemble",
                first.x, first.y, first.z
            )));
        }
    }

    let mut warnings = Vec::new();

    // §9: the mixed case is *not* refused — [0,0,0] is indistinguishable from
    // a legitimate save at world origin.
    if pieces.len() > 1 {
        let zeroed: Vec<&str> = pieces
            .iter()
            .filter(|(_, s)| s.origin == (Coord { x: 0, y: 0, z: 0 }))
            .map(|(n, _)| n.as_str())
            .collect();
        if !zeroed.is_empty() {
            warnings.push(format!(
                "origin may be unset on: {} — these will be placed at world origin",
                zeroed.join(", ")
            ));
        }
    }

    let boxes: Vec<BoundingBox> = pieces
        .iter()
        .map(|(_, s)| BoundingBox::of(s.origin, s.size))
        .collect();
    let union = BoundingBox::union(&boxes).ok_or_else(|| refused("no structures to merge"))?;
    let size = union.size();
    let volume = size.volume();

    if volume > options.max_volume {
        return Err(refused(format!(
            "the merged bounding box is {} x {} x {} = {volume} blocks, too large to build in \
             memory (limit {})",
            size.x, size.y, size.z, options.max_volume
        )));
    }
    if volume > PERFORMANCE_WARN_VOLUME {
        warnings.push(format!(
            "the merged structure is large: {} x {} x {} = {volume} blocks. It will load, but \
             placing it may be slow",
            size.x, size.y, size.z
        ));
    }

    let structures: Vec<Structure> = pieces.iter().map(|(_, s)| s.clone()).collect();
    let (palette, remaps) = unify_palettes(&structures);

    let cells = usize::try_from(volume).map_err(|_| refused("merged size is too large to address"))?;
    let mut layers = [vec![VOID; cells], vec![VOID; cells]];
    // Which piece last wrote each cell of each layer, so an overlap can name
    // both contenders and `block_position_data` can follow the winner.
    let mut owner: [Vec<Option<usize>>; 2] = [vec![None; cells], vec![None; cells]];
    let mut overlaps: BTreeMap<(usize, usize), u64> = BTreeMap::new();

    for (p, (_, piece)) in pieces.iter().enumerate() {
        for layer in 0..2 {
            for (i, &index) in piece.layers[layer].iter().enumerate() {
                if index == VOID {
                    continue;
                }
                let Some(local) = piece.size.coord_of(i) else {
                    continue;
                };
                let world = Coord {
                    x: piece.origin.x + local.x,
                    y: piece.origin.y + local.y,
                    z: piece.origin.z + local.z,
                };
                let target = Coord {
                    x: world.x - union.min.x,
                    y: world.y - union.min.y,
                    z: world.z - union.min.z,
                };
                let Some(out_i) = size.index_of(target) else {
                    continue;
                };

                let remapped = remaps[p][index as usize];
                match owner[layer][out_i] {
                    None => {
                        layers[layer][out_i] = remapped;
                        owner[layer][out_i] = Some(p);
                    }
                    Some(previous) => {
                        *overlaps.entry((previous, p)).or_default() += 1;
                        if options.on_overlap == OnOverlap::Last {
                            layers[layer][out_i] = remapped;
                            owner[layer][out_i] = Some(p);
                        }
                    }
                }
            }
        }
    }

    let overlaps: Vec<Overlap> = overlaps
        .into_iter()
        .map(|((a, b), count)| Overlap {
            count,
            pieces: vec![pieces[a].0.clone(), pieces[b].0.clone()],
        })
        .collect();

    if options.on_overlap == OnOverlap::Error && !overlaps.is_empty() {
        let detail: Vec<String> = overlaps
            .iter()
            .map(|o| format!("{} blocks between {} and {}", o.count, o.pieces[0], o.pieces[1]))
            .collect();
        return Err(refused(format!(
            "structures overlap and --on-overlap=error was given: {}",
            detail.join("; ")
        )));
    }

    Ok(MergeReport {
        structure: Structure {
            format_version: pieces[0].1.format_version,
            size,
            origin: union.min,
            layers,
            palette,
            block_position_data: BTreeMap::new(),
            entities: Vec::new(),
        },
        overlaps,
        warnings,
    })
}
```

`block_position_data` and `entities` are filled in by Task 7; leaving them empty here keeps this task's diff reviewable and its tests honest about what they cover.

- [ ] **Step 5: Verify**

Run: `cargo test -p construct-core --test merge && cargo test -p construct-core merge && cargo clippy --all-targets -- -D warnings && cargo fmt --all --check`
Expected: all merge tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/construct-core/src/merge.rs crates/construct-core/src/error.rs \
        crates/construct-core/tests/merge.rs
git commit -m "Merge structures into the union of their bounding boxes"
```

---

### Task 7: `block_position_data` follows the winner; entities carry through

**Files:**
- Modify: `crates/construct-core/src/merge.rs`, `crates/construct-core/tests/merge.rs`

**Interfaces:**
- Consumes: everything from Task 6.
- Produces: no new public names; `MergeReport.structure` now carries `block_position_data` and `entities`.

- [ ] **Step 1: Write the failing tests**

Append to `crates/construct-core/tests/merge.rs`:

```rust
// --- block_position_data and entities ---

use support::compound;

fn chest_at(b: &mut Build, index: &str, label: &str) {
    b.block_position_data.push((
        index.to_string(),
        compound(vec![(
            "block_entity_data",
            compound(vec![
                ("id", nbtx::Value::String("Chest".into())),
                ("label", nbtx::Value::String(label.into())),
            ]),
        )]),
    ));
}

fn label_at(s: &Structure, world: Coord) -> Option<String> {
    let local = Coord {
        x: world.x - s.origin.x,
        y: world.y - s.origin.y,
        z: world.z - s.origin.z,
    };
    let i = s.size.index_of(local)?;
    let entry = s.block_position_data.get(&i)?;
    let nbtx::Value::Compound(m) = entry else { return None };
    let nbtx::Value::Compound(bed) = m.get("block_entity_data")? else { return None };
    match bed.get("label")? {
        nbtx::Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

#[test]
fn block_position_data_keys_are_recomputed_for_the_merged_grid() {
    // The index is relative to a grid whose dimensions changed, so carrying
    // the key across unchanged would attach the data to the wrong block.
    let mut a = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:chest");
    chest_at(&mut a, "0", "from-a");
    let b = Build::solid([1, 1, 1], [0, 0, 4], "minecraft:stone");

    let out = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default())
        .unwrap()
        .structure;

    assert_eq!(label_at(&out, Coord { x: 0, y: 0, z: 0 }).as_deref(), Some("from-a"));
    assert_eq!(out.block_position_data.len(), 1);
}

#[test]
fn block_position_data_follows_the_block_that_won_the_overlap() {
    // §9: "a chest's contents survive from a block that lost the overlap and
    // end up attached to the wrong thing" is the failure this prevents. It is
    // invisible in a block-only comparison, so it is asserted explicitly.
    let mut a = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:chest");
    a.layer0 = vec![0, 0];
    chest_at(&mut a, "1", "from-a");
    let mut b = Build::solid([1, 1, 1], [1, 0, 0], "minecraft:chest");
    chest_at(&mut b, "0", "from-b");

    let out = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default())
        .unwrap()
        .structure;

    // b is last, so b wins the contested cell — and its chest data must be the
    // data that survives there.
    assert_eq!(label_at(&out, Coord { x: 1, y: 0, z: 0 }).as_deref(), Some("from-b"));
}

#[test]
fn block_position_data_follows_the_winner_under_on_overlap_first() {
    let mut a = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:chest");
    a.layer0 = vec![0, 0];
    chest_at(&mut a, "1", "from-a");
    let mut b = Build::solid([1, 1, 1], [1, 0, 0], "minecraft:chest");
    chest_at(&mut b, "0", "from-b");

    let options = MergeOptions {
        on_overlap: OnOverlap::First,
        ..MergeOptions::default()
    };
    let out = merge::merge(&[named("a", &a), named("b", &b)], &options)
        .unwrap()
        .structure;

    assert_eq!(label_at(&out, Coord { x: 1, y: 0, z: 0 }).as_deref(), Some("from-a"));
}

#[test]
fn tick_queue_data_is_carried_along_with_block_entity_data() {
    // The `<index>` compound may hold block_entity_data, tick_queue_data, or
    // both. Merge carries the whole compound rather than picking fields out.
    let mut a = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:water");
    a.block_position_data.push((
        "0".to_string(),
        compound(vec![(
            "tick_queue_data",
            nbtx::Value::List(vec![compound(vec![("tick_delay", nbtx::Value::Int(5))])]),
        )]),
    ));
    let b = Build::solid([1, 1, 1], [0, 0, 3], "minecraft:stone");

    let out = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default())
        .unwrap()
        .structure;

    let entry = out.block_position_data.values().next().unwrap();
    let nbtx::Value::Compound(m) = entry else { panic!() };
    assert!(m.contains_key("tick_queue_data"));
}

#[test]
fn entities_are_carried_with_their_positions_untouched() {
    // Entity Pos is an absolute world position, and placement subtracts the
    // structure's origin. Because the merged origin is the union's min corner,
    // that subtraction already puts every entity in the right place — so
    // rewriting Pos here would move entities by the origin delta, twice.
    let mut a = Build::solid([1, 1, 1], [10, 0, 0], "minecraft:air");
    let pos = nbtx::Value::List(vec![
        nbtx::Value::Float(10.5),
        nbtx::Value::Float(64.0),
        nbtx::Value::Float(0.5),
    ]);
    a.entities = vec![compound(vec![
        ("identifier", nbtx::Value::String("minecraft:pig".into())),
        ("Pos", pos.clone()),
    ])];
    let b = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:stone");

    let out = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default())
        .unwrap()
        .structure;

    assert_eq!(out.entities.len(), 1);
    let nbtx::Value::Compound(e) = &out.entities[0] else { panic!() };
    assert_eq!(e.get("Pos"), Some(&pos), "entity Pos must not be rewritten");
}

#[test]
fn entities_from_every_piece_are_collected_in_argument_order() {
    let mut a = Build::solid([1, 1, 1], [0, 0, 0], "minecraft:air");
    a.entities = vec![compound(vec![("identifier", nbtx::Value::String("a".into()))])];
    let mut b = Build::solid([1, 1, 1], [5, 0, 0], "minecraft:air");
    b.entities = vec![compound(vec![("identifier", nbtx::Value::String("b".into()))])];

    let out = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default())
        .unwrap()
        .structure;
    assert_eq!(out.entities.len(), 2);
}

#[test]
fn a_merged_structure_encodes_and_decodes_back_equal() {
    let mut a = Build::solid([2, 1, 1], [0, 0, 0], "minecraft:chest");
    a.layer0 = vec![0, 0];
    chest_at(&mut a, "1", "from-a");
    let b = Build::solid([1, 1, 1], [0, 0, 4], "minecraft:stone");

    let merged = merge::merge(&[named("a", &a), named("b", &b)], &MergeOptions::default())
        .unwrap()
        .structure;
    let bytes = mcstructure::encode(&merged, "merged").unwrap();
    let again = mcstructure::decode(&bytes, "merged").unwrap();

    assert_eq!(again.size, merged.size);
    assert_eq!(again.origin, merged.origin);
    assert_eq!(again.layers, merged.layers);
    assert_eq!(again.palette, merged.palette);
    assert_eq!(again.block_position_data, merged.block_position_data);
    assert_eq!(again.entities, merged.entities);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p construct-core --test merge`
Expected: the seven new tests FAIL — merge currently leaves both fields empty.

- [ ] **Step 3: Implement**

In `crates/construct-core/src/merge.rs`, inside the per-piece loop, after the `owner[layer][out_i] = Some(p)` assignments, the winner is already tracked. Add position-data and entity handling after the loop, replacing the `MergeReport` construction.

First, record where each piece's cells landed. Add before the piece loop:

```rust
    // Where each piece's block indices landed in the merged grid, so
    // `block_position_data` can be moved to the same cell without recomputing
    // the coordinate arithmetic a second time.
    let mut placement: Vec<BTreeMap<usize, usize>> = vec![BTreeMap::new(); pieces.len()];
```

Inside the loop, immediately after `let Some(out_i) = size.index_of(target) else { continue; };`, record the mapping (layer 0 only — `block_position_data` is layer-agnostic, since two block entities cannot share a block space):

```rust
                if layer == 0 {
                    placement[p].insert(i, out_i);
                }
```

Then replace the `MergeReport { structure: Structure { … } }` construction's `block_position_data` and `entities` fields:

```rust
    // `block_position_data` follows the block that won its cell. Walking the
    // pieces in order and letting a later winner overwrite an earlier entry
    // reproduces the same resolution the blit used, so a chest's contents can
    // never end up attached to a block that lost (§9).
    let mut block_position_data = BTreeMap::new();
    for (p, (_, piece)) in pieces.iter().enumerate() {
        for (&local_index, data) in &piece.block_position_data {
            let Some(&out_i) = placement[p].get(&local_index) else {
                continue;
            };
            if owner[0][out_i] == Some(p) {
                block_position_data.insert(out_i, data.clone());
            } else {
                block_position_data.remove(&out_i);
            }
        }
    }

    // Entities are carried verbatim. `Pos` is an absolute world position and
    // placement computes `Pos - origin + load_position`; since the merged
    // origin is the union's min corner, that arithmetic already lands every
    // entity correctly. Rewriting `Pos` here would shift them twice.
    let entities: Vec<nbtx::Value> = pieces
        .iter()
        .flat_map(|(_, s)| s.entities.iter().cloned())
        .collect();
```

and use `block_position_data` and `entities` in the returned `Structure`.

The `else { block_position_data.remove(&out_i); }` arm matters: a losing piece processed *after* the winner must not leave stale data behind, and under `OnOverlap::First` the winner is the earlier piece.

- [ ] **Step 4: Verify**

Run: `cargo test -p construct-core --test merge && cargo clippy --all-targets -- -D warnings && cargo fmt --all --check`
Expected: all merge tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/construct-core/src/merge.rs crates/construct-core/tests/merge.rs
git commit -m "Carry block_position_data with the winning block, and entities verbatim"
```

---

### Task 8: `export --merge` on the command line

**Files:**
- Modify: `crates/construct-cli/src/cli.rs`, `crates/construct-cli/src/commands/export.rs`, `crates/construct-cli/src/main.rs`, `crates/construct-cli/tests/cli.rs`

**Interfaces:**
- Consumes: `merge::{merge, MergeOptions, OnOverlap, MergeReport}`, `mcstructure::{decode, encode}`.
- Produces: `construct export <world> <s1> <s2>... --merge -o FILE`.

- [ ] **Step 1: Write the failing tests**

Append to `crates/construct-cli/tests/cli.rs`:

```rust
#[test]
fn merge_without_o_is_a_usage_error() {
    // §5: "--merge requires -o". Without it there is no single name to derive.
    let root = world_with_construct(&[]);
    let out = bin()
        .args([
            "export",
            "Test",
            "a",
            "b",
            "--merge",
            "--com-mojang",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn merge_writes_one_file_from_several_structures() {
    let a = merge_fixture([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let b = merge_fixture([1, 1, 1], [3, 0, 0], "minecraft:dirt");
    let root = world_with_construct(&[("north", &a), ("tower", &b)]);
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("merged.mcstructure");

    let out = bin()
        .args([
            "export",
            "Test",
            "north",
            "tower",
            "--merge",
            "-o",
            target.to_str().unwrap(),
            "--json",
            "--com-mojang",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(target.is_file());

    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["schema"], 1);
    assert_eq!(v["merged"]["sources"].as_array().unwrap().len(), 2);
    assert_eq!(v["merged"]["size"], serde_json::json!([4, 1, 1]));
    assert_eq!(v["merged"]["origin"], serde_json::json!([0, 0, 0]));

    // The written file is a real .mcstructure covering both pieces.
    let bytes = std::fs::read(&target).unwrap();
    let s = construct_core::mcstructure::decode(&bytes, "merged").unwrap();
    assert_eq!(s.size, construct_core::mcstructure::Size { x: 4, y: 1, z: 1 });
}

#[test]
fn merge_refuses_an_existing_target_without_force() {
    let a = merge_fixture([1, 1, 1], [0, 0, 0], "minecraft:stone");
    let b = merge_fixture([1, 1, 1], [3, 0, 0], "minecraft:dirt");
    let root = world_with_construct(&[("north", &a), ("tower", &b)]);
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("merged.mcstructure");
    std::fs::write(&target, b"existing").unwrap();

    let out = bin()
        .args([
            "export", "Test", "north", "tower", "--merge", "-o",
            target.to_str().unwrap(), "--com-mojang", root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(std::fs::read(&target).unwrap(), b"existing");
}

#[test]
fn merge_reports_overlap_on_stderr_and_in_the_payload() {
    let mut a = support_build([2, 1, 1], [0, 0, 0], "minecraft:stone");
    a.layer0 = vec![0, 0];
    let b = support_build([1, 1, 1], [1, 0, 0], "minecraft:dirt");
    let root = world_with_construct(&[("north", &a.bytes()), ("tower", &b.bytes())]);
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("merged.mcstructure");

    let out = bin()
        .args([
            "export", "Test", "north", "tower", "--merge", "-o",
            target.to_str().unwrap(), "--json", "--com-mojang",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("overlap"), "expected an overlap warning: {stderr}");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        v["warnings"].as_array().is_some_and(|w| !w.is_empty()),
        "overlap must appear in the payload too: {v}"
    );
}

#[test]
fn merge_with_on_overlap_error_exits_1_and_writes_nothing() {
    let mut a = support_build([2, 1, 1], [0, 0, 0], "minecraft:stone");
    a.layer0 = vec![0, 0];
    let b = support_build([1, 1, 1], [1, 0, 0], "minecraft:dirt");
    let root = world_with_construct(&[("north", &a.bytes()), ("tower", &b.bytes())]);
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("merged.mcstructure");

    let out = bin()
        .args([
            "export", "Test", "north", "tower", "--merge", "--on-overlap", "error",
            "-o", target.to_str().unwrap(), "--com-mojang",
            root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    assert!(!target.exists(), "a refused merge must write nothing");
}

#[test]
fn merging_one_structure_is_allowed() {
    // The degenerate case is useful: it re-encodes a structure through the
    // codec, and the identical-origin refusal must not fire on a single piece.
    let a = merge_fixture([2, 2, 2], [7, 7, 7], "minecraft:stone");
    let root = world_with_construct(&[("solo", &a)]);
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("one.mcstructure");

    let out = bin()
        .args([
            "export", "Test", "solo", "--merge", "-o", target.to_str().unwrap(),
            "--com-mojang", root.path().to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(target.is_file());
}
```

Add these helpers near the other fixture helpers in `crates/construct-cli/tests/cli.rs`. The CLI test crate cannot reach `construct-core`'s `tests/support`, so it carries its own minimal builder:

```rust
/// A minimal `.mcstructure` builder for CLI tests. `construct-core`'s test
/// support module is not reachable from this crate, so the few fields these
/// tests need are built here rather than shared.
struct SupportBuild {
    size: [i32; 3],
    origin: [i32; 3],
    layer0: Vec<i32>,
    layer1: Vec<i32>,
    name: String,
}

impl SupportBuild {
    fn bytes(&self) -> Vec<u8> {
        use std::collections::HashMap;
        let c = |pairs: Vec<(&str, nbtx::Value)>| {
            nbtx::Value::Compound(
                pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect::<HashMap<_, _>>(),
            )
        };
        let ints = |v: &[i32]| nbtx::Value::List(v.iter().copied().map(nbtx::Value::Int).collect());
        let palette = c(vec![
            (
                "block_palette",
                nbtx::Value::List(vec![c(vec![
                    ("name", nbtx::Value::String(self.name.clone())),
                    ("states", c(vec![])),
                    ("version", nbtx::Value::Int(18163713)),
                ])]),
            ),
            ("block_position_data", c(vec![])),
        ]);
        let structure = c(vec![
            (
                "block_indices",
                nbtx::Value::List(vec![ints(&self.layer0), ints(&self.layer1)]),
            ),
            ("entities", nbtx::Value::List(vec![])),
            ("palette", c(vec![("default", palette)])),
        ]);
        nbtx::to_le_bytes(&c(vec![
            ("format_version", nbtx::Value::Int(1)),
            ("size", ints(&self.size)),
            ("structure", structure),
            ("structure_world_origin", ints(&self.origin)),
        ]))
        .unwrap()
    }
}

fn support_build(size: [i32; 3], origin: [i32; 3], name: &str) -> SupportBuild {
    let volume = (size[0] * size[1] * size[2]) as usize;
    SupportBuild {
        size,
        origin,
        layer0: vec![0; volume],
        layer1: vec![-1; volume],
        name: name.to_string(),
    }
}

fn merge_fixture(size: [i32; 3], origin: [i32; 3], name: &str) -> Vec<u8> {
    support_build(size, origin, name).bytes()
}
```

`world_with_construct` already exists in this file and takes `&[(&str, &[u8])]`; the calls above pass `&a` where `a: Vec<u8>`, which coerces.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p construct-cli merge`
Expected: FAIL — `--merge` is not a recognised argument.

- [ ] **Step 3: Add the flags**

In `crates/construct-cli/src/cli.rs`, extend `Command::Export`:

```rust
        /// Combine the structures into one, reassembled at their saved world
        /// positions. Requires -o.
        #[arg(long)]
        merge: bool,
        /// How to resolve positions where two structures both have a block.
        #[arg(long, value_name = "MODE", default_value = "last")]
        on_overlap: OverlapArg,
```

and, beside `OnOff`:

```rust
#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
pub enum OverlapArg {
    /// The structure named later on the command line wins.
    Last,
    /// The structure named earlier on the command line wins.
    First,
    /// Refuse the merge.
    Error,
}

impl From<OverlapArg> for construct_core::merge::OnOverlap {
    fn from(v: OverlapArg) -> Self {
        match v {
            OverlapArg::Last => Self::Last,
            OverlapArg::First => Self::First,
            OverlapArg::Error => Self::Error,
        }
    }
}
```

- [ ] **Step 4: Wire the usage rules in `main.rs`**

In the `Command::Export` arm, `-o` with several structures is already a usage error. `--merge` inverts that: it *requires* `-o` and permits several structures. Replace the existing guard:

```rust
        Command::Export {
            world,
            structures,
            output,
            merge,
            on_overlap,
        } => {
            if *merge && output.is_none() {
                // --merge produces one file, and there is no structure name to
                // derive it from — the result is not any one of the inputs.
                return Err(usage(
                    "--merge writes a single file and needs -o to name it:\n  \
                     construct export <world> <s1> <s2>... --merge -o merged.mcstructure",
                ));
            }
            if !*merge && structures.len() > 1 && output.is_some() {
                return Err(usage(
                    "-o names a single file, but several structures were given. Drop -o to \
                     write one file each, or add --merge to combine them into one.",
                ));
            }
            let w = resolve_world(world)?;
            commands::export::run(
                &w,
                &installations,
                structures,
                output.as_deref(),
                cli.source.map(Into::into),
                cli.force,
                *merge,
                (*on_overlap).into(),
                out,
            )
        }
```

Match the existing usage-error helper in this file rather than introducing a new one — find how the current "several structures with -o" error is raised and follow it exactly.

Add an error help arm beside the others:

```rust
        CoreError::MergeRefused { .. } => {
            eprintln!(
                "\nNothing was written. Check that the structures were saved at different \
                 places in the world — merge reassembles them at their recorded positions."
            );
        }
        CoreError::BadStructureFile { .. } => {
            eprintln!("\nThe file is not a readable .mcstructure.");
        }
```

- [ ] **Step 5: Implement the merge branch**

In `crates/construct-cli/src/commands/export.rs`, extend `run`'s signature with `merge: bool, on_overlap: OnOverlap,` and add, right after the catalog load and before the existing per-structure plan:

```rust
    if merge {
        return run_merge(world, &entries, loaded.store.as_ref(), structures, output, source, force, on_overlap, out);
    }
```

Then add the function:

```rust
#[derive(Serialize)]
struct MergedPayload {
    world: String,
    merged: Merged,
}

#[derive(Serialize)]
struct Merged {
    path: String,
    bytes: u64,
    sources: Vec<String>,
    size: [i32; 3],
    origin: [i32; 3],
    overlaps: Vec<OverlapRow>,
}

#[derive(Serialize)]
struct OverlapRow {
    count: u64,
    pieces: Vec<String>,
}

/// `--merge`: decode every named structure, combine them, and write one file.
///
/// Unlike the plain path this one must decode — merge is the only command that
/// looks inside a `.mcstructure` at all. Everything else copies bytes.
#[allow(clippy::too_many_arguments)]
fn run_merge(
    world: &World,
    entries: &[catalog::Entry],
    store: Option<&construct_core::store::OpenedStore>,
    structures: &[String],
    output: Option<&Path>,
    source: Option<Source>,
    force: bool,
    on_overlap: OnOverlap,
    out: &mut Out,
) -> Result<()> {
    let target = output.expect("main.rs refuses --merge without -o");
    if target.exists() && !force {
        return Err(CoreError::TargetExists {
            path: target.to_path_buf(),
        });
    }

    let store = store.map(|s| s as &dyn StructureStore);
    let mut pieces = Vec::new();
    for name in structures {
        let entry = catalog::resolve(name, entries, source)?;
        let bytes = catalog::read_entry(&entry, store)?;
        let decoded = mcstructure::decode(&bytes, &entry.name)?;
        pieces.push((entry.name.clone(), decoded));
    }

    let options = MergeOptions {
        on_overlap,
        ..MergeOptions::default()
    };
    let report = merge::merge(&pieces, &options)?;

    for warning in &report.warnings {
        out.warn(warning.clone());
    }
    for overlap in &report.overlaps {
        out.warn(format!(
            "{} blocks overlapped between {:?} and {:?}",
            overlap.count, overlap.pieces[0], overlap.pieces[1]
        ));
    }

    let bytes = mcstructure::encode(&report.structure, &target.display().to_string())?;
    if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(target, &bytes)?;

    let s = &report.structure;
    out.line(format!(
        "wrote {} ({}) — {} x {} x {} from {} structures",
        target.display(),
        human_size(bytes.len() as u64),
        s.size.x,
        s.size.y,
        s.size.z,
        pieces.len()
    ));
    out.line("Reload the world before Construct sees it.");

    out.emit(MergedPayload {
        world: world.qualified(),
        merged: Merged {
            path: target.display().to_string(),
            bytes: bytes.len() as u64,
            sources: pieces.into_iter().map(|(n, _)| n).collect(),
            size: [s.size.x, s.size.y, s.size.z],
            origin: [s.origin.x, s.origin.y, s.origin.z],
            overlaps: report
                .overlaps
                .iter()
                .map(|o| OverlapRow {
                    count: o.count,
                    pieces: o.pieces.clone(),
                })
                .collect(),
        },
    });
    Ok(())
}
```

Add the imports this needs at the top of the file: `use construct_core::mcstructure;` and `use construct_core::merge::{self, MergeOptions, OnOverlap};`.

- [ ] **Step 6: Verify**

Run: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --all --check`
Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/construct-cli/src crates/construct-cli/tests/cli.rs
git commit -m "Add export --merge, writing one structure from several"
```

---

### Task 9: Documentation and carried-forward findings

**Files:**
- Modify: `docs/superpowers/specs/2026-08-31-constructcli-design.md`, `docs/carried-forward.md`, `docs/manual-verification.md`, `README.md`

- [ ] **Step 1: Correct §9's field layout**

In the spec's §9 opening paragraph, replace the sentence beginning "`Structure` models the format" with:

```markdown
`Structure` models the format: `format_version`, `size`, `structure_world_origin`, and a
`structure` compound holding two `block_indices` layers, `entities`, and a `palette`.
**`block_palette` and `block_position_data` sit under `palette.default`**, not directly under
`structure` — `palette` is a compound of named palettes, of which only `default` is ever
written or loaded. Encoding is little-endian NBT.

`block_position_data` is keyed by the block's **flattened index as a decimal string**, and
its values may carry `block_entity_data`, `tick_queue_data`, or both. The index order is
**ZYX**: `index = SZ*SY*X + SZ*Y + Z`.
```

- [ ] **Step 2: Correct §9's entity claim**

Replace merge step 5 ("Translate `block_position_data` keys and entity positions into the new
frame") with:

```markdown
5. Translate `block_position_data` keys into the new frame. **Entity positions are left
   alone**: `Pos` is an absolute world position, and placement computes
   `Pos - structure_world_origin + load_position`. Because the merged origin is the min corner
   of the union, that arithmetic already lands every entity correctly — rewriting `Pos` would
   shift them twice. `block_position_data` keys genuinely do need recomputing, because they
   index a grid whose dimensions changed.
```

- [ ] **Step 3: Replace "The encoder is not yet usable"**

That subsection is now history. Replace its final paragraph (beginning "Decoding is
unaffected") with:

```markdown
Decoding is unaffected — every file above parses correctly — so `list`, `export`, `import`,
and `copy` were never blocked. **Resolved in stage 3** by patching the fork, the first option
listed here: `nbtx` writes a sequence's element type and length lazily, on the first element,
so an empty one emits neither. The patch arms a flag when a sequence opens and writes
`TAG_End` with length 0 from `end()` if no element ever arrived. Pinned at
`bedrock-crustaceans/bedrockrs-nbt@bd28e77` in `third_party/patches/`.

Measured after the patch, against 13 real `.mcstructure` files from 289 B to 4.25 MB: every
one decodes and re-encodes to a payload of identical length that reparses and compares
semantically equal. **No `ByteArray`, `IntArray`, or `LongArray` tag occurs in any of them**,
so the array-to-list defect — which the patch does not address — is unreachable for this
format in practice. It remains reachable for `level.dat`, which is why the fidelity gate in
§10 stays.
```

- [ ] **Step 4: Update the risk register**

Replace the `nbtx` empty-list row's mitigation with:

```markdown
| `nbtx` cannot encode `.mcstructure` (empty lists) | High | **Measured (§9), then resolved in stage 3** by patching the fork. 13 real files round-trip semantically; upstream's own tests still pass |
```

- [ ] **Step 5: Record what stage 3 left behind**

Append to `docs/carried-forward.md`:

```markdown
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
apart costs 8 bytes per block of mostly-void space. `MergeOptions::max_volume` caps this at 64
million blocks (~512 MB) and refuses beyond it. A sparse representation would lift the cap, but
nothing observed needs it: the largest real structure measured is 81×81×81.

## `--merge` does not check that the pieces came from one build

Nothing verifies the structures are pieces of the same build rather than unrelated saves that
happen to have distinct origins. The identical-origin refusal catches the common mistake; a
merge of genuinely unrelated structures produces a mostly-void result the user did not want,
with no warning beyond the size one.
```

- [ ] **Step 6: Add the manual verification item**

Append to `docs/manual-verification.md`:

```markdown
- [ ] **Does a merged structure load and place correctly in-game?** Save two pieces of one
      build with structure blocks, `construct export <world> <a> <b> --merge -o merged.mcstructure`,
      import it, and place it. Check that the pieces land in their original relative positions,
      that the gaps between them leave existing terrain untouched rather than carving air, and
      that block entities (a labelled chest in each piece) kept their contents. Spec §12 asserts
      all three semantically, but only the game proves the file is one it accepts.
- [ ] **Does a merged structure beyond structure-block dimensions load?** The reference
      documentation says sizes past 64×256×64 load "just as expected"; merge only warns. Confirm
      with a merge whose union exceeds that.
```

- [ ] **Step 7: Document the command**

In `README.md`, alongside the other `export` examples:

````markdown
Merge several saves of one build back into a single structure, reassembled at the
positions they were saved at:

```console
$ construct export "My World" north_wing tower --merge -o castle.mcstructure
warning: 1,204 blocks overlapped between "north_wing" and "tower"
wrote castle.mcstructure (2.1 MB) — 48 x 31 x 52 from 2 structures
```

Gaps between the pieces are structure void, so placing the result leaves the terrain
between them untouched. Where two pieces both have a block, the one named later wins;
`--on-overlap first` reverses that and `--on-overlap error` refuses instead.
````

- [ ] **Step 8: Verify and commit**

```bash
cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --all --check
git add docs README.md
git commit -m "Record what stage 3 measured, corrected, and left behind"
```

---

## Self-Review

**1. Spec coverage.** §9's model (Task 3), merge steps 1–6 (Tasks 5–7), void gaps (Task 6), overlap and `--on-overlap` (Tasks 6–7), `block_position_data` following the winner (Task 7), identical-origin refusal and the mixed-origin warning (Task 6), the allocation guard and the oversize performance warning (Task 6), the encoder blocker (Task 1). §5's `export --merge -o FILE` and its usage rules (Task 8). §11's exit codes: usage `2` for `--merge` without `-o`, `1` for a refused merge or an existing target. §12's five fixture shapes (Task 3/4: single block, block entities, waterlogged second layer, entities, void gaps), the semantic comparison rule, and the overlap tests including the chest-contents case.

Two §12 items are deliberately *not* in this plan: byte transparency (`export`→`import`→`export`) is stage 1/2 behaviour already covered, and the database tests belong to stage 4.

**2. Placeholders.** None. Every code step carries the code; the one shell step that depends on the environment (Task 3 Step 7, fetching the real fixture) gives both the command and the fallback.

**3. Type consistency.** `Size`/`Coord`/`BoundingBox` (Task 2) are used unchanged in Tasks 3–8. `Structure`'s fields as declared in Task 3 are the ones Tasks 4, 6, and 7 read and write. `unify_palettes` returns `(Vec<BlockState>, Vec<Vec<i32>>)` in Task 5 and is destructured that way in Task 6. `MergeOptions`/`OnOverlap`/`MergeReport`/`Overlap` are declared in Task 6 and consumed in Tasks 7–8. `decode`/`encode` take `(bytes, what)` and `(&Structure, what)` consistently.

One thing an executor should watch: Task 6 writes `MergeReport` with empty `block_position_data` and `entities`, and Task 7 fills them in. Task 6's tests must not assert those are empty, or Task 7 will have to delete a test rather than extend one.
