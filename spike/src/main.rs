use bedrock_level::db::Database;
use std::path::{Path, PathBuf};

const PREFIX: &[u8] = b"structuretemplate_";

/// Recursively copy a directory. The spike must never touch the real world.
fn copy_dir(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

fn main() {
    let world = PathBuf::from(std::env::args().nth(1).expect("usage: spike <world-dir>"));
    assert!(world.join("level.dat").is_file(), "not a world: {}", world.display());

    // 1. Copy the world so the original is never opened.
    let tmp = std::env::temp_dir().join("construct-spike");
    let _ = std::fs::remove_dir_all(&tmp);
    copy_dir(&world, &tmp).expect("copy failed");
    let db_path = tmp.join("db");
    println!("copied to {}", db_path.display());

    // 2. Open and enumerate.
    let db = Database::open(db_path.to_str().expect("non-UTF-8 path")).expect("open failed");
    let mut found: Vec<(Vec<u8>, usize)> = Vec::new();
    let mut total = 0usize;
    {
        let mut keys = db.keys();
        for kv in &mut keys {
            total += 1;
            let k = kv.key().to_vec();
            if k.starts_with(PREFIX) {
                found.push((k, kv.value().len()));
            }
        }
    }
    println!("FINDING total keys: {total}");
    println!("FINDING structuretemplate_ keys: {}", found.len());
    for (k, len) in &found {
        println!("  {} ({len} bytes)", String::from_utf8_lossy(k));
    }
    assert!(!found.is_empty(), "no structures in this world — wrong fixture");

    // 3. Round-trip: read bytes, write under a new key, read back, compare.
    let (src_key, _) = &found[0];
    let original: Vec<u8> = db.get(src_key).expect("get failed").expect("key vanished").to_vec();
    std::fs::write(tmp.join("spike-out.mcstructure"), &original).expect("write file failed");

    let dst_key = b"structuretemplate_mystructure:spike_roundtrip".to_vec();
    db.insert(&dst_key, &original).expect("insert failed");
    let read_back: Vec<u8> = db.get(&dst_key).expect("get failed").expect("insert did not stick").to_vec();
    println!("FINDING round-trip identical: {}", read_back == original);
    assert_eq!(read_back, original, "bytes changed across write/read");

    // 4. Does the value look like a .mcstructure? Little-endian NBT starts with 0x0A.
    println!("FINDING first byte 0x{:02x} (0x0a = TAG_Compound)", original[0]);

    // 5. What does a second open of the same database do?
    drop(db);
    let held = Database::open(db_path.to_str().unwrap()).expect("reopen after drop failed");
    match Database::open(db_path.to_str().unwrap()) {
        Ok(_) => println!("FINDING second concurrent open: SUCCEEDED (no lock error in-process)"),
        Err(e) => println!("FINDING second concurrent open: {e:?}"),
    }
    drop(held);

    // 6. Probe the LIVE (uncopied) world's db directly, read-only intent.
    // This is a read of the original path, which is acceptable only for this
    // lock probe per Task 1 brief note 4. We never write through this handle.
    let live_db_path = world.join("db");
    match Database::open(live_db_path.to_str().expect("non-UTF-8 path")) {
        Ok(_db) => println!("FINDING live db open: SUCCEEDED (world not currently locked by another process)"),
        Err(e) => println!("FINDING live db open: {e:?}"),
    }

    println!("\nspike OK");
}
