# CLI surface

The whole command grammar in one place, for reworking it. Not user docs —
`--help` is that. Everything below is what the code actually does today, with
anchors for editing and a list of the seams where the grammar is inconsistent.

## The grammar today

`v` = takes a value · `[v]` = optional value · `…` = repeatable · **bold** = required

| Command | Positionals | Own options |
| ------- | ----------- | ----------- |
| `worlds` | — | — |
| `list` | `[world]` | — |
| `export` | **`world`** **`structures…`** | `-o/--output v`, `--merge`, `--on-overlap v` (`last`\|`first`\|`error`, default `last`) |
| `import` | **`file`** | `--world v`, `--name v` |
| `copy` | **`src_world`** **`structure`** **`dst_world`** | — |
| `delete` | **`world`** **`structure`** | — |
| `experiment` | **`world`** | **`--beta-apis [v]`** (`on`\|`off`) |
| `install` | — | `--version v`, `--world v` |
| `status` | — | — |

Globals, declared once on `Cli` (`cli.rs:10`) and therefore *accepted by every
command* — but only consumed by some:

| Global | Consumed by | Ignored by |
| ------ | ----------- | ---------- |
| `--com-mojang v…` | all (discovery, `main.rs:42`) | — |
| `--json` | all | — |
| `--force` | `export`, `import`, `copy`, `install` | `worlds`, `list`, `delete`, `experiment`, `status` |
| `--source v` (`world`\|`pack`) | `list`, `export`, `copy`, `delete` | `worlds`, `import`, `install`, `status` |
| `--pack v` (`world`\|`shared`) | `list`, `export`, `copy`, `delete` | same as above |

## Semantics you can't read off the grammar

- **`--source`/`--pack` are filters, not just tie-breakers.** Naming either one
  drops non-matching rows, so `--pack shared` also hides world-database rows
  (`list.rs:38`, `catalog.rs:119`).
- **`delete` pins `--source pack` internally** whatever you pass, so a world db
  is never opened; `--source world` is refused up front as `NotImplemented`
  (`delete.rs:47`).
- **Shared vs per-world target is expressed by absence.** `list` with no
  positional = shared pack only; `import` with no `--world` = shared pack. There
  is no explicit token for "shared".
- **Installation is never a flag.** `list` (no world), `import` (no `--world`),
  `install` (no `--world`), and `status` resolve it from
  `CONSTRUCT_INSTALLATION` → `default_installation` → sole candidate
  (`main.rs:130,203,260,278`). `--com-mojang` roots get synthetic names
  `flag1`, `flag2`, … (`main.rs:44`).
- **`install --world` does three separate things**: enable both packs in the
  world, ensure a structures home exists, flip Beta APIs on (`install.rs:123`).
  Partial failure exits 5 from inside the command (`install.rs:272`), bypassing
  `exit_code`.
- **`--force` means two different things**: refuse-overwrite escape for a target
  file (`export`, `import`, `copy`) vs re-place a pack of the version already
  installed (`install/mod.rs:66`).

## Validation hand-rolled in `main.rs`, not expressed in clap

Each of these prints its own `error:` + usage line and `exit(2)`. All are
candidates for clap-native expression (`conflicts_with`, `requires`,
`ArgGroup`, a value parser) in any rework:

| Rule | Where |
| ---- | ----- |
| `--merge` requires `-o` | `main.rs:147` |
| `-o` with >1 structure and no `--merge` | `main.rs:177` |
| `-o` must end `.mcstructure` (missing ext is filled in, wrong ext refused) | `main.rs:160`, `mcstructure_path` at `main.rs:297` |
| `--source world` with no world positional on `list` | `main.rs:124` |
| `--beta-apis` required, `num_args = 0..=1` (get/set in one flag) | `cli.rs:133` |

## Seams worth reworking

