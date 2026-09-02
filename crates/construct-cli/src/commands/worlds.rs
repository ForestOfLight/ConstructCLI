use crate::output::Out;
use construct_core::discovery::World;
use construct_core::{Result, discovery};
use serde::Serialize;

#[derive(Serialize)]
struct Payload<'a> {
    worlds: Vec<Row<'a>>,
}

#[derive(Serialize)]
struct Row<'a> {
    installation: &'a str,
    account: Option<&'a str>,
    folder: &'a str,
    display_name: &'a str,
    qualified: String,
    path: String,
    size_bytes: u64,
    last_played: Option<i64>,
    /// "level.dat" or "dir-mtime" — the two disagree often enough to matter.
    last_played_source: &'static str,
}

pub fn run(worlds: &[World], out: &Out) -> Result<()> {
    if !out.is_json() {
        out.line(format!(
            "{:<28} {:<14} {:>8}  {}",
            "NAME", "INSTALLATION", "SIZE", "REFERENCE"
        ));
        for w in worlds {
            out.line(format!(
                "{:<28} {:<14} {:>8}  {}",
                truncate(&w.display_name, 28),
                w.installation,
                human_size(w.size_bytes),
                w.qualified()
            ));
        }
        if worlds.is_empty() {
            out.line("no worlds found");
        }
    }

    out.emit(Payload {
        worlds: worlds
            .iter()
            .map(|w| Row {
                installation: &w.installation,
                account: w.account.as_deref(),
                folder: &w.folder,
                display_name: &w.display_name,
                qualified: w.qualified(),
                path: w.path.display().to_string(),
                size_bytes: w.size_bytes,
                last_played: w.last_played,
                last_played_source: match w.last_played_source {
                    discovery::LastPlayedSource::LevelDat => "level.dat",
                    discovery::LastPlayedSource::DirMtime => "dir-mtime",
                },
            })
            .collect(),
    });
    Ok(())
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[unit])
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max - 1).collect::<String>() + "…"
    }
}
