# CLI surface

The whole command grammar in one place, for reworking it. Not user docs —
`--help` is that. Everything below is what the code actually does today, with
anchors for editing and a list of the seams where the grammar is inconsistent.

## The grammar today

`v` = takes a value · `[v]` = optional value · `…` = repeatable · **bold** = required

| Command | Positionals | Own options |
| ------- | ----------- | ----------- |
| `worlds` | — | — |
| `structures` | — | `-w/--world v`, `--source v` (`world`\|`pack`) (no `--pack` — see below) |
| `export` | **`structures…`** | `-o/--output v`, `--merge`, `--on-overlap v` (`last`\|`first`\|`error`, default `last`), `-w/--world v`, `--source v` (no `--pack` — see below), `--force` |
| `import` | **`files…`** | `-w/--world v`, `--name v`, `--force` |
| `copy` | **`src_world`** **`dst_world`** **`structures…`** | `--source v`, `--pack v`, `--force` |
| `delete` | **`structures…`** | `-w/--world v`, `--source v` (no `--pack` — see below) |
| `enable-beta-apis` | **`world`** | — |
| `install` | — | `--version v`, `-w/--world v`, `--force` |
| `status` | — | — |
| `completions` | — | **`shell`** (`bash`\|`elvish`\|`fish`\|`powershell`\|`zsh`) |

