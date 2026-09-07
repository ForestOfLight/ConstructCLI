use crate::cli::ExportArgs;
use crate::context::Context;
use crate::failure::{self, Failure};
use crate::output::Out;
use crate::support::catalog as loader;
use crate::support::format::human_size;
use crate::support::usage::check_source_against_world;
use construct_core::Result;
use construct_core::catalog::{self, Entry, Source};
use construct_core::discovery::{Installation, World};
use construct_core::error::CoreError;
use construct_core::mcstructure;
use construct_core::merge::{self, MergeOptions, OnOverlap};
use construct_core::pack;
use construct_core::store::{OpenedStore, StructureStore};
use serde::Serialize;
use std::io;
use std::path::{Component, Path, PathBuf};

#[derive(Serialize)]
struct Payload {
    world: Option<String>,
    written: Vec<Written>,
}

#[derive(Serialize)]
struct Written {
    name: String,
    path: String,
    bytes: u64,
}

fn refuse_traversal(name: &str) -> Result<()> {
    let has_separator = name.contains('/') || name.contains('\\');
    let has_dangerous_component = Path::new(name).components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    });
    if has_separator || has_dangerous_component {
        return Err(CoreError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "structure {name:?} cannot be used as a filename: it would write \
                 outside the output directory. Pass -n to choose the destination \
                 explicitly."
            ),
        )));
    }
    Ok(())
}

fn refuse_empty_derived_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(CoreError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "structure has an empty name and cannot be used as a filename. \
             Pass -n to choose the destination explicitly."
                .to_string(),
        )));
    }
    Ok(())
}

const RESERVED_DEVICE_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

fn sanitize_for_filename(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| match c {
            ':' | '<' | '>' | '"' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();

    let stem = sanitized.split('.').next().unwrap_or("");
    if RESERVED_DEVICE_NAMES
        .iter()
        .any(|reserved| stem.eq_ignore_ascii_case(reserved))
    {
        format!("_{sanitized}")
    } else {
        sanitized
    }
}

fn mcstructure_path(path: &Path) -> std::result::Result<PathBuf, String> {
    match path.extension().and_then(|e| e.to_str()) {
        Some(e) if e.eq_ignore_ascii_case(pack::structures::EXTENSION) => Ok(path.to_path_buf()),
        Some(other) => Err(format!(".{other}")),
        None => Ok(path.with_extension(pack::structures::EXTENSION)),
    }
}

fn output_path(args: &ExportArgs) -> failure::Result<Option<PathBuf>> {
    let Some(given) = args.name.as_deref() else {
        return Ok(None);
    };
    mcstructure_path(given).map(Some).map_err(|found| {
        let stem = given
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "out".to_string());
        Failure::usage(
            format!("-n writes a .mcstructure file, but {found} was given"),
            format!("construct export <structure> -n {stem}.mcstructure"),
        )
    })
}

pub fn dispatch(args: &ExportArgs, ctx: &Context, out: &mut Out) -> failure::Result {
    if args.merge && args.name.is_none() {
        return Err(Failure::usage(
            "--merge writes a single file and needs -n to name it",
            "construct export <s1> <s2>... --merge -n merged.mcstructure",
        ));
    }
    check_source_against_world(
        "export <structure>",
        "read from",
        args.world.as_ref(),
        args.source,
        true,
    )?;
    let output = output_path(args)?;
    if !args.merge && args.structures.len() > 1 && output.is_some() {
        return Err(Failure::usage(
            format!(
                "-n takes a single output file, but {} structures were given",
                args.structures.len()
            ),
            "Drop -n to write one file per structure, or add --merge to combine them \
             into one.",
        ));
    }

    let opts = Options {
        structures: &args.structures,
        output: output.as_deref(),
        source: args.source.map(Into::into),
        force: args.force,
        merge: args.merge,
        on_overlap: args.on_overlap.into(),
    };

    match &args.world {
        Some(reference) => {
            let world = ctx.world(reference)?;
            Ok(for_world(&world, &ctx.installations, &opts, out)?)
        }
        None => Ok(shared(ctx.installation()?, &opts, out)?),
    }
}

pub struct Options<'a> {
    pub structures: &'a [String],
    pub output: Option<&'a Path>,
    pub source: Option<Source>,
    pub force: bool,
    pub merge: bool,
    pub on_overlap: OnOverlap,
}

struct View<'a> {
    world: Option<&'a World>,
    entries: Vec<Entry>,
    store: Option<OpenedStore>,
}

fn shared(installation: &Installation, opts: &Options, out: &mut Out) -> Result<()> {
    let home = pack::for_installation(installation)?.pack;
    let entries = catalog::from_pack(&home.dir, Source::SharedPack);
    let view = View {
        world: None,
        entries,
        store: None,
    };
    run(&view, opts, out)
}

fn for_world(
    world: &World,
    installations: &[Installation],
    opts: &Options,
    out: &mut Out,
) -> Result<()> {
    let loaded = loader::for_world(world, installations, opts.source, out)?;
    let view = View {
        world: Some(world),
        entries: loader::world_scoped(loaded.entries),
        store: loaded.store,
    };
    run(&view, opts, out)
}

