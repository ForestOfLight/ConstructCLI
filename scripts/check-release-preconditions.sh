#!/usr/bin/env bash
# Decide whether a release tag is safe to publish.
#
# One gate: the tag agrees with workspace.package.version. Cargo has no release
# command and nothing otherwise checks this, so a mistyped tag would produce
# archives whose filenames disagree with the release they hang under.
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
