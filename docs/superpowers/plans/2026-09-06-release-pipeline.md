# Release Publishing Pipeline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship prebuilt `construct` binaries from a git tag, so installing ConstructCLI requires downloading one file instead of a Rust toolchain, CMake, a C++ compiler, and a dependency-patching script.

**Architecture:** A single tag-triggered GitHub Actions workflow in three stages — a cheap `verify` gate, a three-platform `build` matrix, and a `publish` job that creates the Release. Logic that can be tested locally lives in `scripts/` rather than in YAML; the workflow only sequences it.

**Tech Stack:** GitHub Actions, bash, Python 3.11+ `tomllib` (present on all runners), `cargo-about`, `gh` CLI.

**Spec:** `docs/superpowers/specs/2026-09-06-release-pipeline-design.md`

## Global Constraints

- Target triples, exactly three: `x86_64-unknown-linux-gnu` (on `ubuntu-22.04`), `aarch64-apple-darwin` (on `macos-14`), `x86_64-pc-windows-msvc` (on `windows-latest`). Intel macOS is explicitly out of scope.
- `ubuntu-22.04` is deliberate, not incidental: it sets the Linux glibc floor at 2.35. Do not "modernise" it to `ubuntu-latest`.
- No cross-compilation. leveldb is real C++ built through CMake; every target needs a native runner.
- Archive naming: `construct-<version>-<target>.<ext>` where `<version>` is the tag with a leading `v` stripped, `<ext>` is `zip` for Windows and `tar.gz` elsewhere.
- Every archive contains one top-level directory of the same name holding: the binary, `LICENSE`, `README.md`, `THIRD-PARTY-NOTICES.md`.
- No shell completion files are shipped. ConstructCLI's completions are dynamic — `construct completions bash` emits a stub that calls the binary at runtime.
- Scripts are `#!/usr/bin/env bash` with `set -euo pipefail`, and must run under git-bash on Windows runners (as `scripts/setup-deps.sh` already does).
- No script invokes `python3` by name. git-bash on the Windows runner may expose the interpreter only as `python`, and `tomllib` needs 3.11+, so every script that needs Python probes `python3` then `python` for one that can `import tomllib`, and fails with a diagnostic if neither can.
- Version is read from `workspace.package.version` in the root `Cargo.toml`.

## Two corrections to the spec

Both were found while checking the spec's claims against the actual repository, and both have already been applied to the spec. They are recorded here because the reasoning is worth carrying into implementation.

1. **`cargo package --workspace --no-verify` is dropped from the `verify` job.** The spec says it "is expected to succeed today." It does not. Verified by running it:

   ```
   error: failed to verify manifest at crates/construct-core/Cargo.toml
   Caused by:
     all dependencies must have a version requirement specified when packaging.
     dependency `bedrock_level` does not specify a version
   ```

   Both workspace members carry versionless `path` dependencies, so this cannot pass until the crates.io blocker chain is resolved. Including it would break every release. The spec's crates.io readiness section already documents the blockers, which is where that concern belongs.

2. **Version is read with Python's `tomllib`, not `cargo metadata`.** `cargo metadata` must resolve the `third_party/checkouts/` path dependencies, so it only works after `scripts/setup-deps.sh` has cloned three repositories. Reading `Cargo.toml` directly keeps the version check free of that dependency. This serves the spec's stated intent — fail fast before spending three platform builds — better than the mechanism it named.

## File Structure

| File | Status | Responsibility |
|---|---|---|
| `Cargo.toml` | modify | Add `[profile.release] strip = true` |
| `scripts/check-release-preconditions.sh` | create | Answers one question: is this tag safe to publish? Tag/version agreement, and the leveldb-sys licence gate. |
| `about.toml` | create | `cargo-about` configuration: accepted licences, and clarification entries for the six crates that come from patched local checkouts. |
| `about.hbs` | create | Handlebars template rendering `THIRD-PARTY-NOTICES.md`. |
| `scripts/package-release.sh` | create | Assembles and archives one platform's release artifact. |
| `.github/workflows/release.yml` | create | Sequences the above. Contains no logic of its own. |
| `.gitignore` | modify | Ignore `/dist` and the generated `THIRD-PARTY-NOTICES.md` |
| `README.md` | modify | Install section: download table, checksum verification, PATH setup; build-from-source demoted to a subsection |
| `.github/workflows/ci.yml` | unchanged | — |

