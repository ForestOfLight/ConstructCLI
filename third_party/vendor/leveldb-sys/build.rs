// Derived from bedrock-rs crates/level/build.rs (Apache-2.0). See NOTICE for the
// list of changes.
fn main() {
    println!("cargo:rerun-if-changed=ffi");

    let mut config = cmake::Config::new("ffi");
    // ffi.cpp uses C++14 features such as `std::make_unique`. GCC and MSVC happen
    // to accept it with their current defaults, but Apple clang on macOS 14 still
    // defaults low enough that the build falls back to pre-C++11 parsing unless the
    // standard is pinned explicitly.
    config
        .define("CMAKE_CXX_STANDARD", "14")
        .define("CMAKE_CXX_STANDARD_REQUIRED", "ON");
    // The vendored zlib calls lseek/read/write/close without including <unistd.h>.
    // Implicit function declarations are a hard error from C99 onward, and current
    // clang enforces it, so the vendored sources no longer compile without this.
    // cl.exe needs no such opt-out — it only warns (C4013) — and it rejects the
    // unrecognised spelling outright with D8021, so the flag has to stay off MSVC
    // targets. This reads the target rather than cfg!(), because it has to match
    // the compiler cmake picks for the target, not the one running the build.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        config.cflag("-Wno-error=implicit-function-declaration");
    }
    let mut ffi_dst = config.build().join("build");
    let mut leveldb_dst = ffi_dst.join("leveldb");

    if cfg!(target_env = "msvc") {
        let profile = config.get_profile();
        ffi_dst = ffi_dst.join(profile);
        leveldb_dst = leveldb_dst.join(profile);

        println!("cargo:rustc-link-lib=shell32");
    }

    println!(
        "Searching for leveldb-ffi and leveldb-mcpe in {}",
        ffi_dst.display()
    );
    println!("cargo:rustc-link-search=native={}", ffi_dst.display());
    println!("cargo:rustc-link-search=native={}", leveldb_dst.display());
    println!("cargo:rustc-link-lib=static=leveldb-ffi");
    println!("cargo:rustc-link-lib=static=leveldb-mcpe");

    // Apple's toolchain has not shipped libstdc++ for years; the C++ runtime there is
    // libc++. Linking stdc++ unconditionally on unix fails on every macOS target.
    #[cfg(all(unix, target_os = "macos"))]
    println!("cargo:rustc-link-lib=dylib=c++");
    #[cfg(all(unix, not(target_os = "macos")))]
    println!("cargo:rustc-link-lib=dylib=stdc++");
}
