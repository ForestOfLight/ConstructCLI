# CLI surface

The whole command grammar in one place, for reworking it. Not user docs —
`--help` is that. Everything below is what the code actually does today, with
anchors for editing and a list of the seams where the grammar is inconsistent.

## The grammar today

`v` = takes a value · `[v]` = optional value · `…` = repeatable · **bold** = required

| Command | Positionals | Own options |
| ------- | ----------- | ----------- |
| `worlds` | — | — |
| `structures` | — | `-w/--world v`, `--source v` (`world-db`\|`world-pack`\|`shared-pack`) |
| `export` | **`structures…`** | `-n/--name v`, `--merge`, `--on-overlap v` (`last`\|`first`\|`error`, default `last`), `-w/--world v`, `--source v`, `--force` |
| `import` | **`files…`** | `-w/--world v`, `--name v`, `--force` |
| `copy` | **`src_world`** **`dst_world`** **`structures…`** | `--source v`, `--force` |
| `delete` | **`structures…`** | `-w/--world v`, `--source v` |
| `enable-beta-apis` | **`world`** | — |
| `install` | — | `--version v`, `-w/--world v`, `--force` |
| `status` | — | — |
| `completions` | — | **`shell`** (`bash`\|`elvish`\|`fish`\|`powershell`\|`zsh`) |

