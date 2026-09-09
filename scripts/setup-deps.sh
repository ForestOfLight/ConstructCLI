#!/usr/bin/env bash
# Recreate the patched database dependencies in third_party/checkouts/.
#
# The upstream crates do not compile on macOS, or on any non-x86_64 target. Rather
# than vendor ~5 MB of C++ into this repo, we clone upstream at a pinned commit and
# apply the patches in third_party/patches/. Run this once after cloning.
#
# leveldb-sys additionally gets its Rust wrapper replaced wholesale — see
# overlay_vendored_wrapper below.
#
# When the patched branches are published as forks, this script goes away and the
# workspace's Cargo.toml points at the fork URLs instead.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/third_party/checkouts"
mkdir -p "$OUT"

clone_and_patch() {
  # Takes one or more patches, applied in the order given, so a dependency
  # patched more than once (e.g. nbtx: 0003 then 0004) gets all of them from a
  # fresh clone.
  local name="$1" url="$2" rev="$3"
  shift 3
  if [ -d "$OUT/$name" ]; then
    echo "$name: already present, skipping (delete it to re-create)"
    return
  fi
  echo "$name: cloning $url @ $rev"
  git clone --quiet "$url" "$OUT/$name"
  git -C "$OUT/$name" checkout --quiet "$rev"
  for patch in "$@"; do
    git -C "$OUT/$name" apply "$ROOT/third_party/patches/$patch"
    echo "$name: patched with $patch"
  done
}

clone_and_patch leveldb-sys \
  https://github.com/bedrock-crustaceans/leveldb-sys \
  cb3be905852781fd0911a3af91bf0328da2529af \
  0001-leveldb-sys-macos-and-arm-portability.patch

clone_and_patch bedrock-rs \
  https://github.com/bedrock-crustaceans/bedrock-rs \
  2d9e4087a207bdcbad6e4cdc83de46e94712b2f4 \
  0002-bedrock-rs-non-x86_64-compilation.patch

clone_and_patch nbtx \
  https://github.com/bedrock-crustaceans/bedrockrs-nbt \
  bd28e77 \
  0003-nbtx-empty-list-serialization.patch \
  0004-nbtx-recursion-depth-limit.patch

# The upstream leveldb-sys repository declares no licence of its own. That is an
# accident: bedrock-rs commit d4946739 moved the Rust wrapper and C++ shim out of
# a repository licensed Apache-2.0 without carrying the LICENSE file across.
# third_party/vendor/leveldb-sys/ holds our own Apache-2.0 copies of those files,
# taken from bedrock-rs at the commit before the move; copying them over the
# clone means nothing we compile and ship traces to an unlicensed file. Only
# ffi/leveldb/ is used as cloned, and that carries Google's BSD-3-Clause plus
# zlib's own notice.
#
# This runs unconditionally rather than inside clone_and_patch: a checkout made
# before the wrapper was vendored is already on disk, and clone_and_patch would
# skip it and leave the unlicensed files in place.
overlay_vendored_wrapper() {
  local vendor="$ROOT/third_party/vendor/leveldb-sys"
  local dest="$OUT/leveldb-sys"
  if [ ! -d "$vendor" ]; then
    echo "error: $vendor is missing" >&2
    exit 1
  fi
  cp -R "$vendor/." "$dest/"
  echo "leveldb-sys: overlaid the Apache-2.0 wrapper from third_party/vendor/leveldb-sys"
}

overlay_vendored_wrapper

echo "done — dependencies ready in third_party/checkouts/"
