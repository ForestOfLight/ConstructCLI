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

Download the archive for your platform from the [latest
release](https://github.com/ForestOfLight/ConstructCLI/releases/latest),
extract it, and put the `construct` binary somewhere on your `PATH`.

| Platform | Architecture | Archive |
| --- | --- | --- |
| Windows | x86_64 | `construct-<version>-x86_64-pc-windows-msvc.zip` |
| macOS | Apple Silicon | `construct-<version>-aarch64-apple-darwin.tar.gz` |
| Linux | x86_64, glibc 2.35+ | `construct-<version>-x86_64-unknown-linux-gnu.tar.gz` |

On macOS and Linux:

```
tar xzf construct-<version>-<target>.tar.gz
sudo install construct-<version>-<target>/construct /usr/local/bin/
construct --version
```

macOS marks anything downloaded through a browser as quarantined. Extracting
with `tar` in Terminal, as above, avoids that. If macOS still refuses to open
the binary, clear the flag:

```
sudo xattr -d com.apple.quarantine /usr/local/bin/construct
```

On Windows, extract the zip and move `construct.exe` into a directory that is
already on your `PATH`. If you don't have one, create
`%LOCALAPPDATA%\Programs\bin`, put `construct.exe` there, and add that folder
to your `PATH` from Settings → "Edit environment variables for your account".

Intel Macs, 32-bit systems, and Linux distributions older than Debian 12 or
Ubuntu 22.04 have no published binary — build from source instead.

### Verifying a download

Every release includes a `SHA256SUMS` file. Download it alongside the archive
and check them against each other:

On Linux:

```
sha256sum --check --ignore-missing SHA256SUMS
```

On macOS, which ships `shasum` rather than `sha256sum`:

```
shasum -a 256 --check --ignore-missing SHA256SUMS
```

On Windows PowerShell:

```
(Get-FileHash construct-<version>-x86_64-pc-windows-msvc.zip).Hash.ToLower()
```

Compare the result against the matching line in `SHA256SUMS`.

### Building from source

Contributors, and anyone on a platform with no published binary, build it
themselves.

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