---

### Task 1: Release profile and the precondition gate

Two gates must pass before binaries go out: the tag has to agree with `Cargo.toml`, and every patched dependency has to carry a licence. Both live in one script because they answer the same question — is this tag releasable — and a reviewer would accept or reject them together.

**Files:**
- Modify: `Cargo.toml` (append a `[profile.release]` section)
- Create: `scripts/check-release-preconditions.sh`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `scripts/check-release-preconditions.sh <tag>` — exit `0` if releasable, `1` if a gate fails (with a diagnostic on stderr), `2` on wrong argument count. Task 4's `verify` job calls it.

- [ ] **Step 1: Add the release profile to `Cargo.toml`**

Append to the root `Cargo.toml`, after the `[workspace.dependencies]` section and before the `[patch...]` sections:

```toml
[profile.release]
# Strip symbols in Cargo rather than with a `strip` step in the release
# workflow. `strip` is absent from the Windows MSVC toolchain and behaves
# differently between GNU and Apple binutils; doing it here gives one
# behaviour on all three runners, and makes a local `cargo build --release`
# produce the same binary that ships.
strip = true
```

- [ ] **Step 2: Verify the profile applies**

Run:
```bash
cargo build --release -p construct-cli
file target/release/construct
```
Expected: the output ends with `stripped`, not `with debug_info, not stripped`.

- [ ] **Step 3: Write the failing test — run a script that does not exist yet**

Run:
```bash
./scripts/check-release-preconditions.sh v0.1.0
```
Expected: FAIL with `bash: ./scripts/check-release-preconditions.sh: No such file or directory`.

- [ ] **Step 4: Write the script**

Create `scripts/check-release-preconditions.sh`:

```bash
#!/usr/bin/env bash
# Decide whether a release tag is safe to publish.
#
# Two gates:
#
#   1. The tag agrees with workspace.package.version. Cargo has no release
#      command and nothing otherwise checks this, so a mistyped tag would
#      produce archives whose filenames disagree with the release they hang
#      under.
#   2. Every patched dependency carries a licence. The release archives
#      statically link this code, and redistributing it needs terms. See the
#      precondition section of
#      docs/superpowers/specs/2026-09-06-release-pipeline-design.md.
#
# The version is read straight from Cargo.toml rather than through
# `cargo metadata`, which would first need scripts/setup-deps.sh to clone three
# upstream repositories. This gate is meant to be the cheap one.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if [ $# -ne 1 ]; then
  echo "usage: $0 <tag>   (for example: v0.1.0)" >&2
  exit 2
fi

tag="$1"
tag_version="${tag#v}"

# Resolve a Python 3.11+ interpreter. git-bash on the Windows runner may expose
# it as `python` rather than `python3`, and tomllib arrived in 3.11 — so probe
# for a name that both exists and can import it, rather than assuming either.
python_bin=""
for candidate in python3 python; do
  if command -v "$candidate" > /dev/null 2>&1 \
    && "$candidate" -c 'import tomllib' > /dev/null 2>&1; then
    python_bin="$candidate"
    break
  fi
done
if [ -z "$python_bin" ]; then
  echo "error: no Python 3.11+ with tomllib on PATH (tried: python3, python)" >&2
  exit 1
fi

manifest_version="$("$python_bin" -c "
import tomllib
with open('$ROOT/Cargo.toml', 'rb') as f:
    print(tomllib.load(f)['workspace']['package']['version'])
")"

if [ "$tag_version" != "$manifest_version" ]; then
  {
    echo "error: tag '$tag' does not match workspace.package.version"
    echo "  the tag says:    $tag_version"
    echo "  Cargo.toml says: $manifest_version"
    echo "Bump the version in Cargo.toml, or delete the tag and retag."
  } >&2
  exit 1
fi
echo "ok: tag $tag matches workspace.package.version $manifest_version"

# leveldb-sys declares no licence of its own: no LICENSE file at its root, no
# license field in its Cargo.toml. The only licence text in its checkout is
# Google's BSD-3-Clause for the vendored C++ under ffi/leveldb/, which says
# nothing about the Rust wrapper in build.rs and src/. Release archives link
# that wrapper.
sys_root="$ROOT/third_party/checkouts/leveldb-sys"
if [ ! -d "$sys_root" ]; then
  echo "error: $sys_root is missing — run ./scripts/setup-deps.sh first" >&2
  exit 1
fi

if compgen -G "$sys_root/LICENSE*" > /dev/null \
  || compgen -G "$sys_root/COPYING*" > /dev/null \
  || grep -sq '^license' "$sys_root/Cargo.toml"; then
  echo "ok: leveldb-sys carries a licence"
else
  cat >&2 <<'GATE'
error: leveldb-sys still declares no licence of its own.

Release archives statically link its Rust wrapper (build.rs and src/), and
redistributing that needs terms. Google's BSD-3-Clause under ffi/leveldb/
covers the vendored C++ only.

Resolve one of these before tagging a release:
  1. Ask bedrock-crustaceans/leveldb-sys to add a licence file, then bump the
     pinned commit in scripts/setup-deps.sh.
  2. Write our own bindings over the vendored BSD-3-Clause C++.
  3. Move to the rusty-leveldb backend.

See docs/superpowers/specs/2026-09-06-release-pipeline-design.md.
GATE
  exit 1
fi
```

