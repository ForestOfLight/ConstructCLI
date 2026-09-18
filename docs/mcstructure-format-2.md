# `.mcstructure` format 2

`docs/bedrock-mcstructure-files.md` describes the format as it stood when `format_version` was
"currently always set to `1`". A 2026 game update began exporting `format_version` `2`. This
file records what actually differs, measured from files the game wrote — it is *our* note, not
tryashtar's, which is why it is not in that document.

## What was measured

Two structure blocks exported on 2026-09-17 by the game version then installed, kept as
fixtures under `crates/construct-core/tests/fixtures/`:

| File | Contents |
| --- | --- |
| `format2-fences.mcstructure` | 3×1×3 of spruce fences, seven palette entries, no entities |
| `format2-entity.mcstructure` | 5×5×5 with one entity and one `block_entity_data` |

The blocks in both carry `version` `18168865` — `01 15 3C 21`, meaning 1.21.60.33 — against
`18163713` (1.21.40.1) in the format 1 fixture `construct.mcstructure`.

## The differences

Only `structure.block_indices` changed. Everything else — `size`, `structure_world_origin`,
`palette.default.block_palette`, `palette.default.block_position_data`, `entities`, ZYX index
order, `-1` meaning "leave the existing block" — is byte-for-byte the same idea as format 1.

1. **Each layer is a `TAG_Int_Array`**, not a `TAG_List` of `TAG_Int`. Same numbers, four bytes
   each instead of a tagged list.
2. **A layer that is entirely void is not written at all.** Both files carry a single layer,
   because neither has a waterlogged block. Format 1 always wrote two, the second nearly always
   all `-1`.

Two layers under format 2 were not observed, because neither exported build contains a
waterlogged block. The game still has waterlogging, so a second layer is assumed to appear when
one is needed; nothing here depends on that being true, since one layer and two are both
accepted.

## What ConstructCLI does with it

**Reading** (`mcstructure::decode`): both layouts decode into the same `Structure`. A layer is
read whether it is a list or an int array, and a missing second layer becomes all-void so that
merge and encode always see the two layers the model promises. One layer or two are accepted;
zero, or more than two, are still refused. `format_version` is kept as the file declared it.

**Writing** (`mcstructure::encode`): output is always the format 1 layout — `format_version` 1,
two `block_indices` layers, each a list of ints. Format 2 carries no information format 1
cannot, and format 1 is what every version of the game loads, which is what a merged structure
shipped inside a behaviour pack needs. So merging format 2 pieces is lossless, but the file that
comes back out is a format 1 file.

Writing format 2 back would need `nbtx` patched as well: it widens a `TAG_Int_Array` into a list
on the way in and has no way to write one on the way out (see `third_party/README.md`).

## Merging

Merge never sees the difference. Pieces in either layout, in any combination, merge together —
`crates/construct-core/tests/merge.rs` covers a format 2 piece against a format 1 piece and two
format 2 pieces against each other.
