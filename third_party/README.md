# Patched database dependencies

`bedrock_level` (from `bedrock-rs`) and its `leveldb-sys` backend do not compile as
published:

- `leveldb-sys/build.rs` links `stdc++` unconditionally on unix. Apple ships `libc++`,
  so every macOS target fails at the link step.
- `bedrock-rs/crates/level/src/greedy.rs` uses `is_x86_feature_detected!`,
  `#[target_feature(enable = "avx2")]`, and `std::arch::x86_64` with no architecture
  gate. This is a *compile* failure on any non-x86_64 target, not a runtime one.
- The vendored `port_posix_sse.cc` defines `LEVELDB_PLATFORM_POSIX_SSE` unconditionally
  and then reaches x86 intrinsics under an `#elif defined(__GNUC__)` that Apple clang
  satisfies on arm64.
- The vendored zlib's `zutil.h` treats `TARGET_OS_MAC` as classic Mac OS and redefines
  `fdopen()` to `NULL`, clobbering the modern SDK's declaration.

`patches/` holds the fixes. `scripts/setup-deps.sh` clones each upstream repo at its
pinned commit and applies them into `checkouts/`, which is git-ignored.

Both upstreams are Apache-2.0, which permits this. The fixes are intended to go upstream;
when they land, or when the patched branches are published as forks, this directory is
deleted and the workspace depends on a URL again.

Run `scripts/setup-deps.sh` once after cloning.
