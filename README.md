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

No binaries are published yet; building from source is currently the only
way to get the tool.

You need Rust, CMake, and a C++ compiler, because the leveldb
backend is Mojang's own C++ implementation rather than a reimplementation.
The upstream `bedrock-rs` and `leveldb-sys` crates do not compile as published
on macOS or on any non-x86_64 target, so this repo carries fixes in
`third_party/patches/` and applies them to pinned checkouts of both upstreams.
Run the setup script once after cloning, before building:

```
git clone https://github.com/ForestOfLight/ConstructCLI
cd ConstructCLI
./scripts/setup-deps.sh
cargo build --release
```

The fixes are intended to go upstream; once they land, this step goes away.
See `third_party/README.md` for what each patch fixes.

## Usage

```
construct worlds                          # every world this machine can see
construct list <world>                    # structures in a world
construct export <world> <structure>      # write <structure>.mcstructure
construct export <world> <s1> <s2>        # one file each
construct export <world> <s> -o out.mcstructure
```

A `<world>` is a world's display name, a folder name, a qualified
`<installation>/<account>/<world>` reference, or a path to a world directory.
When a name is ambiguous, the error prints the qualified forms to pick from.

Global flags, available on every command:

- `--json` — print one JSON document instead of human-readable output.
- `--force` — overwrite an existing target file; never relaxes the
  world-in-use refusal.
- `--com-mojang <PATH>` — probe an extra `com.mojang` root, in addition to the
  ones discovered automatically. Repeatable.
- `--source <world|pack>` — disambiguate a structure name that exists in both
  a world and a behavior pack.

`--json` prints exactly one JSON document on stdout carrying `"schema": 1`;
warnings are printed both to stderr and in a `warnings` array in that
document. This is the contract the eventual GUI is meant to build on.

### Exit codes

| Code | Meaning |
| ---- | ------- |
| 0 | success |
| 1 | failure |
| 2 | usage error |
| 3 | not found |
| 4 | world in use |

## Safety

Opening a leveldb database runs recovery and can rewrite it — this was
measured, not assumed. So ConstructCLI never opens a world's live database:
every read copies `db/` to a temporary directory first and reads the copy,
telling you when it does (`reading from a 2.2 MB snapshot`). This is why a
world currently open in Minecraft can be read safely. Nothing in stage 1
writes to a world.

## License

MIT, matching Construct. The test fixture world is derived from
[bedrock-rs](https://github.com/bedrock-crustaceans/bedrock-rs) under Apache-2.0;
see `crates/construct-core/tests/fixtures/NOTICE`.
