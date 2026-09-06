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
`.mcstructure` into its `structures` folder, and restart the world. This
replaces that with:

```
construct install --world "My Survival"
construct import house.mcstructure --world "My Survival"
```

If you already moved `Construct[BP]` into `behavior_packs` rather than
`development_behavior_packs` — they sit next to each other and the game reads
both — `install` moves it across for you and keeps every structure in it.

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

```
construct worlds                          # every world this machine can see
construct structures --world <world>      # structures in a world
construct export <structure>              # write <structure>.mcstructure out of the shared Construct
construct export <s1> <s2>                # one file each
construct export <structure> --world <world>           # out of that world's database or its own Construct copy
construct export <s> -o out.mcstructure                # -o always writes a .mcstructure; a bare name gets the extension
construct install                         # download and install the latest Construct
construct install --version 1.2.0         # a specific release
construct install --world <world>         # also enable it in a world and turn Beta APIs on
construct import house.mcstructure                    # copy into Construct's structures/
construct import house.mcstructure barn.mcstructure   # several at once
construct import house.mcstructure --world <world>    # into that world's own Construct copy
construct import house.mcstructure --name my_house    # override the derived name (one file only)
construct copy <src-world> <dst-world> house          # read from one world, write into another's Construct
construct copy <src-world> <dst-world> house barn     # several at once
construct delete house                                # from the shared Construct (every world using it)
construct delete house barn                          # several at once
construct delete house --world <world>               # that world only: its database and its own pack
construct delete house --world <world> --source world  # ...just the database
construct delete house --world <world> --source pack   # ...just that world's pack file
construct structures                      # just the shared copy: what every world using it gets
construct status                          # installed version, latest available, where it's enabled, and where structures live
construct enable-beta-apis <world>        # turn a world's Beta APIs experiment on
```

`import` and `copy` only ever write within a `BP/structures/` folder, never into a
world's database. `delete` is the exception and the only leveldb write this tool
makes: it refuses outright if Minecraft looks to have the world open, since a
second writer is how a save gets damaged, and there is no `--force` for that.

**Structures belong to a world.** Every world Construct is installed in gets
one structures home: its own copy of Construct when it has one, otherwise a
small `ConstructStructures` pack — a manifest, an icon, and a `structures/`
folder — created inside the world by `construct install --world`, or by the
first `import` or `copy` that needs it. That is where per-world writes go, so
a structure imported for one world does not turn up in every other.

`structures` reports everything a world actually sees, which is wider than where it
writes: its database, its own pack, and the shared Construct's `structures/`
when that is the copy it runs. Each row says which, because that is the
difference between a structure being yours and being everyone's:

```console
$ construct structures --world "My Survival"
NAME                     SOURCE          SIZE
house                    world         12.0 KB
imported_tower           pack:world    31.0 KB
shared_prefab            pack:shared    8.0 KB
```

Structures already in the shared copy stay there and keep working; nothing is
moved for you. Importing a name a world already sees from the shared copy
warns, because the game will load both and log a conflict.

`delete` is `import` read backwards, and its grammar says so. `import <file>`
places into the shared copy of Construct and `import <file> --world W` places
into that world's own; so `delete <name>` removes from the shared copy and
`delete <name> --world W` removes from that world.

**`--world` never touches the shared copy.** A structure there belongs to every
world using it, so a command scoped to one world will not remove it as a side
effect — it reports the name as missing from that world instead. To remove the
shared one, say so by leaving `--world` off.

Within a world a name can still be in two places at once — the database and
that world's own pack — and `delete --world` removes both, because "remove this
name from this world" leaves nothing to guess at. Every other command refuses a
name living in two places and makes you say which you meant; `delete` does not
need to. `--source world|pack` narrows it when you want one gone and the other
kept.

There is no undo: run `export` first if you might want the structure back.

A name — from `--name`, or derived from the file stem when it's omitted —
may use only `A-Za-z0-9_.-`; a `/` is refused. Capitals are kept, not folded:
the game's own names carry them (`10HzCounter`, `CanopyPlayers:players`), so a
derived name differs from the file stem only where a space became `_`. `structures` reports structures
Construct or a hand-edited pack nested at any depth, but nothing this tool
*writes* creates a nested path: `/` is exactly the character that makes path
traversal possible, the same class of bug `export`'s derived filenames
already had once, caught before stage 1 shipped.

A `<world>` is a world's display name, a folder name, a qualified
`<installation>/<account>/<world>` reference, or a path to a world directory.
When a name is ambiguous, the error prints the qualified forms to pick from.

### Tab Autocompletion

ConstructCLI supports dynamic tab autocompletion for subcommands, options, world names, and structure names.

To enable completions in your shell:

- **Bash** (`~/.bashrc`):
  ```bash
  source <(construct completions bash)
  ```
- **Zsh** (`~/.zshrc`):
  ```zsh
  source <(construct completions zsh)
  ```
- **Fish** (`~/.config/fish/config.fish`):
  ```fish
  construct completions fish | source
  ```
- **PowerShell** (`$PROFILE`):
  ```powershell
  construct completions powershell | Out-String | Invoke-Expression
  ```
- **Elvish** (`~/.elvish/rc.elv`):
  ```elvish
  eval (construct completions elvish | slurp)
  ```

`--json` — print one JSON document instead of human-readable output — is the
one flag every command takes, before or after the subcommand.

The rest are declared only on the commands that read them, so a command refuses
a flag it would otherwise have ignored, and they go **after** the subcommand:

- `--com-mojang <PATH>` — probe an extra `com.mojang` root, in addition to the
  ones discovered automatically. Repeatable. Taken by every command that
  searches for Minecraft, which is all of them but `add` and `completions`.
- `--force` — overwrite what is already there: `export`'s output file,
  `import`'s and `copy`'s structure of the same name in the target pack, and
  the version `install` would otherwise leave alone. It never relaxes the
  world-in-use refusal, and `structures`, `worlds`, `delete`, `status` and
  `enable-beta-apis` do not take it at all.

`structures`, `export`, `copy`, and `delete` also take `--source <world|pack>` to
disambiguate a structure name that exists in both a world and a behavior pack.
`copy` takes `--pack <world|shared>` to pick between the two packs its source
world can see. Neither applies to `import`, which writes files and never reads a
name back out of a world.

`export` and `delete` do not take `--pack`, because `--world` already says which
copy is meant: without it they read and write the shared Construct, and with it
they are confined to that world and cannot reach the shared copy at all.
`structures` does not take it either: `--world` asks what that world sees, which
is every pack serving it, and the SOURCE column says which pack each row is in.

On `copy`, both flags describe the **source** world — they pick which copy to
read. Nothing selects the destination: a copy always lands in the destination
world's own structures home.

`--json` prints exactly one JSON document on stdout carrying `"schema": 1`;
warnings are printed both to stderr and in a `warnings` array in that
document. This is the contract the eventual GUI is meant to build on.

Three environment variables:

- `CONSTRUCT_INSTALLATION` — which installation `install` and `import`
  target when they aren't pinned to a `--world`, and which installation
  `status` reports on (it has no `--world` flag of its own to pin with).
  Same role as `config.toml`'s `default_installation`, and checked first.
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