1. **World is a positional in `list`/`export`/`copy`/`delete`/`experiment` but a
   flag in `import`/`install`.** The split is real (there it's optional and
   selects shared-vs-world), but it means "the world" is spelled two ways.
2. **`world` is overloaded across the two selectors**: `--source world` means
   the world's *database*, `--pack world` means the world's *pack*. Same token,
   two referents, and they can be combined.
3. **`install --version` shadows the conventional `-V/--version`.** clap keeps
   them distinct because the global is `-V` on the root, but `construct install
   --version` reads as "print version".
4. **Arity is inconsistent**: `export` takes N structures, `copy`/`delete` take
   exactly one, `import` takes one file. No plural form anywhere else.
5. **Globals are accepted where they do nothing** — `construct worlds --pack
   shared` parses and is silently ignored. Consider per-command args or
   `global = false`.
6. **`experiment` is named for a category but hard-codes one toggle** as a
   required flag. A second toggle needs either another required-ish flag or a
   different shape (`experiment <world> [toggle] [on|off]`).
7. **Get/set in one flag** (`--beta-apis` with optional value) is the only
   read-write flag in the CLI; everywhere else reading is its own command.
8. **`-o` is the only short flag.** Either commit to shorts or drop it.
9. **Exit 5 escapes the error taxonomy** — emitted directly so the success JSON
   already printed isn't clobbered. Any restructure of partial-success reporting
   has to keep that ordering property.

## Contracts a rename would break

- **JSON payloads** — one struct per command, `schema: 1` + `warnings` injected
  at emit (`output.rs:56`). Keys today: `worlds.rs:7,12` · `list.rs:11,18` ·
  `export.rs:21,27,201,207` · `import.rs:16` · `copy.rs:19` · `delete.rs:19` ·
  `experiment.rs:15` · `install.rs:17` · `status.rs:25,36`. `list`, `export`,
  `copy` identify worlds by *qualified reference*; human lines use display name.
- **Exit codes** (`main.rs:483`): 0 ok · 1 fail · 2 usage/ambiguous/malformed/
  not-implemented · 3 not-found · 4 world in use · 5 partial install.
- **Error hints in `report`** (`main.rs:321`) hard-code command syntax in prose:
  `construct worlds --com-mojang <path>`, `construct list <world> --source
  world`, `construct <command> ... --pack world`, `construct import <file>
  --name <name>`, `construct experiment <world> --beta-apis on`,
  `construct install --world <world>`. Grep `construct ` in `main.rs` and
  `install.rs` after any rename.
- **Tests** (`crates/construct-cli/tests/cli.rs`, 3.7k lines) pin flag spellings:
  `--com-mojang` ×79, `--json` ×28, `--world` ×22, `--source` ×21,
  `--beta-apis` ×8, `--merge` ×7, `--name` ×5, `--force` ×5, `--pack` ×3.
- **README usage block** (`README.md`, "## Usage") lists ~20 example
  invocations verbatim.

## Reference grammars (unchanged by any flag rework)

- **World**: display name · folder name · `<installation>/<account>/<world>`
  with segments optional from the left · filesystem path (checked first).
  Reserved installation names: `release`, `preview`, `legacy`, `mcpelauncher`,
  plus `path` and `env` (`config.rs:49`).
- **Structure name**: `A-Za-z0-9_.-`, no `/`, capitals kept. Bare name →
  `mystructure:<name>` → `structures/<name>.mcstructure`; explicit namespace →
  `structures/<ns>/<name>.mcstructure` and invisible to Construct's in-game list.
- **Env**: `CONSTRUCT_CONFIG`, `CONSTRUCT_INSTALLATION`, `CONSTRUCT_COM_MOJANG`
  (root named `env`), `CONSTRUCT_GITHUB_TOKEN`/`GITHUB_TOKEN`,
  `CONSTRUCT_GITHUB_API`.
- **Config**: `default_installation`, `[[roots]] name/path`,
  `[backups] dir/keep` (default 10). Precedence: flag → env → file → discovery.
