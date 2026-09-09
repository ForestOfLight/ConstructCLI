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

# leveldb-sys upstream declares no licence of its own. Its Rust wrapper and C++
# shim were moved out of bedrock-rs (Apache-2.0) without that repository's
# LICENSE following them, so scripts/setup-deps.sh overlays our own Apache-2.0
# copies from third_party/vendor/leveldb-sys onto the clone.
#
# Release archives statically link that wrapper, so what this gate has to prove
# is that the overlay actually happened. A checkout made before the wrapper was
# vendored is still on disk, still builds, and still passes every test while
# linking the unlicensed upstream files — nothing but this check would notice.
vendor_root="$ROOT/third_party/vendor/leveldb-sys"
sys_root="$ROOT/third_party/checkouts/leveldb-sys"

if [ ! -d "$sys_root" ]; then
  echo "error: $sys_root is missing — run ./scripts/setup-deps.sh first" >&2
  exit 1
fi

for required in LICENSE NOTICE; do
  if [ ! -f "$vendor_root/$required" ]; then
    echo "error: third_party/vendor/leveldb-sys/$required is missing." >&2
    echo "It is what grants us the terms to ship the wrapper; do not delete it." >&2
    exit 1
  fi
done

stale=""
while IFS= read -r vendored; do
  relative="${vendored#"$vendor_root/"}"
  if ! cmp -s "$vendored" "$sys_root/$relative"; then
    stale="$stale  $relative"$'\n'
  fi
done < <(find "$vendor_root" -type f)

if [ -n "$stale" ]; then
  {
    echo "error: the leveldb-sys checkout does not carry our licensed wrapper."
    echo
    echo "These files differ from third_party/vendor/leveldb-sys, or are absent:"
    printf '%s' "$stale"
    echo
    echo "Upstream ships them without any licence, and a release archive links"
    echo "them. Run ./scripts/setup-deps.sh to overlay our Apache-2.0 copies."
    echo
    echo "See third_party/vendor/leveldb-sys/README.md."
  } >&2
  exit 1
fi

echo "ok: leveldb-sys carries our Apache-2.0 wrapper"