Then:
```bash
chmod +x scripts/check-release-preconditions.sh
```

- [ ] **Step 5: Test the version-mismatch gate**

Run:
```bash
./scripts/check-release-preconditions.sh v9.9.9; echo "exit=$?"
```
Expected: `exit=1`, and stderr naming both `9.9.9` and `0.1.0`.

- [ ] **Step 6: Test the licence gate**

Run:
```bash
./scripts/check-release-preconditions.sh v0.1.0; echo "exit=$?"
```
Expected: `exit=1`. stdout shows `ok: tag v0.1.0 matches workspace.package.version 0.1.0`; stderr shows the leveldb-sys licence message. **This failure is correct** — it is the spec's precondition firing, and it stays until the licence question is resolved in Task 6.

- [ ] **Step 7: Test the passing path**

Prove the gate clears once a licence exists:
```bash
touch third_party/checkouts/leveldb-sys/LICENSE
./scripts/check-release-preconditions.sh v0.1.0; echo "exit=$?"
rm third_party/checkouts/leveldb-sys/LICENSE
```
Expected: `exit=0` with both `ok:` lines. The `rm` restores the real state — do not leave that file behind, and note that `third_party/checkouts/` is git-ignored so it could never have been committed.

- [ ] **Step 8: Test the usage guard**

Run:
```bash
./scripts/check-release-preconditions.sh; echo "exit=$?"
```
Expected: `exit=2` and the usage line.

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml scripts/check-release-preconditions.sh
git commit -m "feat: add release precondition gate and strip release binaries"
```

---

### Task 2: Third-party notices

The binaries statically link Google's BSD-3-Clause leveldb, Apache-2.0 code from bedrock-rs and nbtx, and the wider Rust dependency tree. Every one of those licences requires attribution on redistribution of a compiled artifact, so this file is an obligation rather than polish.

`cargo-about` generates it from the real dependency graph, so it cannot drift from what is actually linked.

**Files:**
- Create: `about.toml`
- Create: `about.hbs`
- Modify: `.gitignore`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `cargo about generate about.hbs -o THIRD-PARTY-NOTICES.md` writes `THIRD-PARTY-NOTICES.md` at the repository root. Task 3's packaging script requires that file to exist.

- [ ] **Step 1: Install cargo-about locally**

```bash
cargo install --locked --version 0.8.1 cargo-about
```

Pinned to match the version the release workflow installs in Task 4, so that
locally generated notices match what ships. If 0.8.1 is unavailable or fails to
build, pick a current version, use it consistently in both places, and note the
change in the Task 4 workflow comment.

- [ ] **Step 2: Write the failing test — generate with no configuration**

Run:
```bash
cargo about generate about.hbs -o THIRD-PARTY-NOTICES.md
```
Expected: FAIL, because neither `about.toml` nor `about.hbs` exists.

- [ ] **Step 3: Write the template**

Create `about.hbs`:

````handlebars
# Third-party notices

ConstructCLI links the following third-party components. Each licence below is
reproduced in full, followed by the components distributed under it.

{{#each licenses}}
## {{name}}

Used by:
{{#each used_by}}
- {{crate.name}} {{crate.version}}{{#if crate.repository}} — {{crate.repository}}{{/if}}
{{/each}}

```
{{{text}}}
```

{{/each}}
````

- [ ] **Step 4: Write the configuration**

Create `about.toml`. The `clarify` entries exist because six crates in the tree come from patched local checkouts and four of them carry no `license` field even though their repository is licensed. Verified with `cargo metadata`:

```
bedrock_level          license=None
bedrock_macros         license=None
bedrock_protocol_core  license=None
bedrock_shared         license=None
leveldb-sys            license=None
nbtx                   license='Apache-2.0'
```

The four `bedrock_*` crates are covered by bedrock-rs's top-level Apache-2.0 `LICENSE`, two directories above each crate root. `leveldb-sys` is the genuine unknown, and its entry below covers only the vendored Google C++ — its own wrapper is gated by `scripts/check-release-preconditions.sh` from Task 1.

```toml
# Licences we are willing to ship. cargo-about fails on anything outside this
# list, which is the point: a new dependency under unexpected terms should stop
# a release rather than slip into the notices file unnoticed.
accepted = [
    "Apache-2.0",
    "MIT",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Zlib",
    "Unicode-3.0",
    "MPL-2.0",
]

