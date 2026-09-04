//! Reading and flipping a world's Beta APIs toggle.
//!
//! Reading is what makes the flip verifiable without launching the game, and
//! it is what `status` (a later task) will report. A read must never touch
//! the file: `leveldat::read` alone, no backup, no write.

use crate::output::Out;
use construct_core::Result;
use construct_core::config::Backups;
use construct_core::discovery::World;
use construct_core::{backup, leveldat};
use serde::Serialize;

#[derive(Serialize)]
struct Payload {
    world: String,
    beta_apis: bool,
    changed: bool,
    backup: Option<String>,
}

pub fn run(world: &World, state: Option<bool>, backups: &Backups, out: &mut Out) -> Result<()> {
    let path = world.path.join("level.dat");

    let Some(on) = state else {
        // Read-only: no backup, no write. A read that modifies the world
        // would be the same class of bug this project already found and
        // fixed once in its leveldb layer.
        let dat = leveldat::read(&path)?;
        let current = dat.beta_apis();
        out.line(format!("Beta APIs: {}", describe(current)));
        out.emit(Payload {
            world: world.qualified(),
            beta_apis: current.unwrap_or(false),
            changed: false,
            backup: None,
        });
        return Ok(());
    };

    // Read first, without touching anything: a no-op flip (the requested
    // state already holds) must not take a backup at all, or ten no-op runs
    // would evict every genuine pre-flip backup at the default `keep`. This
    // is the same read `apply_beta_apis` would do internally; doing it here
    // first keeps the ordering guarantee below intact — a real flip is still
    // always preceded by its backup — while a non-flip takes none.
    if leveldat::read(&path)?.beta_apis() == Some(on) {
        out.line(format!(
            "Beta APIs already {}; nothing to do",
            describe(Some(on))
        ));
        out.emit(Payload {
            world: world.qualified(),
            beta_apis: on,
            changed: false,
            backup: None,
        });
        return Ok(());
    }

    // Back up before touching anything: a backup taken after a bad write
    // preserves the bad write.
    let backup = backup::file(&path, &world.qualified(), backups)?;
    let change = leveldat::apply_beta_apis(&path, on).inspect_err(|_| {
        // `apply_beta_apis` failing here means either the write itself
        // failed, or it succeeded but read back wrong (§ the report arm in
        // main.rs distinguishes the two). Either way the backup already
        // exists on disk; main.rs has no path to it, so print it here or
        // the "restore from backup" advice is one the user cannot act on.
        eprintln!("backup taken before the attempt: {}", backup.display());
    })?;

    if change.changed {
        out.line(format!(
            "Beta APIs: {} → {}",
            describe(change.before),
            describe(Some(change.after))
        ));
        out.line(format!("  backup: {}", backup.display()));
        out.line("Reload the world for the change to take effect.");
    } else {
        out.line(format!(
            "Beta APIs already {}; nothing to do",
            describe(Some(on))
        ));
    }
    out.emit(Payload {
        world: world.qualified(),
        beta_apis: change.after,
        changed: change.changed,
        backup: Some(backup.display().to_string()),
    });
    Ok(())
}

/// Renders a Beta APIs state the same way everywhere it is shown, so the
/// no-`experiments`-compound case cannot drift into different wording in
/// different call sites.
fn describe(state: Option<bool>) -> &'static str {
    match state {
        Some(true) => "on",
        Some(false) => "off",
        None => "off (this world has no experiments)",
    }
}
