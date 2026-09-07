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
# license field in its Cargo.toml. What licence text the checkout does carry
# belongs to the vendored C/C++ it bundles — Google's BSD-3-Clause under
# ffi/leveldb/, and zlib's own notice inside ffi/leveldb/lib/zlib/README (both
# attributed in about.toml). Neither says anything about the Rust wrapper in
# build.rs and src/, and release archives link that wrapper.
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