# The bedrock-rs workspace licenses its crates at the repository root rather
# than per-crate, so none of its members declare a license field. See
# third_party/checkouts/bedrock-rs/LICENSE.
[bedrock_level.clarify]
license = "Apache-2.0"
[[bedrock_level.clarify.files]]
path = "../../LICENSE"
checksum = "c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4"

[bedrock_shared.clarify]
license = "Apache-2.0"
[[bedrock_shared.clarify.files]]
path = "../../LICENSE"
checksum = "c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4"

[bedrock_macros.clarify]
license = "Apache-2.0"
[[bedrock_macros.clarify.files]]
path = "../../LICENSE"
checksum = "c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4"

[bedrock_protocol_core.clarify]
license = "Apache-2.0"
[[bedrock_protocol_core.clarify.files]]
path = "../../LICENSE"
checksum = "c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4"

# leveldb-sys declares no licence of its own. This entry attributes the
# vendored C++ it actually ships — Google's leveldb, BSD-3-Clause — which is
# what dominates the compiled artifact. It deliberately does NOT settle the
# terms of leveldb-sys's own Rust wrapper in build.rs and src/; that question
# is enforced separately by scripts/check-release-preconditions.sh, which
# blocks any release tag until it is answered.
[leveldb-sys.clarify]
license = "BSD-3-Clause"
[[leveldb-sys.clarify.files]]
path = "ffi/leveldb/LICENSE"
checksum = "ccc19f1da0798ed666609b65a5b44dd8b3abe6fc08b9c0592eb76e82e174db19"
```

- [ ] **Step 5: Generate and iterate**

Run:
```bash
cargo about generate about.hbs -o THIRD-PARTY-NOTICES.md
```

If it reports a crate with an unrecognised or missing licence that is not already listed above, add it. For a missing licence, find the licence file in that crate's checkout, compute its checksum with `sha256sum <path>`, and add a `clarify` block in the same shape as those above — `path` is relative to the crate's own root directory. For a licence that is present but not accepted, add the SPDX identifier to `accepted` only if shipping under those terms is genuinely acceptable.

Repeat until the command exits `0`.

- [ ] **Step 6: Verify the output**

Run:
```bash
grep -c "^## " THIRD-PARTY-NOTICES.md
grep -n "LevelDB Authors" THIRD-PARTY-NOTICES.md | head -1
grep -n "Apache License" THIRD-PARTY-NOTICES.md | head -1
grep -n "^- nbtx" THIRD-PARTY-NOTICES.md | head -1
```
Expected: several licence sections; Google's leveldb copyright present; the Apache licence text present; `nbtx` listed among its users.

- [ ] **Step 7: Ignore the generated file**

Append to `.gitignore`:

```
# Generated at release time by `cargo about generate` (see about.toml)
/THIRD-PARTY-NOTICES.md
```

- [ ] **Step 8: Commit**

```bash
git add about.toml about.hbs .gitignore
git commit -m "feat: generate third-party licence notices with cargo-about"
```

---

### Task 3: Release packaging script

**Files:**
- Create: `scripts/package-release.sh`
- Modify: `.gitignore`

**Interfaces:**
- Consumes: `target/release/construct` (or `construct.exe`) from `cargo build --release -p construct-cli`, and `THIRD-PARTY-NOTICES.md` from Task 2.
- Produces: `scripts/package-release.sh <target-triple>` — writes `dist/construct-<version>-<target>.tar.gz` (or `.zip` for Windows targets) and removes its staging directory. Task 4's `build` job calls it once per matrix entry.

- [ ] **Step 1: Write the failing test — run a script that does not exist yet**

Run:
```bash
./scripts/package-release.sh x86_64-unknown-linux-gnu
```
Expected: FAIL with `No such file or directory`.

- [ ] **Step 2: Write the script**

Create `scripts/package-release.sh`:

```bash
#!/usr/bin/env bash
# Assemble one platform's release archive.
#
# Usage: scripts/package-release.sh <target-triple>
#
# Expects `cargo build --release -p construct-cli` and `cargo about generate`
# to have run already. Produces dist/construct-<version>-<target>.<ext>
# containing a single top-level directory of the same name, so that extracting
# it never scatters files into the user's current directory.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [ $# -ne 1 ]; then
  echo "usage: $0 <target-triple>" >&2
  exit 2