`--json` is the only global left on `Cli` (`cli.rs:13`), so it is the only flag
that may precede the subcommand. Every other option is declared on the commands
that read it, and a command refuses one it would have ignored (exit 2, with
clap's own `tip: '<cmd> --flag' exists`).

`--path v…` is the one that is nearly universal without being universal.
It rides on a `Discovery` struct flattened into every command that searches for
Minecraft (`cli.rs:26`), which is all of them except `add` — it writes the path
it was handed into the config file — and `completions`, which prints a script.
`main` reads the whole set back through `Command::paths()`, an exhaustive
match, so a new command has to answer the question rather than inherit an
answer. `main` then sorts each value with `discovery::classify`: a directory
holding a `level.dat` is a world and is folded into the enumeration under the
reserved installation name `path`; anything else is probed as a `com.mojang`
root. This is also why `worlds` reports "no installation found" from
`nothing_to_search` rather than `installations.is_empty()` — a world named by
path belongs to no installation, and is still something to search.

`--force` is declared on `export`, `import`, `copy`, and `install`, each with
its own help text for what it overwrites (an output file · a structure of that
name in the target pack · the version already installed). It is absent from
`worlds`, `structures`, `delete`, `enable-beta-apis`, and `status`, and it
deliberately never overrode `delete`'s in-use refusal — that refusal is now
unreachable by flag rather than merely unmoved by one.

`--source` and `--pack` were the first two to move, for the same reason:
`construct worlds --pack shared` and `construct import f.mcstructure --source
world-pack` are refused by clap instead of parsing and doing nothing. `--pack`
has since been retired outright — see the next section.

## Semantics you can't read off the grammar

- **`--source` is one axis over three places, not two selectors.** It takes
  `world-db`, `world-pack`, or `shared-pack` — the world's leveldb, a pack
  serving that world alone (its structures pack, or its own copy of Construct),
  and the shared copy in `development_behavior_packs`. `--pack world|shared` is
  gone: it existed only because `--source world-pack` could not say *which*
  pack, and one flag that names the place outright needs no second one beside
  it. The same three spellings are the `source` values in JSON and the SOURCE
  column in `structures`, so a row can be fed straight back to the CLI.
- **It is a filter, not just a tie-breaker.** Naming a value drops
  non-matching rows, both when resolving a name (`catalog::resolve`) and in the
  `structures` listing (`structures.rs`). `--source shared-pack` therefore also
  hides world-database rows.
- **Naming a pack source provably never opens the world's database.**
  `loader::for_world` short-circuits on `Source::is_pack()`, which is a
  correctness property rather than an optimisation: opening a leveldb runs
  recovery and rewrites it.
- **On `copy` it selects the *source* world, never the destination.** It is
  passed only to `loader::for_world(src, ..)` and `catalog::resolve` in
  `copy.rs`; the destination comes from `home_for_write(dst, ..)`, which does
  not take it. There is no flag that steers where a copy lands — it is always
  the destination world's own structures home. The help text says so, since the
  grammar alone can't. `copy` is also the only command where all three values
  are live at once, because its worlds are positionals and there is no
  `--world` for `shared-pack` to contradict.
- **`delete`'s and `export`'s scope is their grammar, not a filter.** `delete
  <name>` targets the shared copy of Construct; `delete <name> --world W`
  targets that world's database and its own pack and **cannot reach the shared
  copy at all** — a name that exists only there reports as missing from the
  world. `export` reads along exactly the same two paths. Both drop the shared
  rows through `loader::world_scoped` before resolving, and both spell the two
  scopes as two functions: `delete::shared`/`delete::for_world`,
  `export::shared`/`export::for_world`.
- **`--world W --source shared-pack` is refused on `export` and `delete`, and
  allowed on `structures`.** For the first two it asks for the copy `--world`
  exists to exclude, so it is a usage error (exit 2) rather than a request with
  no honest answer. A listing is different: a world's view genuinely *contains*
  the shared copy's rows when that is what the world runs, so narrowing to them
  answers a real question — and it is the only way to list a world's structures
  without opening its database. `main.rs:check_source_against_world` takes a
  flag for the difference.
- **`--source world-db` or `world-pack` with no `--world` is refused
  everywhere it can appear.** Both name a place inside a world and none was
  named; `shared-pack` is the only value a world-less command can serve, and it
  is what that command already does by default.
- **Within a world, `delete` removes every copy** rather than refusing as
  ambiguous the way every other command does (`catalog::resolve_all`) — the
  database key *and* the world's own pack file. `--source` narrows it.
- **Shared vs per-world target is expressed by absence.** `structures`,
  `import`, `export` and `delete` with no `--world` all mean the shared pack.
  `--source shared-pack` names it but does not select it: it filters what a
  command already reaches rather than redirecting the command.

- **Tab completion follows the same scoping.** `complete_structures`
  (`complete.rs`) offers the shared copy's names for a bare `export`/`delete`,
  the world's own database and pack under `--world`, and everything `copy`'s
  source world sees — so a completed name is never one the command then
  refuses.
- **Installation is never a flag.** `structures` (no world), `import` (no
  `--world`), `export` (no `--world`), `delete` (no `--world`),
  `install` (no `--world`), and `status` resolve it from
  `CONSTRUCT_INSTALLATION` → `default_installation` → sole candidate.
  `--path` roots get synthetic names
  `flag1`, `flag2`, … (`main.rs:44`); world folders given to `--path` wear the
  reserved name `path` instead and are not numbered.
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
  Reported in the human output and in `migrated[]` under `--json`.

## Validation hand-rolled in `main.rs`, not expressed in clap

Each of these prints its own `error:` + usage line and `exit(2)`. All are
candidates for clap-native expression (`conflicts_with`, `requires`,
`ArgGroup`, a value parser) in any rework:

| Rule | Where |
| ---- | ----- |
| `--merge` requires `-n` | `main.rs:148` |
| `-n` with >1 structure and no `--merge` | `main.rs:178` |
| `-n` must end `.mcstructure` (missing ext is filled in, wrong ext refused) | `main.rs:161`, `mcstructure_path` at `main.rs:309` |
| `--name` with >1 file on `import` | `main.rs:205` |
| `--source world-db`/`world-pack` with no `--world` on `structures`, `export`, `delete` | `main.rs:check_source_against_world` |
| `--source shared-pack` with `--world` on `export`, `delete` | the same, with `world_excludes_shared` set |

## Seams worth reworking

1. **World is a positional in `copy`/`enable-beta-apis` but a flag in
   `structures`/`export`/`import`/`install`/`delete`.** Mostly closed. `delete`
   moved to the flag camp to be `import`'s mirror image — absence means the
   shared copy for both — and `export` and `structures` followed. What remains
   is `copy`, whose two worlds are both required and positional, and
   `enable-beta-apis`, which takes a world and nothing else. Neither has a
   shared-vs-world sense for a flag to express, so the split that is left
   tracks a real difference rather than an inconsistency.
2. ~~**`world` was overloaded across the two selectors**~~ — closed, by
   deleting one of the selectors. `--source` and `--pack` were two axes over
   one question, and neither could answer it alone: `--source world-pack` did
   not say which pack, `--pack shared` did not say pack-or-database. They are
   now one flag over three places (`world-db`\|`world-pack`\|`shared-pack`),
   so the bare token `world` appears in no value at all and there is nothing
   left to collide. The same three spellings carry through to the `source`
   field in JSON and the SOURCE column in `structures`.
3. **`install --version` shadows the conventional `-V/--version`.** clap keeps
   them distinct because the global is `-V` on the root, but `construct install
   --version` reads as "print version".
4. ~~**Arity is inconsistent**~~ — closed. `export`, `copy`, `delete`, and
   `import` all take N, each resolving the whole batch before it writes or
   unlinks anything, so a bad name leaves the job untouched rather than half
   done. Closing it moved `copy`'s destination ahead of its structures
   (`copy <src> <dst> <s…>`), which is the one breaking change in the set.
5. ~~**Globals are accepted where they do nothing**~~ — closed. `--source`,
   `--force`, and `--path` are all per-command now; `--json` is
   the only survivor, and it genuinely is universal. The cost is that
   `--path` is declared nine times (via a flattened `Discovery`) to
   exclude the two commands that never search, and that flags no longer work
   ahead of the subcommand: `construct --path <path> worlds` is now
   `construct worlds --path <path>`.
6. **A second experiment toggle has nowhere to go.** `enable-beta-apis` is named
   for its one toggle, so another would be another verb command
   (`enable-<toggle>`), and there is still no way to turn any of them off.
7. **Shorts are uneven.** `-w`, `-n` exist; `import --name` has no short.
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
- **There is one place vocabulary, and `scope` is gone from every payload.**
  `source` is `world-db`\|`world-pack`\|`shared-pack` wherever a row names
  where a structure lives — `structures` rows, `delete` rows, and `status`'s
  pack rows. The two commands that name a *destination* carry `target` in the
  same spelling (`world-pack`\|`shared-pack`): `import` alongside `pack`, and
  `copy` alongside `from`/`to`, because one destination home is chosen per
  invocation. `export` and `delete` dropped their per-command field outright —
  `world` already carries it, since `null` means the shared copy and nothing
  else while a world means that world and never the shared copy.
- **The plural commands always emit an array**, one row per item, whatever the
  count — `export`/`copy`/`import` under `written`, `delete` under `deleted`.
  Since stage 4 a `delete` row is one *copy removed*, not one name: deleting a
  name that lives in the database and in a pack emits two rows sharing a
  `name`, told apart by `source`. A `world-db` row carries `path: null`.
- **Exit codes** (`main.rs:483`): 0 ok · 1 fail · 2 usage/ambiguous/malformed ·
  3 not-found · 4 world in use · 5 partial install. `NotImplemented` was
  removed in stage 4 — `delete` was its only producer — and `CoreError::Internal`
  took its slot in the enum, exiting 1.
- **Error hints in `report`** hard-code command syntax in prose:
  `construct worlds --path <path>`, `construct <command> ... --source
  world-pack`, `construct import <file> --name <name>`, `construct
  enable-beta-apis <world>`, `construct install --world <world>`. The
  ambiguity hint is the exception — it now reads the matched `source` values
  back rather than naming values in prose, so it cannot drift from the enum.
  Grep `construct ` in `main.rs` and `install.rs` after any rename.
- **Tests** (`crates/construct-cli/tests/cli.rs`) pin flag spellings:
  `--path`, `--world`, `--json`, `--source`, `--name`, `--merge`, `--force`.
  Three of them pin that `--pack` is *not* accepted, so a reintroduction would
  be caught. They also pin flag *position*: everything but `--json` must follow
  the subcommand, and `copy`'s positional order, which the plural rework
  changed.
- **README usage block** (`README.md`, "## Usage") lists ~20 example
  invocations verbatim.

## Reference grammars (unchanged by any flag rework)

- **World**: display name · folder name · `<installation>/<account>/<world>`
  with segments optional from the left · filesystem path (checked first).
  A name may contain `/` itself — Minecraft's default level name embeds a date,
  as in `Advanced Automation 10/13/21 23:33:18` — so the whole input is matched
  as a name as well as split into segments, and the qualified reading wins only
  where it is the one that matches.
  Reserved installation names: `release`, `preview`, `legacy`, `mcpelauncher`,
  plus `path` and `env` (`config.rs:49`).
- **Structure name**: each segment `A-Za-z0-9_.-`, capitals kept. Bare name →
  `mystructure:<name>` → `structures/<name>.mcstructure`; explicit namespace →
  `structures/<ns>/<name>.mcstructure` and invisible to Construct's in-game list.
  A namespaced name may carry `/` to nest further (`a:b/c` →
  `structures/a/b/c.mcstructure`), which is what `import <dir>` writes and what
  `structures` has always read; every segment is validated separately, so `..`
  is refused at any depth. The default namespace is the exception and cannot
  nest: it has no folder of its own, so `mystructure:a/b` would be read back as
  `a:b`, and `path_for` refuses it rather than file it under another namespace.
  A *derived* name — a file stem, or `--name` — is still one segment, so depth
  only ever comes from real directories.
- **Env**: `CONSTRUCT_CONFIG`, `CONSTRUCT_INSTALLATION`, `CONSTRUCT_COM_MOJANG`
  (root named `env`), `CONSTRUCT_GITHUB_TOKEN`/`GITHUB_TOKEN`,
  `CONSTRUCT_GITHUB_API`, `CONSTRUCT_STATE_DIR` (where `writemark` keeps its
  per-world records; exists so tests do not write into the real data
  directory, not a documented user knob), `CONSTRUCT_BACKUPS_DIR` (the same
  for backups — it stands in for the platform data directory and loses to
  `[backups] dir`; also not a documented user knob).
- **Config**: `default_installation`, `[[roots]] name/path`,
  `[backups] dir/keep` (default 10). Precedence: flag → env → file → discovery.
