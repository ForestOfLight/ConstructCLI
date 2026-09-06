# Integrating ConstructCLI

How to drive `construct` from another program.

This documents the contracts — the envelope, the exit codes, the error
vocabulary, and the naming rules. It deliberately does **not** reproduce each
command's payload fields: run the command once with `--json` and read the
document it prints. `--help` is the reference for the command grammar.

## Output

Pass `--json`. It is the only global flag, so it is the only one that may
precede the subcommand — every other option must follow it
(`construct worlds --path X`, not `construct --path X worlds`).

- **stdout is exactly one JSON object**, on success and on failure alike. No
  progress lines, no banners. Parse it whole.
- **stderr is human prose** — warnings, progress, and an explanation of any
  failure. Never parse it.

Every document carries these keys, injected at emit time:

| Key | Always present | Meaning |
| --- | -------------- | ------- |
| `schema` | yes | Envelope version, `1` today. Check it before trusting field names. |
| `warnings` | yes | Array of strings. Non-fatal problems, also printed to stderr as they happen. |
| `error` | on failure | `{ "kind": …, "message": … }`. See below. |

The rest of the object is the command's own payload. Warnings with exit 0 mean
the command succeeded and something is still worth showing the user.

`add` emits no JSON; `completions` prints a shell script regardless.

## Exit codes

| Code | Meaning |
| ---- | ------- |
| 0 | Success |
| 1 | Failure |
| 2 | Usage error — the input is what needs fixing |

The code answers two questions only: did it work, and was the input at fault.
*What* went wrong is `error.kind`, which is finer than a number can be. Ambiguous
and malformed references are 2 rather than 1 because the target may well exist
and it is the reference that was underspecified.

**Do not treat a non-zero exit as "no output".** A failed command still prints
its document, and `install` in particular emits its payload *and* an `error` in
one object when the packs were placed but a later step failed — the paths and
version in that payload are what you need to recover.

The one gap: **argument-parsing errors produce no JSON.** Bad or conflicting
flags are caught before the command runs — by the parser, or by the checks that
reject combinations like `--merge` without `-n`, or `--source world-db` without
`--world` — and exit 2 with prose on stderr and empty stdout. Everything past
argument parsing emits a document. If you construct argument lists
programmatically, treat empty stdout with exit 2 as a bug in your caller.

## Errors

`error.kind` is a stable, machine-readable slug — the kebab-cased name of the
underlying failure, e.g. `world-not-found`, `world-in-use`, `target-exists`,
`ambiguous-world`, `rate-limited`. `error.message` is prose for a human and is
not a contract; branch on `kind`.

The authoritative set is `CoreError::kind()` in
[`crates/construct-core/src/error.rs`](crates/construct-core/src/error.rs),
plus `partial-install` from `install`. It is expected to grow, so **treat an
unrecognised kind as a generic failure** rather than failing to parse.

Kinds worth handling specifically:

- `world-in-use` — Minecraft has the world open. There is no `--force`; writing
  to a live world is either silently reverted or corrupts the save. Retry once
  the world is closed.
- `target-exists` — retry with `--force` if overwriting is what you want.
- `ambiguous-world`, `ambiguous-structure`, `ambiguous-installation` — the
  reference matched several things. stderr lists the qualified references or
  `--source` values that would disambiguate.
- `partial-install` — the packs are installed but a later step failed. `install`
  is safe to repeat.
- `rate-limited`, `network` — GitHub was unreachable or throttled. Set
  `CONSTRUCT_GITHUB_TOKEN` for the former.

## Conventions across payloads

These hold everywhere, so you can rely on them without this document listing
fields:

- **`world` is `null`** where it means the shared copy of Construct rather than
  one world.
- **Worlds are named by qualified reference** (`<installation>/<account>/<world>`)
  except in `status` and `install`, which report display names.
- **Sizes are bytes**, timestamps are Unix seconds, and either may be `null`
  where unknown.
- **Commands that can act on multiple structures always emit an array**
- **`source` and `target`** use the same three spellings as `--source`, so a row
  feeds straight back into the CLI.

## Vocabulary

### Structure Source

Refers to the location of the stored structures:
- `world-db` - a world's leveldb (`MyWorld/db`)
- `world-pack` - a world's Construct Structures pack, or its discrete copy of Construct (`MyWorld/behavior_packs`)
- `shared-pack` - the shared copy of Construct (`com.mojang/development_behavior_packs`)

## Environment and config

| Variable | Effect |
| -------- | ------ |
| `CONSTRUCT_CONFIG` | Path to `config.toml`. `--config` beats it |
| `CONSTRUCT_INSTALLATION` | Installation to use when several are discovered |
| `CONSTRUCT_GITHUB_TOKEN`, `GITHUB_TOKEN` | Raises the GitHub rate limit for `install`/`status` |

The release source is fixed at `https://api.github.com` and cannot be
redirected at runtime.

Precedence is flag → environment → `config.toml` → discovery. `--config` is
global, so it may appear anywhere on the line. The installation is never a
flag: it resolves from `CONSTRUCT_INSTALLATION`, then `default_installation`,
then the sole candidate — with several and no default, commands needing one
fail as `ambiguous-installation`.

`config.toml` keys:

| Key | Effect |
| --- | ------ |
| `default_installation` | Installation to use when several are discovered |
| `[[roots]]` with `name`/`path` | An extra `com.mojang` root to probe. `name` is required and becomes the installation name in qualified references |
| `other_worlds` | Array of save-folder paths outside any root. They are discovered under the `path` installation, exactly as `--path` would place them |
| `[backups]` with `dir`/`keep` | Where snapshots go, and how many to retain per world (default 10) |

`construct add <path>` writes to `roots` or `other_worlds`, whichever the path
turns out to be, and honours `--config` when deciding which file to write.

## Notes

- **Batches are all-or-nothing at resolve time.** `export`, `import`, `copy`,
  and `delete` take N items and resolve the whole batch before writing or
  unlinking, so a bad name leaves the job untouched.
- **Budget for the in-use check.** Confirming a live world watches its database
  for up to 20 seconds, so a write against an open or recently closed world can take that long to fail.
