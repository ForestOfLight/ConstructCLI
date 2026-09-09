# ConstructCLI

Move structures between Minecraft Bedrock worlds from the command line.

This tool is designed to compliment the host of solutions for moving structures between Minecraft Bedrock worlds. It does not assume sole ownership over your structures. Instead, it expects to find them in a variety of places and handle changes gracefully.

> [!NOTE]
> Development of this repo has involved significant AI usage.

### Upgrading the Old Construct Workfow

[Construct](https://github.com/ForestOfLight/Construct)'s documented workflow
for getting a structure out of a world is to upload the whole world to
holoprint-mc.github.io and use its "Extract From World" feature. This replaces
that with:

```bash
construct export house --world "My Survival"
```

Construct's documented workflow for getting a structure back *in* is manual
too: move `Construct[BP]` into `development_behavior_packs` by hand, drop the
`.mcstructure` into its `structures` folder. This
replaces that with:

```bash
construct import house.mcstructure --world "My Survival"
```

## Install

Download the archive for your platform from the [latest
release](https://github.com/ForestOfLight/ConstructCLI/releases/latest),
extract it, and put the `construct` binary somewhere on your `PATH`.

On **macOS** and **Linux**:

```bash
tar xzf construct-<version>-<platform>.tar.gz
sudo install construct-<version>-<platform>/construct /usr/local/bin/
construct --version
```

On **Windows**, extract the zip and move `construct.exe` into a directory that is
already on your `PATH`. If you don't have one, create
`%LOCALAPPDATA%\Programs\bin`, put `construct.exe` there, and add that folder
to your `PATH` from Settings > "Edit environment variables for your account".

Intel Macs, 32-bit systems, and Linux distributions older than Debian 12 or
Ubuntu 22.04 have no published binary. To run construct on these versions,
you can build from source instead.

### Building from source

You need Rust, CMake, and a C++ compiler.

Run the setup script once after cloning, before building:

```bash
git clone https://github.com/ForestOfLight/ConstructCLI
cd ConstructCLI
./scripts/setup-deps.sh
```

The `setup-deps.sh` script applies a set of fixed to the upstream packages.
See `third_party/README.md` for what each patch fixes.

To build:

```bash
cargo build --release
```

## Usage

All usage is displayed in the help.

```bash
construct help
```

### Tab Autocompletion

ConstructCLI supports dynamic tab autocompletion for subcommands, options, world names, and structure names. For example, to enable completions in bash:

```bash
source <(construct completions bash)
```

Completions are also available for zsh, fish, powershell, and elvish.

### Building with ConstructCLI

If you're interested in how ConstructCLI can be integrated in an application, please read the full integration guide here: [INTEGRATION.md](INTEGRATION.md)

## Contributing

Contributions of all kinds are welcome, including bug fixes, new features, docs updates, and improvements.
Please read the full contribution guide here: [CONTRIBUTING.md](CONTRIBUTING.md)

For guidance or collaboration, connect with the community on [Discord](https://discord.gg/9KGche8fxm).
