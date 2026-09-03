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
        out.line(format!(
            "Beta APIs: {}",
            match current {
                Some(true) => "on",
                Some(false) => "off",
                None => "off (this world has no experiments)",
            }
        ));
        out.emit(Payload {
            world: world.qualified(),
            beta_apis: current.unwrap_or(false),
            changed: false,
            backup: None,
        });
        return Ok(());
    };

    // Back up before touching anything: a backup taken after a bad write
    // preserves the bad write.
    let backup = backup::file(&path, &world.qualified(), backups)?;
    let change = leveldat::apply_beta_apis(&path, on)?;

    if change.changed {
        out.line(format!(
            "Beta APIs: {} → {}",
            yes_no(change.before),
            yes_no(Some(change.after))
        ));
        out.line(format!("  backup: {}", backup.display()));
        out.line("Reload the world for the change to take effect.");
    } else {
        out.line(format!(
            "Beta APIs already {}; nothing to do",
            yes_no(Some(on))
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

fn yes_no(state: Option<bool>) -> &'static str {
    match state {
        Some(true) => "on",
        Some(false) => "off",
        None => "off (no experiments)",
    }
}