`--json` is the only global left on `Cli` (`cli.rs:13`), so it is the only flag
that may precede the subcommand. Every other option is declared on the commands
that read it, and a command refuses one it would have ignored (exit 2, with
clap's own `tip: '<cmd> --flag' exists`).

`--com-mojang v…` is the one that is nearly universal without being universal.
It rides on a `Discovery` struct flattened into every command that searches for
Minecraft (`cli.rs:26`), which is all of them except `add` — it writes the path
it was handed into the config file — and `completions`, which prints a script.
`main` reads the whole set back through `Command::com_mojang()`, an exhaustive
match, so a new command has to answer the question rather than inherit an
answer.

`--force` is declared on `export`, `import`, `copy`, and `install`, each with
its own help text for what it overwrites (an output file · a structure of that
name in the target pack · the version already installed). It is absent from
`worlds`, `structures`, `delete`, `enable-beta-apis`, and `status`, and it
deliberately never overrode `delete`'s in-use refusal — that refusal is now
unreachable by flag rather than merely unmoved by one.

`--source` and `--pack` were the first two to move, for the same reason:
`construct worlds --pack shared` and `construct import f.mcstructure --source
pack` are refused by clap instead of parsing and doing nothing.

## Semantics you can't read off the grammar

- **`--source`/`--pack` are filters, not just tie-breakers.** Naming either one
  drops non-matching rows, so `--pack shared` also hides world-database rows
  (`catalog.rs:119`).
- **On `copy` they select the *source* world, never the destination.** Both are
  passed only to `loader::for_world(src, ..)` and `catalog::resolve` in
  `copy.rs:58,67`; the destination comes from `home_for_write(dst, ..)`, which
  takes neither. There is no flag that steers where a copy lands — it is always
  the destination world's own structures home. The help text on both args says
  so, since the grammar alone can't.
- **`delete`'s and `export`'s scope is their grammar, not a filter.** `delete
  <name>` targets the shared copy of Construct; `delete <name> --world W`
  targets that world's database and its own pack and **cannot reach the shared
  copy at all** — a name that exists only there reports as missing from the
  world. `export` reads along exactly the same two paths. Both drop the shared
  rows through `loader::world_scoped` before resolving, and both spell the two
  scopes as two functions: `delete::shared`/`delete::for_world`,
  `export::shared`/`export::for_world`.
- **Within a world, `delete` removes every copy** rather than refusing as
  ambiguous the way every other command does (`catalog::resolve_all`) — the
  database key *and* the world's own pack file. `--source` narrows it.
- **`--pack` is not declared on `delete`, `export`, or `structures`.** On the
  first two `--world` expresses scope and the two can contradict each other
  (`--world W --pack shared` asks for the copy `--world` exists to exclude); on
  `structures` it narrowed a listing that was never ambiguous, since a world's
  view is meant to show both packs and the SOURCE column already says which
  pack each row is in. clap refuses it, exit 2. It survives only on `copy`,
  which resolves one name out of the two packs a source world sees.
- `delete --world` is the one form that opens a world's own database rather
  than a copy; `--source pack` short-circuits that and provably never opens
  one.
- **Shared vs per-world target is expressed by absence.** `structures`,
  `import`, `export` and `delete` with no `--world` all mean the shared pack.
  There is no explicit token for "shared".

- **Tab completion follows the same scoping.** `complete_structures`
  (`complete.rs`) offers the shared copy's names for a bare `export`/`delete`,
  the world's own database and pack under `--world`, and everything `copy`'s
  source world sees — so a completed name is never one the command then
  refuses.
- **Installation is never a flag.** `structures` (no world), `import` (no
  `--world`), `export` (no `--world`), `delete` (no `--world`),
  `install` (no `--world`), and `status` resolve it from
  `CONSTRUCT_INSTALLATION` → `default_installation` → sole candidate
  (`main.rs:130,203,260,278`). `--com-mojang` roots get synthetic names
  `flag1`, `flag2`, … (`main.rs:44`).
- **`install --world` does three separate things**: enable both packs in the
  world, ensure a structures home exists, flip Beta APIs on (`install.rs:123`).
  Partial failure exits 5 from inside the command (`install.rs:272`), bypassing
  `exit_code`.
- **`install` rescues a Construct in the wrong folder first.** Before placing
  anything it folds a copy sitting in `com.mojang/behavior_packs` /
  `resource_packs` into the `development_*` sibling beside it
  (`install.rs:migrate_stray` → `install::adopt`), because the game loads both
  roots and no command here looks in the non-development one. With no copy in
  the development root the pack is moved whole; with one already there only
  its `structures/` is folded in, and a structure that clashes by name but
  differs by content is kept as `<name>-1.mcstructure` rather than dropped.
  Reported in the human output and in `migrated[]` under `-o json`.

## Validation hand-rolled in `main.rs`, not expressed in clap

Each of these prints its own `error:` + usage line and `exit(2)`. All are
candidates for clap-native expression (`conflicts_with`, `requires`,
`ArgGroup`, a value parser) in any rework:

| Rule | Where |
| ---- | ----- |
| `--merge` requires `-o` | `main.rs:148` |
| `-o` with >1 structure and no `--merge` | `main.rs:178` |
| `-o` must end `.mcstructure` (missing ext is filled in, wrong ext refused) | `main.rs:161`, `mcstructure_path` at `main.rs:309` |
| `--name` with >1 file on `import` | `main.rs:205` |
| `--source world` with no `--world` on `structures` | `main.rs:134` |
| `--source world` with no `--world` on `export` | the `Export` arm in `main.rs` |

## Seams worth reworking

1. **World is a positional in `copy`/`enable-beta-apis` but a flag in
   `structures`/`export`/`import`/`install`/`delete`.** Mostly closed. `delete`
   moved to the flag camp to be `import`'s mirror image — absence means the
   shared copy for both — and `export` followed, which also retired its
   `--pack`: with the shared copy out of scope under `--world` there is no
   second pack left to choose between. `structures` moved too, and retired
   `--pack` for the opposite reason: a world listing shows every pack that
   world sees, so there was nothing to pick between. What remains is `copy`, whose two worlds
   are both required and positional, and `enable-beta-apis`, which takes a world
   and nothing else. Neither has a shared-vs-world sense for a flag to express,
   so the split that is left tracks a real difference rather than an
   inconsistency.
2. **`world` is overloaded across the two selectors**: `--source world` means
   the world's *database*, `--pack world` means the world's *pack*. Same token,
   two referents, and they can be combined.
3. **`install --version` shadows the conventional `-V/--version`.** clap keeps
   them distinct because the global is `-V` on the root, but `construct install
   --version` reads as "print version".
4. ~~**Arity is inconsistent**~~ — closed. `export`, `copy`, `delete`, and
   `import` all take N, each resolving the whole batch before it writes or
   unlinks anything, so a bad name leaves the job untouched rather than half
   done. Closing it moved `copy`'s destination ahead of its structures
   (`copy <src> <dst> <s…>`), which is the one breaking change in the set.
5. ~~**Globals are accepted where they do nothing**~~ — closed. `--source`,
   `--pack`, `--force`, and `--com-mojang` are all per-command now; `--json` is
   the only survivor, and it genuinely is universal. The cost is that
   `--com-mojang` is declared nine times (via a flattened `Discovery`) to
   exclude the two commands that never search, and that flags no longer work
   ahead of the subcommand: `construct --com-mojang <path> worlds` is now
   `construct worlds --com-mojang <path>`.
6. **A second experiment toggle has nowhere to go.** `enable-beta-apis` is named
   for its one toggle, so another would be another verb command
   (`enable-<toggle>`), and there is still no way to turn any of them off.
7. **`-o` is the only short flag.** Either commit to shorts or drop it.
8. **Exit 5 escapes the error taxonomy** — emitted directly so the success JSON
   already printed isn't clobbered. Any restructure of partial-success reporting
   has to keep that ordering property.

## Contracts a rename would break

- **JSON payloads** — one struct per command, `schema: 1` + `warnings` injected
  at emit (`output.rs:56`). Keys today: `worlds.rs:7,12` · `structures.rs:11,18` ·
  `export.rs:21,27,201,207` · `import.rs:16,26` · `copy.rs:20,40` ·
  `delete.rs:31,37` · `enable_beta_apis.rs:15` · `install.rs:17` ·
  `status.rs:25,36`. `structures`, `export`, `copy` identify worlds by *qualified
  reference*; human lines use display name.
- **The plural commands always emit an array**, one row per item, whatever the
  count — `export`/`copy`/`import` under `written`, `delete` under `deleted`.
  Fields that describe the invocation rather than a row sit at the top level:
  `copy`'s `from`/`to`/`scope` and `import`'s `pack`/`scope`, because one
  destination home is chosen per command. `export` names its scope the same
  way: `world` is null under the shared form and `scope` is `shared`/`world`
  per command, since the grammar picks one and no row in a batch can escape it.
  `delete` keeps `scope` per row —
  its entries are found by name across every pack the world sees, so two in
  one batch can come from different packs. Since stage 4 a `delete` row is one
  *copy removed*, not one name: deleting a name that lives in the database and
  in both packs emits three rows sharing a `name`, told apart by the new
  `source` field (`world`/`pack`). A `world` row carries `path: null` and
  `scope: null`.
- **Exit codes** (`main.rs:483`): 0 ok · 1 fail · 2 usage/ambiguous/malformed ·
  3 not-found · 4 world in use · 5 partial install. `NotImplemented` was
  removed in stage 4 — `delete` was its only producer — and `CoreError::Internal`
  took its slot in the enum, exiting 1.
- **Error hints in `report`** (`main.rs:321`) hard-code command syntax in prose:
  `construct worlds --com-mojang <path>`, `construct structures --world <world> --source
  world`, `construct <command> ... --pack world`, `construct import <file>
  --name <name>`, `construct enable-beta-apis <world>`,
  `construct install --world <world>`. Grep `construct ` in `main.rs` and
  `install.rs` after any rename.
- **Tests** (`crates/construct-cli/tests/cli.rs`, 4.4k lines) pin flag spellings:
  `--com-mojang` ×126, `--world` ×88, `--json` ×37, `--source` ×30,
  `--merge` ×7, `--name` ×7, `--force` ×6, `--pack` ×3. They also pin flag
  *position*: everything but `--json` must now follow the subcommand.
  They also pin `copy`'s positional order, which the plural rework changed.
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
  `CONSTRUCT_GITHUB_API`, `CONSTRUCT_STATE_DIR` (where `writemark` keeps its
  per-world records; exists so tests do not write into the real data
  directory, not a documented user knob).
- **Config**: `default_installation`, `[[roots]] name/path`,
  `[backups] dir/keep` (default 10). Precedence: flag → env → file → discovery.
