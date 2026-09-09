# Contributing to ConstructCLI

Thank you for contributing to **ConstructCLI**.
This document explains how to get set up, what to check before opening a pull request,
and how PRs are handled.

## Before You Start

1. Fork the repository and create a branch from `main`.
2. Keep your branch focused on a single change.
3. Discuss larger changes in an issue first.

## Development Setup

You need Rust, plus **CMake** and a **C++ compiler** — the leveldb backend is Mojang's
own C++ implementation.

```bash
rustup toolchain install stable
rustup component add rustfmt clippy

git clone https://github.com/ForestOfLight/ConstructCLI.git
cd ConstructCLI
./scripts/setup-deps.sh   # run once, before your first build
cargo build
```

`setup-deps.sh` clones pinned checkouts of `bedrock-rs`, `leveldb-sys`, and `nbtx` into
`third_party/checkouts/` and applies the fixes in `third_party/patches/`, because the
upstream crates do not build or behave correctly for this tool as published.

Those checkouts are generated. Don't commit edits made inside them. To change a
dependency, edit its patch in `third_party/patches/`, delete the checkout, and re-run
the script:

```bash
rm -rf third_party/checkouts/nbtx
./scripts/setup-deps.sh
```

`leveldb-sys` is the exception. Its Rust wrapper and C++ shim are not patched but
replaced: `third_party/vendor/leveldb-sys/` holds our own Apache-2.0 copies, which the
script overlays onto the checkout, because upstream ships that code with no licence at
all. Edit those files in `vendor/`, not in the checkout and not through a patch — a
release is refused if the two disagree.

See `third_party/README.md` for what each patch fixes, and why `leveldb-sys` is
vendored.

## Checks Before a PR

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

CI runs these on Linux, macOS, and Windows, with clippy warnings treated as errors.

## The Crates

- `construct-core` — world and pack discovery, the leveldb read path, `.mcstructure`
  encoding/decoding, merging, and installation.
- `construct-cli` — the `construct` binary: argument parsing, output, completions.

Test a single one with `cargo test -p construct-core`. New behavior should come with
tests; integration tests live in each crate's `tests/` and build worlds in temp
directories, never against a real Minecraft installation.

Update `docs/cli-surface.md` when you change the command grammar, and verify in-game
anything only Minecraft can confirm (see `docs/manual-verification.md`).

## Commit Guidelines

1. Use clear commit messages that explain the intent.
2. Keep commits small and reviewable.
3. Don't mix formatting-only changes with behavioral ones.
4. Any style is acceptable (Conventional Commit or free-form).

## Pull Request Process

1. Ensure your branch is up to date with `main`.
2. Push your branch and open a Pull Request describing what changed, why, and how you
   validated it. Note any breaking changes.
3. Link related issues (for example: `Closes #123`).
4. Respond to review feedback with follow-up commits.

## PR Checklist

- [ ] `cargo fmt --all`, clippy, and `cargo test --workspace` all pass.
- [ ] New behavior is covered by tests.
- [ ] Dependency changes are patches in `third_party/patches/`.
- [ ] Docs (`README.md`, `docs/`, `--help` text) updated if behavior changed.

## Reporting Bugs and Proposing Features

Include the exact command you ran, expected vs actual behavior (with the exit code),
your OS and Minecraft installation, and any errors — `--json` output is usually the
most useful thing to paste.

## Community

For help or discussion, join the project Discord:
<https://discord.gg/9KGche8fxm>
