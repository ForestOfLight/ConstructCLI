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