fi
target="$1"

# Resolve a Python 3.11+ interpreter. git-bash on the Windows runner may expose
# it as `python` rather than `python3`, and tomllib arrived in 3.11 — so probe
# for a name that both exists and can import it, rather than assuming either.
python_bin=""
for candidate in python3 python; do
  if command -v "$candidate" > /dev/null 2>&1 \
    && "$candidate" -c 'import tomllib' > /dev/null 2>&1; then
    python_bin="$candidate"
    break
  fi
done
if [ -z "$python_bin" ]; then
  echo "error: no Python 3.11+ with tomllib on PATH (tried: python3, python)" >&2
  exit 1
fi

version="$("$python_bin" -c "
import tomllib
with open('Cargo.toml', 'rb') as f:
    print(tomllib.load(f)['workspace']['package']['version'])
")"

name="construct-${version}-${target}"
staging="dist/${name}"

case "$target" in
  *windows*) bin="target/release/construct.exe" ;;
  *)         bin="target/release/construct" ;;
esac

if [ ! -f "$bin" ]; then
  echo "error: $bin not found — run 'cargo build --release -p construct-cli' first" >&2
  exit 1
fi

if [ ! -f THIRD-PARTY-NOTICES.md ]; then
  echo "error: THIRD-PARTY-NOTICES.md not found — run 'cargo about generate about.hbs -o THIRD-PARTY-NOTICES.md' first" >&2
  exit 1
fi

rm -rf "$staging"
mkdir -p "$staging"
cp "$bin" "$staging/"
cp LICENSE README.md THIRD-PARTY-NOTICES.md "$staging/"

cd dist
case "$target" in
  *windows*)
    # 7z rather than `zip`, which is not present in git-bash on the Windows
    # runners. 7z is preinstalled on all GitHub-hosted images.
    7z a -tzip "${name}.zip" "$name" > /dev/null
    echo "created dist/${name}.zip"
    ;;
  *)
    tar czf "${name}.tar.gz" "$name"
    echo "created dist/${name}.tar.gz"
    ;;
