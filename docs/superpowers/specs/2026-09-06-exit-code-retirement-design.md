# Retiring exit codes 3–5 — Design

**Date:** 2026-09-06
**Status:** Approved design, ready for implementation planning

## 1. Problem

`construct` exits with six distinct codes today: 0 success, 1 failure, 2 usage,
3 not found, 4 world in use, 5 partial install. Three of those encode facts
about *what went wrong* rather than *whether it went wrong*, and they encode
them in the one channel that has no room to carry them well.

The costs compound:

- **The taxonomy is coarse where it matters and arbitrary where it doesn't.**
  Exit 3 lumps a missing installation, a missing world, a missing structure,
  and a missing release asset into one number. Meanwhile ambiguity is 2 rather
  than 3, and a malformed reference is 2 as well — defensible rulings, but
  ones every caller has to be taught, and ones the code comments in
  `exit_code` have to keep justifying.
- **Exit 5 already escapes the taxonomy.** `commands::install::run` calls
  `std::process::exit(5)` directly, bypassing `exit_code` entirely, because
  returning an error would clobber the success payload it has already printed.
  `docs/cli-surface.md` lists this as seam #8.
- **Growth has nowhere to go.** `CoreError` has 29 variants and one number per
  interesting failure does not scale. The next distinction worth drawing needs
  a code 6.
- **The information is wanted by programs, and programs already pass `--json`.**
  `--json` is the integration channel. It is a structured document with an
  injected `schema` and `warnings`, and it can carry a name for the failure at
  no cost to anyone reading plain output.

`--json` currently refuses to carry it: *"A failure prints no JSON. stdout is
empty, the reason is on stderr as `error: <message>`. Branch on the exit code,
not on the payload."* That rule is what forces the exit code to be expressive
in the first place.

## 2. Goals and non-goals

**Goals**

- Reduce the exit code surface to 0, 1, and 2.
- Give `--json` a machine-readable name for every failure, finer-grained than
  the codes it replaces.
- Make `error.kind` present on every nonzero exit that reaches a `CoreError`,
  with no command-specific exceptions — including `install`'s partial case,
  which is the one non-`CoreError` failure that gets a kind.
- Close seam #8: no `std::process::exit` from inside a command for a reason the
  error taxonomy should have expressed.
- Keep human-facing stderr output exactly as it is.

**Non-goals**

- Changing any success payload's fields.
- Bringing clap's own errors, or the hand-rolled usage checks in `main.rs`,
  into the JSON error contract. They are grammar mistakes, not results, and
  exit 2 survives to report them.
- Adding structured per-variant detail (candidate lists, `near` suggestions,
  probed paths) to the error document. Considered and rejected — see §4.
- Bumping `schema`. Nothing consumes this contract yet.

## 3. Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Surviving codes | 0 success · 1 failure · 2 usage | Whether it worked, and whether the input was the problem — the two things a code can carry honestly |
| Where 3 and 4 go | 1 | Both are failures; the distinction moves to `error.kind` |
| Where 5 goes | 1 | Partial install is a failure with a useful payload, not a third outcome |
| Error document | `{"error": {"kind", "message"}}` plus injected `schema` and `warnings` | Same envelope as every success payload |
| Granularity | One `kind` per `CoreError` variant | Strictly finer than the codes; costs nothing to be specific |
| Detail fields | None | A per-variant detail schema would have to stay stable; `message` plus stderr prose covers today's needs |
| Where `kind` lives | `CoreError::kind()` in `construct-core` | A GUI links the library directly (INTEGRATION.md); the kind is the error's identity, not a CLI rendering choice |
| Match style | Exhaustive, no wildcard arm | A new variant must answer the question rather than inherit an answer — the discipline `Command::paths()` already uses |
| Which failures emit it | Every `CoreError`, whether it exits 1 or 2 | The ambiguity and malformed-reference cases keep a machine-readable name, which is what exit-3-vs-2 encoded |
| clap and hand-rolled usage errors | Unchanged, stderr only | Not results; exit 2 already says "fix the input" |
| `install` partial | Emits its payload **and** a merged `error` key, exits 1 | Preserves the ordering property while keeping "nonzero ⇒ `error.kind`" universal |
| `schema` | Stays `1` | No consumers yet |
| Plain (non-`--json`) output | Unchanged | The stderr prose in `report()` becomes the only signal, by design |

## 4. Rejected alternatives

**Structured `details` per variant.** An `error.details` object carrying the
variant's own data — `candidates` for an ambiguity, `near` for a
did-you-mean, `probed` for a missing installation — would let a GUI render its
own disambiguation picker instead of scraping the prose `report()` prints.
Rejected for now because it commits 29 variants to individually stable field
schemas in exchange for a consumer that does not exist yet. `kind` plus
`message` restores everything the exit codes said and more. Adding `details`
later is additive and does not break a caller reading `kind`.

**A coarse category (`not-found` / `in-use` / `ambiguous` / `usage` /
`failure`).** A literal 1:1 replacement for the retired codes. Rejected
because it reproduces exactly the lumping that made the codes inadequate — one
name for four different missing things — while being no cheaper to implement
than the per-variant version.

**Exit 0 for partial install.** The packs did land, and the payload is a
success payload. Rejected: `install --world` was asked to do three things and
did not do them all, and a caller that treats 0 as "done" would be wrong.
Exit 1 with a payload the caller can read is the honest report.

**Error document only for partial install, discarding the payload.** Most
uniform, but throws away the version and pack paths the caller needs to
recover, and undoes the ordering property `install.rs` was deliberately built
around.

## 5. The contract, after

Exit codes:

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Failure |
| 2 | Usage error — bad flags, or an ambiguous or malformed reference |

Under `--json`:

- Success prints one document, unchanged.
- **Failure prints one document too**, carrying `error`. This inverts today's
  rule.
- `error.kind` is present on every nonzero exit that reaches a `CoreError`,
  and on `install`'s partial case.
- clap errors and the hand-rolled usage checks in `main.rs` print nothing on
  stdout and exit 2, as today.

Without `--json`, stderr prose is the only signal. A shell script can no
longer distinguish "world in use, retry once closed" from "the disk failed".
That is the accepted cost of the change; `report()`'s output is unchanged so a
human loses nothing.

Callers branch on `error.kind`, not on the exit code.

## 6. The `kind` vocabulary

Kebab-case of the `CoreError` variant name, all 29:

```
no-installations        world-not-found         malformed-reference
ambiguous-world         structure-not-found     ambiguous-structure
ambiguous-installation  installation-not-found  world-in-use
insufficient-space      target-exists           db
bad-level-dat           unwritable-level-dat    unreadable-world
bad-config              invalid-path            bad-pack
bad-structure-file      merge-refused           construct-not-installed
incomplete-install      bad-structure-name      internal
no-backup-dir           network                 rate-limited
asset-not-found         io
```

Plus one that is not a `CoreError` variant: **`partial-install`**, emitted by
`commands::install::run`.

Mapping from the retired codes, for anyone porting:

| Was | Now |
|---|---|
| 3 | `no-installations`, `world-not-found`, `structure-not-found`, `installation-not-found`, `construct-not-installed`, `asset-not-found` |
| 4 | `world-in-use` |
| 5 | `partial-install` |

The five variants that exit 2 keep doing so, and now also carry a kind:
`ambiguous-world`, `ambiguous-structure`, `ambiguous-installation`,
`malformed-reference`, `bad-structure-name`.

## 7. Components

### `CoreError::kind` — `crates/construct-core/src/error.rs`

```rust
impl CoreError {
    /// The stable machine-readable name for this failure, as `error.kind`
    /// under `--json`.
    pub fn kind(&self) -> &'static str { /* exhaustive match, no wildcard */ }
}
```

No `Serialize` impl on `CoreError`: `kind()` returns `&'static str` and
`message` is the existing `Display`, so the CLI builds the object with
`json!`. Nothing per-variant has to stay stable.

### `Out` — `crates/construct-cli/src/output.rs`

Two methods beside `emit`, both no-ops without `--json`:

- `emit_error(&self, err: &CoreError)` — emits
  `{"error": {"kind", "message"}, "schema": 1, "warnings": [...]}`.
- `emit_with_error<T: Serialize>(&self, payload: T, kind: &str, message: String)`
  — merges `error` into the payload map the way `schema` and `warnings` are
  already merged.

`emit`, `emit_error`, and `emit_with_error` all route through one private
method taking the payload map and an optional `(kind, message)`, so the
injection of `schema` and `warnings` keeps a single home.

No payload uses a bare `error` key (`install`'s are `enable_error`,
`level_dat_error`, `structures_error`), so the merge cannot collide.

### `main.rs`

The error arm becomes:

```rust
Err(err) => {
    out.emit_error(&err);   // stdout first
    report(&err);           // then stderr
    std::process::exit(exit_code(&err));
}
```

stdout before stderr, matching the ordering `install.rs` already uses.

`exit_code` keeps only its 2-arm and `_ => 1`; its doc comment loses the 3/4/5
paragraphs and the explanation of why 5 bypasses it.

### `commands/install.rs`

Compute `partial` before emitting, pick the emitter, then print the same
stderr recovery steps and exit **1**:

```rust
let partial = enable_error.is_some() || level_dat_error.is_some()
    || structures_error.is_some();
let payload = Payload { /* unchanged */ };
if partial {
    out.emit_with_error(
        payload,
        "partial-install",
        "the packs are installed, but a later step did not finish",
    );
} else {
    out.emit(payload);
}
if partial { /* unchanged stderr */ std::process::exit(1); }
```

The `std::process::exit` stays — the payload-before-error ordering still
requires it — but it no longer carries a meaning absent from the taxonomy,
which is what seam #8 was about.

## 8. Testing

**Changed:** the ~25 assertions in `crates/construct-cli/tests/cli.rs` matching
`Some(3)`, `Some(4)`, `Some(5)` become `Some(1)`. The two `Some(4)` delete
tests additionally assert their existing database-untouched property, which is
unaffected.

**New:**

- One test per retired code's representative case asserting `error.kind` under
  `--json`: a missing world (`world-not-found`), a live world
  (`world-in-use`), a partial install (`partial-install`).
- An exit-2 `CoreError` under `--json` carries its kind
  (`ambiguous-structure`).
- A hand-rolled usage error under `--json` prints nothing on stdout and exits
  2, pinning the deliberate exclusion.
- A failure *without* `--json` still prints nothing on stdout.
- `install`'s partial case emits its payload fields **and** `error` in one
  document — pinning both halves of the merge.

**Property to keep:** stdout is still exactly one JSON document in every case.

## 9. Documentation

| File | Change |
|---|---|
| `README.md` | The exit code table, down to three rows |
| `INTEGRATION.md` | "Output" section (the no-JSON-on-failure rule inverts), the exit code table and its three bullets, plus the exit-code mentions at the `--on-overlap error`, `--source`, installation-resolution, and in-use-budget passages |
| `docs/cli-surface.md` | The exit-code contract line, seam #8, and the `install.rs:272` partial-failure note |
| `docs/manual-verification.md` | The step expecting exit 4 |
| `docs/carried-forward.md` | Historical notes mentioning exits 4 and 5 — annotate rather than rewrite; they record what was true at the time |

INTEGRATION.md gains a short section documenting the error document and the
`kind` vocabulary.
