# Integrating ConstructCLI

How to drive `construct` from another program. `--help` remains the reference
for the command grammar; this covers the contracts a caller depends on.

## Output

Pass `--json`. It is the only global flag, so it is the only one that may
precede the subcommand — every other option must follow it
(`construct worlds --path X`, not `construct --path X worlds`).

- **stdout is exactly one JSON document.** No progress lines or banners. Parse
  it whole.
- **stderr is human text** — warnings and progress. Never parse it.
- **A failure prints no JSON.** stdout is empty, the reason is on stderr as
  `error: <message>`. Branch on the exit code, not on the payload. Exit 5 is
  the sole exception.

Every payload carries two injected keys: `schema` (integer, `1` today — check
it before trusting field names) and `warnings` (array of strings, also printed
to stderr as they happen). Warnings with exit 0 mean success plus something
worth showing the user.

`add` emits no JSON; `completions` prints a shell script regardless.

## Exit codes

| Code | Meaning | Retry? |
| ---- | ------- | ------ |
| 0 | Success | — |
| 1 | Failure — I/O, database, network, malformed pack, bad config | Depends |
| 2 | Usage error — bad flags, or an ambiguous or malformed reference | No, fix the input |
| 3 | Not found — no installation, world, structure, or release asset | No |
| 4 | World in use — Minecraft has it open | Yes, once closed |
| 5 | Partial — `install` placed the packs but a later step failed | Yes, `install` is safe to repeat |

- **Ambiguity and malformed references are 2, not 3**: the target exists but was
  underspecified, or never named anything real. stderr lists the qualified
  references or `--source` values that would disambiguate.
- **Exit 4 has no `--force`.** Writing to a live world is either silently
  reverted or corrupts the save.
- **Exit 5 still prints its JSON**, before the stderr explanation. Its
  `enable_error`, `level_dat_error`, and `structures_error` fields say which
  steps failed.

## Payloads

`world` is `null` where it means the shared copy of Construct rather than one
world. Sizes are bytes. Worlds appear as *qualified references* except in
`status` and `install`, which report display names.

**`worlds`**
```json
{ "worlds": [ { "installation": "release", "account": null, "folder": "aB3=",
  "display_name": "My World", "qualified": "release/aB3=", "path": "/…",
  "size_bytes": 461992, "last_played": 1785913389,
  "last_played_source": "level.dat" } ] }
```
`account` is null on installations that are not per-account. `last_played` is
Unix seconds, may be null, and `last_played_source` is `level.dat` or
`dir-mtime` — the two disagree often.

**`structures`**
```json
{ "world": "release/aB3=", "structures": [
  { "name": "3x3", "id": "mystructure:3x3", "source": "world-db", "size_bytes": 8336 },
  { "name": "Amelix:xmas", "id": "Amelix:xmas", "source": "shared-pack", "size_bytes": 29231 } ] }
```
`name` is what the user types; `id` adds the implicit `mystructure:` namespace.
They are equal for an explicitly namespaced structure.

**`export`**
```json
{ "world": null, "written": [
  { "name": "Amelix:xmas", "path": "Amelix_xmas.mcstructure", "bytes": 29231 } ] }
```
`:` is replaced in derived filenames. `--merge` replaces `written` with
`merged`:
```json
{ "world": null, "merged": { "path": "out.mcstructure", "bytes": 12384,
  "sources": ["a", "b"], "size": [16, 8, 16], "origin": [100, 64, -20],
  "overlaps": [ { "count": 12, "pieces": ["a", "b"] } ] } }
```
`size`/`origin` are `[x, y, z]`. `overlaps` reports blocks claimed by more than
one piece; `--on-overlap last|first|error` picks the winner, and `error` makes
an overlap a failure (exit 1) instead of a report.

**`import`**
```json
{ "pack": "/…/Construct[BP]", "target": "shared-pack", "written": [
  { "name": "house", "id": "mystructure:house", "path": "/…", "bytes": 4497 } ] }
```

**`copy`**
```json
{ "from": "release/aB3=", "to": "release/cD4=", "target": "world-pack",
  "written": [ { "name": "house", "id": "mystructure:house", "path": "/…", "bytes": 4497 } ] }
```

**`delete`**
```json
{ "world": null, "deleted": [
  { "name": "house", "id": "mystructure:house", "source": "world-pack", "path": "/…" } ] }
```
A row is one *copy removed*, not one name: within a world, `delete` removes the
database key and the pack file, emitting two rows sharing a `name` and told
apart by `source`. Narrow with `--source`. A `world-db` row has `path: null`.

**`enable-beta-apis`**
```json
{ "world": "release/aB3=", "beta_apis": true, "changed": false, "backup": null }
```
`changed` is false when it was already on; no backup is taken in that case.