esac
rm -rf "$name"
```

Then:
```bash
chmod +x scripts/package-release.sh
```

- [ ] **Step 3: Test the missing-prerequisite guards**

Run:
```bash
mv THIRD-PARTY-NOTICES.md /tmp/notices-backup.md
./scripts/package-release.sh x86_64-unknown-linux-gnu; echo "exit=$?"
mv /tmp/notices-backup.md THIRD-PARTY-NOTICES.md
```
Expected: `exit=1` naming `THIRD-PARTY-NOTICES.md` and the `cargo about` command to run.

- [ ] **Step 4: Test the happy path**

Run:
```bash
cargo build --release -p construct-cli
./scripts/package-release.sh x86_64-unknown-linux-gnu
ls dist/
tar tzf dist/construct-0.1.0-x86_64-unknown-linux-gnu.tar.gz
```
Expected: `dist/` contains exactly the `.tar.gz` and no leftover staging directory. The listing shows exactly five entries — the directory `construct-0.1.0-x86_64-unknown-linux-gnu/` and, inside it, `construct`, `LICENSE`, `README.md`, `THIRD-PARTY-NOTICES.md`.

- [ ] **Step 5: Confirm the extracted binary runs**

Run, from the repository root:
```bash
archive="$PWD/dist/construct-0.1.0-x86_64-unknown-linux-gnu.tar.gz"
rm -rf /tmp/pkgtest && mkdir -p /tmp/pkgtest
tar xzf "$archive" -C /tmp/pkgtest
/tmp/pkgtest/construct-0.1.0-x86_64-unknown-linux-gnu/construct --version
rm -rf /tmp/pkgtest
```
Expected: prints the version. This extracts the archive somewhere with none of the build tree around it, which is what confirms the layout is what a user actually gets rather than what happens to work in place.

- [ ] **Step 6: Test the usage guard**

Run:
```bash
./scripts/package-release.sh; echo "exit=$?"
```
Expected: `exit=2` and the usage line.

- [ ] **Step 7: Ignore the output directory**

Append to `.gitignore`:

```
# Release archives built by scripts/package-release.sh
/dist
```

- [ ] **Step 8: Commit**

```bash
git add scripts/package-release.sh .gitignore
git commit -m "feat: add release packaging script"
```

---

### Task 4: The release workflow

Sequences the previous three tasks. It contains no logic of its own — everything it does is a script that was already tested locally.

**Files:**
- Create: `.github/workflows/release.yml`

**Interfaces:**
- Consumes: `scripts/check-release-preconditions.sh` (Task 1), `about.hbs` and `about.toml` (Task 2), `scripts/package-release.sh` (Task 3), and the existing `scripts/setup-deps.sh`.
- Produces: a GitHub Release carrying three archives and a `SHA256SUMS` file. Task 5's README documents those exact filenames.

- [ ] **Step 1: Write the workflow**

Create `.github/workflows/release.yml`:

```yaml
name: Release

on:
  push:
    tags: ["v*"]
  # A manual run IS the dry run: it builds all three platforms and uploads the
  # archives as workflow artifacts, but never publishes, because `publish` below
  # is gated on the push trigger. There is deliberately no input to override
  # that — a control that cannot publish should not offer the choice.
  workflow_dispatch:

# Only the publish job needs write access; everything else stays read-only.
permissions:
  contents: read

