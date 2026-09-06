# ConstructCLI

Move structures between Minecraft Bedrock worlds from the command line.

This tool is designed to compliment the host of solutions for moving structures between Minecraft Bedrock worlds. It does not assume sole ownership over your structures. Instead, it expects to find them in a variety of places and handle changes gracefully.

### Upgrading the Old Construct Workfow

[Construct](https://github.com/ForestOfLight/Construct)'s documented workflow
for getting a structure out of a world is to upload the whole world to
holoprint-mc.github.io and use its "Extract From World" feature. This replaces
that with:

```
construct export house --world "My Survival"
```

Construct's documented workflow for getting a structure back *in* is manual
too: move `Construct[BP]` into `development_behavior_packs` by hand, drop the
`.mcstructure` into its `structures` folder. This
replaces that with:

```
construct import house.mcstructure --world "My Survival"
```

## Install

No binaries are published yet; building from source is currently the only
way to get the tool.

You need Rust, CMake, and a C++ compiler, because the leveldb
backend is Mojang's own C++ implementation rather than a reimplementation.
The upstream `bedrock-rs` and `leveldb-sys` crates do not compile as published
on macOS or on any non-x86_64 target, and `nbtx` cannot serialize an empty
list — most `.mcstructure` files have at least one — so this repo carries
fixes in `third_party/patches/` and applies them to pinned checkouts of all
three upstreams. Run the setup script once after cloning, before building:

```
git clone https://github.com/ForestOfLight/ConstructCLI
cd ConstructCLI
./scripts/setup-deps.sh
cargo build --release
```

The fixes are intended to go upstream; once they land, this step goes away.
See `third_party/README.md` for what each patch fixes.

## Usage

All usage is displayed in the help.

```
construct help
```

If you're interested in how ConstructCLI can be integrated in an application, please read the full integration guide here: [INTEGRATION.md](INTEGRATION.md)

### Tab Autocompletion

ConstructCLI supports dynamic tab autocompletion for subcommands, options, world names, and structure names. For example, to enable completions in bash:

```bash
source <(construct completions bash)
```

Completions are also available for zsh, fish, powershell, and elvish.

### Exit codes

| Code | Meaning |
| ---- | ------- |
| 0 | success |
| 1 | failure |
| 2 | usage error |
| 3 | not found |
| 4 | world in use |
| 5 | partial success - `install` placed the packs but could not flip Beta APIs on |

### Building with ConstructCLI

The `--json` flag can be added to any command to output formatted in JSON instead of human-readable output.

## Contributing

Contributions of all kinds are welcome, including bug fixes, new features, docs updates, and improvements.
Please read the full contribution guide here: [CONTRIBUTING.md](CONTRIBUTING.md)

For guidance or collaboration, connect with the community on [Discord](https://discord.gg/9KGche8fxm).