**`install`**
```json
{ "version": "1.2.3", "tag": "v1.2.3", "behavior": "/…", "resource": "/…",
  "preserved": 12, "migrated": [], "world": null, "beta_apis": null,
  "structures_pack": null, "enable_error": null, "level_dat_error": null,
  "structures_error": null }
```
`preserved` counts structures carried across the version bump. `migrated[]`
records a Construct rescued out of a non-development pack root:
`{ "kind": "moved"|"merged", "from": "/…", "to": "/…", "merged": 0,
"rescued": [ { "from": "…", "to": "…" } ], "left_behind": null }`.

**`status`**
```json
{ "installation": "release", "installed": "1.2.3", "pack": "/…",
  "latest": "v1.2.4", "enabled_worlds": ["My World"], "structures": [
  { "world": null, "source": "shared-pack", "path": "/…", "count": 52 } ] }
```
`installed` is a bare version; `latest` is the GitHub tag, usually `v`-prefixed
— strip it before comparing. `latest` is null when GitHub was unreachable,
which is a warning, not a failure: the command still exits 0.

## Vocabulary

**`source`** — one axis over three places, used as the `--source` values, the
`source` field in payloads, and the SOURCE column, so a row feeds straight back
into the CLI. Commands naming a *destination* spell it `target`, same
vocabulary.

| Value | Place |
| ----- | ----- |
| `world-db` | The world's own leveldb database |
| `world-pack` | A pack serving one world — its structures pack, or its own copy of Construct |
| `shared-pack` | The shared copy of Construct, seen by every world using it |

It filters rather than tie-breaks: naming a value drops non-matching rows.
`world-db` and `world-pack` are refused without `--world`; `shared-pack` is
refused *with* `--world` on `export` and `delete` (both exit 2). It is allowed
on `structures`, the only way to list a world's structures without opening its
database — naming any pack source never opens the database at all, since
opening a leveldb runs recovery and rewrites it.

On `copy` it selects the *source* world only. There is no flag for where a copy
lands; it is always the destination world's own structures home.

**Shared vs per-world is expressed by absence.** `structures`, `import`,
`export`, and `delete` with no `--world` mean the shared copy. With `--world`
they mean that world's database and its own pack, and cannot reach the shared
copy at all.

**World references** resolve as: filesystem path (checked first), display name,
folder name, or qualified `<installation>/<account>/<world>` with segments
optional from the left. A display name may itself contain `/` — Minecraft's
default level name embeds a date — so the whole input is matched as a name as
well as split into segments. Prefer the `qualified` field from `worlds`: it is
unambiguous across installations and round-trips into every command taking a
world. Reserved installation names are `release`, `preview`, `legacy`,
`mcpelauncher`, `path`, and `env`; `--path` roots are numbered `flag1`,
`flag2`, …, and a world folder given to `--path` is filed under `path`.

**Structure names** use segments of `A-Za-z0-9_.-`, capitals preserved. A bare
name becomes `mystructure:<name>` at `structures/<name>.mcstructure`; an
explicit namespace becomes `structures/<ns>/<name>.mcstructure` and is invisible
to Construct's in-game list. Namespaced names may nest (`a:b/c`); the default
namespace may not. `..` is refused at any depth.

## Environment and config

| Variable | Effect |
| -------- | ------ |
| `CONSTRUCT_CONFIG` | Path to `config.toml` |
| `CONSTRUCT_INSTALLATION` | Installation to use when several are discovered |
| `CONSTRUCT_COM_MOJANG` | Extra `com.mojang` root, discovered as `env` |
| `CONSTRUCT_GITHUB_TOKEN`, `GITHUB_TOKEN` | Raises the GitHub rate limit for `install`/`status` |
| `CONSTRUCT_GITHUB_API` | Override the GitHub API base, for stubbing in tests |

Precedence is flag → environment → `config.toml` → discovery. The installation
is never a flag: it resolves from `CONSTRUCT_INSTALLATION`, then
`default_installation`, then the sole candidate — with several and no default,
commands needing one exit 2.

`config.toml` keys: `default_installation`, `[[roots]]` with `name`/`path`,
`[backups]` with `dir`/`keep` (default 10). Add a root non-interactively with
`construct add <path>`.

## Notes

- **Batches are all-or-nothing at resolve time.** `export`, `import`, `copy`,
  and `delete` take N items and resolve the whole batch before writing or
  unlinking, so a bad name leaves the job untouched. They always emit an array,
  whatever the count.
- **Budget for the in-use check.** Confirming a live world watches its database
  for up to 20 seconds, so a write against an open world can take that long to
  exit 4. Set timeouts above it.