jobs:
  # The cheap gate. Runs before three platform builds are spent on a bad tag.
  # On workflow_dispatch there is no tag to check, so the steps no-op and the
  # job still succeeds, which keeps `needs: verify` working for both triggers.
  verify:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Fetch patched dependencies
        if: github.event_name == 'push'
        shell: bash
        run: ./scripts/setup-deps.sh
      # TAG goes through env, not `${{ }}` inside the script body: expression
      # interpolation happens before bash parses the line, so a tag containing
      # a backtick or $( ) would execute. git allows both in a ref name.
      - name: Check release preconditions
        if: github.event_name == 'push'
        shell: bash
        env:
          TAG: ${{ github.ref_name }}
        run: ./scripts/check-release-preconditions.sh "$TAG"

  build:
    needs: verify
    strategy:
      fail-fast: false
      matrix:
        include:
          # ubuntu-22.04 is deliberate: it sets the glibc floor at 2.35, which
          # covers Ubuntu 22.04+ and Debian 12. Binaries built on newer images
          # fail on those systems with an unhelpful symbol error. When GitHub
          # retires this image the job fails at queue time on a tag push; the
          # successor is `ubuntu-latest` with `container: ubuntu:22.04`.
          - os: ubuntu-22.04
            target: x86_64-unknown-linux-gnu
          - os: macos-14
            target: aarch64-apple-darwin
          - os: windows-latest
            target: x86_64-pc-windows-msvc
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with:
          key: release-${{ matrix.target }}
      # CMake and a C++ compiler are preinstalled on all three runner images;
      # leveldb-sys vendors the leveldb source, so no submodule checkout is
      # needed. Same setup as ci.yml.
      - run: ./scripts/setup-deps.sh
        shell: bash
      # Pinned: cargo-about's output format and its strictness about missing
      # licence fields both move between versions, and an unpinned tool would
      # let a routine re-run of an old tag produce different notices. Bump this
      # deliberately, then re-run a dry run to see what changed.
      - name: Install cargo-about
        uses: taiki-e/install-action@v2
        with:
          tool: cargo-about@0.8.1
      # Release rather than debug: one compilation serves both the test run and
      # the shipped binary. The C++ leveldb build already dominates the wall
      # clock, so a separate debug pass would roughly double it.
      - run: cargo test --workspace --release
      - run: cargo build --release -p construct-cli
      # about.toml declares no `targets`, so cargo-about resolves the whole crate
      # graph rather than the host's slice, and all three runners emit the same
      # file. That is the property worth having: a licence that passes locally
      # cannot fail on one runner and strand the matrix mid-release.
      - name: Generate third-party notices
        run: cargo about generate about.hbs -o THIRD-PARTY-NOTICES.md
        shell: bash
      - name: Package
        run: ./scripts/package-release.sh ${{ matrix.target }}
        shell: bash
      - uses: actions/upload-artifact@v4
        with:
          name: construct-${{ matrix.target }}
          path: dist/*
          if-no-files-found: error

  publish:
    needs: build
    # Never publish from a manual dispatch. A dry run stops after `build`, so
    # the whole pipeline can be exercised without anything reaching the public.
    if: github.event_name == 'push'
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - uses: actions/download-artifact@v4
        with:
          path: artifacts
          merge-multiple: true
      - name: Generate checksums
        working-directory: artifacts
        run: |
          # Globbing by extension rather than `*`, so the redirect target
          # cannot end up checksumming itself.
          sha256sum *.tar.gz *.zip > SHA256SUMS
          # The matrix produces exactly three archives. Publishing two of them
          # is worse than publishing none, so refuse rather than ship a
          # release that silently omits a platform.
          found=$(grep -c . SHA256SUMS)
          if [ "$found" -ne 3 ]; then
            echo "error: expected 3 archives, found $found" >&2
            cat SHA256SUMS >&2
            exit 1
          fi
          cat SHA256SUMS
      - name: Create release
        env:
          GH_TOKEN: ${{ github.token }}
          # Same reasoning as the verify job: never interpolate a ref name
          # directly into a script body.
          TAG: ${{ github.ref_name }}
          REPO: ${{ github.repository }}
        run: |
          prerelease=""
          case "$TAG" in
            *-*) prerelease="--prerelease" ;;
          esac
          # $prerelease is deliberately unquoted: empty must expand to no
          # argument at all, not to an empty one.
          gh release create "$TAG" \
            --repo "$REPO" \
            --title "$TAG" \
            --generate-notes \
            $prerelease \
            artifacts/*
```

- [ ] **Step 2: Validate the YAML parses**

Run:
```bash
python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/release.yml')); print('valid yaml')"
```
Expected: `valid yaml`. If PyYAML is unavailable, install it in a throwaway venv or skip — the dry run in Task 6 is the real check.

- [ ] **Step 3: Confirm every referenced path exists**

Run:
```bash
for f in scripts/setup-deps.sh scripts/check-release-preconditions.sh scripts/package-release.sh about.hbs about.toml; do
  test -f "$f" && echo "ok $f" || echo "MISSING $f"
done
```
Expected: five `ok` lines.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "feat: add tag-triggered release workflow"
```

---

### Task 5: README install section

**Files:**
- Modify: `README.md` (lines 27-48, the `## Install` section)

**Interfaces:**
- Consumes: the archive filenames produced by Task 3 and published by Task 4.
- Produces: nothing later tasks depend on.

- [ ] **Step 1: Replace the Install section**

Replace `README.md` lines 27 through 48 — everything from `## Install` up to but not including `## Usage` — with:

````markdown
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

On Windows, extract the zip and move `construct.exe` into a directory that is
already on your `PATH`.

Intel Macs, 32-bit systems, and Linux distributions older than Debian 12 or
Ubuntu 22.04 have no published binary — build from source instead.

### Verifying a download

Every release includes a `SHA256SUMS` file. Download it alongside the archive
and check them against each other:

```
sha256sum --check --ignore-missing SHA256SUMS
```

On Windows PowerShell:

```
(Get-FileHash construct-<version>-x86_64-pc-windows-msvc.zip).Hash
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
````

- [ ] **Step 2: Check the section boundaries survived**

Run:
```bash
grep -n "^## \|^### " README.md | head -20
```
Expected: `## Install`, then `### Verifying a download`, `### Building from source`, then `## Usage` — in that order, with no duplicated or orphaned headings.

- [ ] **Step 3: Confirm the stale claim is gone**

Run:
```bash
grep -n "No binaries are published yet" README.md; echo "exit=$?"
```
Expected: `exit=1` (no match).

- [ ] **Step 4: Commit**

```bash
git add README.md
git commit -m "docs: document binary installation in README"
```

---

### Task 6: Dry run, licence resolution, and the first release

The pipeline cannot be tested locally: `act` does not help, because the matrix needs real macOS and Windows runners. This task is where it meets reality.

**Files:** none — this is verification and an external dependency.

**Interfaces:**
- Consumes: everything from Tasks 1 through 5.
- Produces: a published `v0.1.0` release.

- [ ] **Step 1: Push the branch and open the PR**

```bash
git push -u origin release-pipeline
```
Then open a PR against `main` per `CONTRIBUTING.md`, describing what changed, why, and how it was validated.

- [ ] **Step 2: Dry-run the workflow**

From the GitHub Actions tab, run the `Release` workflow manually against the `release-pipeline` branch. A manual run cannot publish: `publish` is gated on the push trigger, so it is skipped and the archives land as workflow artifacts instead.

Confirm:
- All three `build` matrix legs succeed.
- Each uploads exactly one archive.
- `publish` is skipped.

- [ ] **Step 3: Inspect the artifacts**

Download all three from the run summary and check each one:
```bash
tar tzf construct-0.1.0-x86_64-unknown-linux-gnu.tar.gz
tar tzf construct-0.1.0-aarch64-apple-darwin.tar.gz
unzip -l construct-0.1.0-x86_64-pc-windows-msvc.zip
```
Expected: each holds one top-level directory containing the binary, `LICENSE`, `README.md`, and `THIRD-PARTY-NOTICES.md`. Open one `THIRD-PARTY-NOTICES.md` and confirm it names Google's leveldb, the Apache-2.0 text, and nbtx.

- [ ] **Step 4: Fix anything the dry run found, then repeat**

Most likely failure points, in rough order of probability:
- `cargo about` flagging a platform-specific dependency that does not appear in the Linux tree (add it to `accepted` or clarify it in `about.toml`).
- `7z` not resolving in git-bash on the Windows runner (call it as `"/c/Program Files/7-Zip/7z.exe"` instead).
- `scripts/setup-deps.sh` behaving differently under the release workflow's shell than in `ci.yml`.

Re-run the dry run after each fix until it is clean.

- [ ] **Step 5: Resolve the leveldb-sys licence question**

**This blocks everything below it.** `scripts/check-release-preconditions.sh` will fail any tag push until it is answered, which is the design working as intended.

Take the cheapest route first: open an issue on `bedrock-crustaceans/leveldb-sys` asking them to add a licence file, noting that the repository currently has no `LICENSE` and no `license` field in `Cargo.toml`, and that this blocks redistribution of anything linking it. If they add one, bump the pinned commit in `scripts/setup-deps.sh` and re-run `./scripts/check-release-preconditions.sh v0.1.0` to confirm the gate clears.

If they do not respond, the remaining options are writing our own bindings over the vendored BSD-3-Clause C++, or moving to the `rusty-leveldb` backend. Both are separate projects; see the crates.io readiness section of the spec.

- [ ] **Step 6: Merge, tag, and release**

Once the PR is merged and the licence gate clears:
```bash
git checkout main
git pull
./scripts/check-release-preconditions.sh v0.1.0   # must exit 0
git tag v0.1.0
git push origin v0.1.0
```

- [ ] **Step 7: Verify the release**

Confirm on the releases page:
- Three archives plus `SHA256SUMS` are attached.
- Notes were generated from the commit history.
- It is *not* marked prerelease (the tag has no `-`).

Then verify a download end to end, as a user would:
```bash
cd /tmp && rm -rf relcheck && mkdir relcheck && cd relcheck
gh release download v0.1.0 --repo ForestOfLight/ConstructCLI
sha256sum --check --ignore-missing SHA256SUMS
tar xzf construct-0.1.0-x86_64-unknown-linux-gnu.tar.gz
./construct-0.1.0-x86_64-unknown-linux-gnu/construct --version
```
Expected: every checksum line reads `OK`, and the binary prints `0.1.0`.

- [ ] **Step 8: Record the manual verification**

Add to `docs/manual-verification.md`, under the existing list:

```markdown
- [ ] **Release archives run on a clean machine.** Download the published
      archive for each platform onto a machine that has never had the Rust
      toolchain, CMake, or a C++ compiler installed, and confirm `construct
      --version` and `construct worlds` both work. The build machine always has
      the toolchain present, so only a clean machine proves the binary is
      actually self-contained.
```

```bash
git add docs/manual-verification.md
git commit -m "docs: add release archive verification to manual checks"
```