fn run(view: &View, opts: &Options, out: &mut Out) -> Result<()> {
    if opts.merge {
        return run_merge(view, opts, out);
    }

    let mut plan: Vec<(catalog::Entry, PathBuf)> = Vec::new();
    for name in opts.structures {
        let entry = resolve(name, view, opts.source)?;
        let target = match opts.output {
            Some(path) => path.to_path_buf(),
            None => {
                refuse_empty_derived_name(&entry.name)?;
                refuse_traversal(&entry.name)?;
                PathBuf::from(format!(
                    "{}.mcstructure",
                    sanitize_for_filename(&entry.name)
                ))
            }
        };
        plan.push((entry, target));
    }

    for (_, target) in &plan {
        if target.exists() && !opts.force {
            return Err(CoreError::TargetExists {
                path: target.clone(),
            });
        }
    }

    let store = view.store.as_ref().map(|s| s as &dyn StructureStore);
    let mut written = Vec::new();
    for (entry, target) in plan {
        let bytes = catalog::read_entry(&entry, store)?;
        if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target, &bytes)?;
        out.line(format!(
            "wrote {} ({})",
            target.display(),
            human_size(bytes.len() as u64)
        ));
        written.push(Written {
            name: entry.name,
            path: target.display().to_string(),
            bytes: bytes.len() as u64,
        });
    }

    out.emit(Payload {
        world: view.world.map(World::qualified),
        written,
    });
    Ok(())
}

fn resolve(name: &str, view: &View, source: Option<Source>) -> Result<catalog::Entry> {
    catalog::resolve(name, &view.entries, source).map_err(|e| loader::explain_miss(e, view.world))
}

#[derive(Serialize)]
struct MergedPayload {
    world: Option<String>,
    merged: Merged,
}

#[derive(Serialize)]
struct Merged {
    path: String,
    bytes: u64,
    sources: Vec<String>,
    size: [i32; 3],
    origin: [i32; 3],
    overlaps: Vec<OverlapRow>,
}

#[derive(Serialize)]
struct OverlapRow {
    count: u64,
    pieces: Vec<String>,
}

fn run_merge(view: &View, opts: &Options, out: &mut Out) -> Result<()> {
    let target = opts.output.expect("dispatch refuses --merge without -n");
    if target.exists() && !opts.force {
        return Err(CoreError::TargetExists {
            path: target.to_path_buf(),
        });
    }

    let store = view.store.as_ref().map(|s| s as &dyn StructureStore);
    let mut pieces = Vec::new();
    for name in opts.structures {
        let entry = resolve(name, view, opts.source)?;
        let bytes = catalog::read_entry(&entry, store)?;
        let decoded = mcstructure::decode(&bytes, &entry.name)?;
        pieces.push((entry.name.clone(), decoded));
    }

    let options = MergeOptions {
        on_overlap: opts.on_overlap,
        ..MergeOptions::default()
    };
    let report = merge::merge(&pieces, &options)?;

    for warning in &report.warnings {
        out.warn(warning.clone());
    }
    for overlap in &report.overlaps {
        out.warn(format!(
            "{} blocks overlapped between {:?} and {:?}",
            overlap.count, overlap.pieces[0], overlap.pieces[1]
        ));
    }

    let bytes = mcstructure::encode(&report.structure, &target.display().to_string())?;
    if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(target, &bytes)?;

    let s = &report.structure;
    out.line(format!(
        "wrote {} ({}) — {} x {} x {} from {} structures",
        target.display(),
        human_size(bytes.len() as u64),
        s.size.x,
        s.size.y,
        s.size.z,
        pieces.len()
    ));

    out.emit(MergedPayload {
        world: view.world.map(World::qualified),
        merged: Merged {
            path: target.display().to_string(),
            bytes: bytes.len() as u64,
            sources: pieces.into_iter().map(|(n, _)| n).collect(),
            size: [s.size.x, s.size.y, s.size.z],
            origin: [s.origin.x, s.origin.y, s.origin.z],
            overlaps: report
                .overlaps
                .iter()
                .map(|o| OverlapRow {
                    count: o.count,
                    pieces: o.pieces.clone(),
                })
                .collect(),
        },
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_extension_is_completed() {
        assert_eq!(
            mcstructure_path(Path::new("castle")).unwrap(),
            PathBuf::from("castle.mcstructure")
        );
    }

    #[test]
    fn the_right_extension_passes_through_untouched() {
        assert_eq!(
            mcstructure_path(Path::new("out/castle.mcstructure")).unwrap(),
            PathBuf::from("out/castle.mcstructure")
        );
    }

    #[test]
    fn an_uppercase_extension_is_accepted_as_given() {
        assert_eq!(
            mcstructure_path(Path::new("CASTLE.MCSTRUCTURE")).unwrap(),
            PathBuf::from("CASTLE.MCSTRUCTURE")
        );
    }

    #[test]
    fn another_extension_reports_the_one_that_was_given() {
        assert_eq!(
            mcstructure_path(Path::new("castle.nbt")).unwrap_err(),
            ".nbt"
        );
    }

    #[test]
    fn a_path_with_no_file_name_is_left_alone() {
        assert_eq!(
            mcstructure_path(Path::new(".")).unwrap(),
            PathBuf::from(".")
        );
    }
}
