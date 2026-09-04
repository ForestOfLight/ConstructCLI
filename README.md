# ConstructCLI

Move structures between Minecraft Bedrock worlds from the command line.

[Construct](https://github.com/ForestOfLight/Construct)'s documented workflow
for getting a structure out of a world is to upload the whole world to
holoprint-mc.github.io and use its "Extract From World" feature. This replaces
that with:

```
construct export "My Survival" house
```

Construct's documented workflow for getting a structure back *in* is manual
too: move `Construct[BP]` into `development_behavior_packs` by hand, drop the
`.mcstructure` into its `structures` folder, and restart the world. This
replaces that with:

```
construct install --world "My Survival"
construct import house.mcstructure --world "My Survival"
```

## Status

Stage 2 of 4. `worlds`, `list`, and `export` now see a world's database and
Construct's own `structures/` folder as one catalog. `install`, `import`,
`copy`, `delete --source pack`, `status`, and `experiment` are also built.
Merging structures and deleting one from a world's database are not — see
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
construct install                         # download and install the latest Construct
construct install --version 1.2.0         # a specific release
construct install --world <world>         # also enable it in a world and turn Beta APIs on
construct import house.mcstructure                    # copy into Construct's structures/
construct import house.mcstructure --world <world>    # into that world's own Construct copy
construct import house.mcstructure --name my_house    # override the derived name
construct copy <src-world> house <dst-world>          # read from one world, write into another's Construct
construct delete <world> house --source pack          # remove an imported structure
construct status                          # installed version, latest available, and where it's enabled
construct experiment <world> --beta-apis         # show the current toggle
construct experiment <world> --beta-apis on      # turn it on
```

`import`, `copy`, and `delete` all write into Construct's `structures/`
folder, never into a world's database — reload the world before Construct
shows the change. `delete` currently only removes an imported structure
(`--source pack`); removing one from a world's *database* is stage 4, the
only leveldb write this tool will ever make, and `--source world` refuses
with a usage error until then.

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

Three environment variables:

- `CONSTRUCT_INSTALLATION` — which installation `install`, `import`, and
  `status` target when they aren't pinned to a `--world`. Same role as
  `config.toml`'s `default_installation`, and checked first.
- `CONSTRUCT_GITHUB_TOKEN` (or `GITHUB_TOKEN`, which CI environments already
  set) — raises GitHub's rate limit above the unauthenticated default. Read
  by `install` and `status`.
- `CONSTRUCT_GITHUB_API` — overrides the GitHub API base URL. This is what
  lets the test suite exercise `install`, download included, without ever
  touching the network; it doubles as an escape hatch for an enterprise
  proxy that mirrors the GitHub API under a different address.

### Exit codes

| Code | Meaning |
| ---- | ------- |
| 0 | success |
| 1 | failure |
| 2 | usage error |
| 3 | not found |
| 4 | world in use |
| 5 | partial success — `install` placed the packs but could not flip Beta APIs on |

## Safety

Opening a leveldb database runs recovery and can rewrite it — this was
measured, not assumed. So ConstructCLI never opens a world's live database:
every read copies `db/` to a temporary directory first and reads the copy,
telling you when it does (`reading from a 2.2 MB snapshot`). This is why a
world currently open in Minecraft can be read safely. No command in stage 2
opens or writes a world's database either — `install` and `experiment` write
`level.dat`, and `import`, `copy`, and `delete` write only files under
Construct's own `structures/`. Every `level.dat` write is backed up first and
runs through a fidelity gate — re-serializing the file *unmodified* and
refusing to write if that doesn't round-trip byte-for-byte. The one write
this tool does not yet make is into a world's structure database itself;
that's stage 4.

## License

MIT, matching Construct. The test fixture world is derived from
[bedrock-rs](https://github.com/bedrock-crustaceans/bedrock-rs) under Apache-2.0;
see `crates/construct-core/tests/fixtures/NOTICE`.
