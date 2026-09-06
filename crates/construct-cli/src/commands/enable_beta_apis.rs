//! Turning a world's Beta APIs toggle on.
//!
//! Only on: there is no read and no off. Turning the experiment back off is
//! Minecraft's own Experiments screen, and the state a read would report is
//! the state this command prints anyway when there is nothing to do.

use crate::output::Out;
use construct_core::Result;
use construct_core::config::Backups;
use construct_core::discovery::World;
use construct_core::{backup, inuse, leveldat};
use serde::Serialize;

#[derive(Serialize)]
struct Payload {
    world: String,
    beta_apis: bool,
    changed: bool,
    backup: Option<String>,
}

pub fn run(world: &World, backups: &Backups, out: &mut Out) -> Result<()> {
    let path = world.path.join("level.dat");

    // Refuse before reading, let alone writing: Minecraft holds level.dat in
    // memory for the whole session and rewrites it from memory on every save,
    // so a write under a live world is discarded and the read that would
    // decide "already on; nothing to do" is answering about a file the game
    // is about to overwrite anyway. Neither answer is trustworthy while the
    // world is open, so the command stops here rather than reporting a result
    // that will not survive.
    crate::commands::refuse_if_in_use(world, inuse::AtRisk::LevelDat, out)?;

    // Read first, without touching anything: a no-op (Beta APIs already on)
    // must not take a backup at all, or ten no-op runs would evict every
    // genuine pre-flip backup at the default `keep`. This is the same read
    // `apply_beta_apis` would do internally; doing it here first keeps the
    // ordering guarantee below intact — a real flip is still always preceded
    // by its backup — while a non-flip takes none.
    if leveldat::read(&path)?.beta_apis() == Some(true) {
        out.line("Beta APIs already on; nothing to do".to_string());
        out.emit(Payload {
            world: world.qualified(),
            beta_apis: true,
            changed: false,
            backup: None,
        });
        return Ok(());
    }

    // Back up before touching anything: a backup taken after a bad write
    // preserves the bad write.
    let backup = backup::file(&path, &world.qualified(), backups)?;
    let change = leveldat::apply_beta_apis(&path, true).inspect_err(|_| {
        // `apply_beta_apis` failing here means either the write itself
        // failed, or it succeeded but read back wrong (§ the report arm in
        // main.rs distinguishes the two). Either way the backup already
        // exists on disk; main.rs has no path to it, so print it here or
        // the "restore from backup" advice is one the user cannot act on.
        eprintln!("backup taken before the attempt: {}", backup.display());
    })?;

    if change.changed {
        out.line(format!("Beta APIs: {} → on", describe(change.before)));
        out.line(format!("  backup: {}", backup.display()));
    } else {
        out.line("Beta APIs already on; nothing to do".to_string());
    }
    out.emit(Payload {
        world: world.qualified(),
        beta_apis: change.after,
        changed: change.changed,
        backup: Some(backup.display().to_string()),
    });
    Ok(())
}

/// Renders the state a world was in before the flip. Only the "before" side
/// needs this: the "after" side of a flip that happened is always `on`.
fn describe(state: Option<bool>) -> &'static str {
    match state {
        Some(true) => "on",
        Some(false) => "off",
        None => "off (this world has no experiments)",
    }
}
