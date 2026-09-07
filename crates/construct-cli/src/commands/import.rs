use crate::cli::ImportArgs;
use crate::context::Context;
use crate::failure::{self, Failure};
use crate::output::Out;
use crate::support::packs::{home_for_write, warn_if_another_pack_has_it};
use crate::support::phrasing::{pack_phrase, target_field};
use construct_core::discovery::{Installation, World};
use construct_core::pack::{self, structures};
use construct_core::store::key;
use construct_core::{CoreError, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
struct Payload {
    pack: String,
    target: &'static str,
    written: Vec<Written>,
}

#[derive(Serialize)]
struct Written {
    name: String,
    id: String,
    path: String,
    bytes: u64,
}

fn id_for(file: &Path, name: Option<&str>) -> Result<String> {
    match name {
        Some(n) => Ok(key::qualify(n)),
        None => {
            let stem = file.file_stem().and_then(|s| s.to_str()).ok_or_else(|| {
                CoreError::BadStructureName {
                    name: file.display().to_string(),
                    reason: "the file has no usable stem".to_string(),
                }
            })?;
            Ok(key::qualify(&structures::derive_name(stem)?))
        }
    }
}

struct Planned {
    file: PathBuf,
    id: String,
}

fn mcstructures_under(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    walk(dir, &mut found)?;
    found.sort();
    Ok(found)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            walk(&path, out)?;
        } else if path.extension().and_then(|x| x.to_str()) == Some(structures::EXTENSION) {
            out.push(path);
        }
    }
    Ok(())
}

fn id_under_directory(root_name: &str, file: &Path, dir: &Path) -> Result<String> {
    let rel = file.strip_prefix(dir).map_err(|_| CoreError::Internal {
        what: format!("{} is not under {}", file.display(), dir.display()),
    })?;

    let mut segments = vec![structures::derive_name(root_name)?];
    let parents = rel.parent().map(Path::to_path_buf).unwrap_or_default();
    for component in parents.components() {
        let part = component
            .as_os_str()
            .to_str()
            .ok_or_else(|| CoreError::BadStructureName {
                name: file.display().to_string(),
                reason: "a folder name that is not valid UTF-8".to_string(),
            })?;
        segments.push(structures::derive_name(part)?);
    }
    let stem =
        file.file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| CoreError::BadStructureName {
                name: file.display().to_string(),
                reason: "the file has no usable stem".to_string(),
            })?;
    segments.push(structures::derive_name(stem)?);

    let (namespace, rest) = segments.split_first().expect("root name is always present");
    Ok(format!("{}:{}", namespace, rest.join("/")))
}

fn expand(paths: &[PathBuf], name: Option<&str>) -> Result<Vec<Planned>> {
    let mut planned = Vec::new();
    for path in paths {
        if !path.is_dir() {
            planned.push(Planned {
                id: id_for(path, name)?,
                file: path.clone(),
            });
            continue;
        }

        let root_name = path.file_name().and_then(|s| s.to_str()).ok_or_else(|| {
            CoreError::BadStructureName {
                name: path.display().to_string(),
                reason: "this folder has no name to import under; \
                         name the folder by its own path"
                    .to_string(),
            }
        })?;

        let files = mcstructures_under(path)?;
        if files.is_empty() {
            return Err(CoreError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("{} holds no .mcstructure files", path.display()),
            )));
        }
        for file in files {
            planned.push(Planned {
                id: id_under_directory(root_name, &file, path)?,
                file,
            });
        }
    }
    Ok(planned)
}

fn warn_about_namespaces_outside_the_default(ids: &[String], out: &mut Out) {
    let default = format!("{}:", key::DEFAULT_NAMESPACE);
    let mut seen: Vec<&str> = Vec::new();
    for id in ids {
        if id.starts_with(&default) {
            continue;
        }
        let Some((namespace, _)) = id.split_once(':') else {
            continue;
        };
        if seen.contains(&namespace) {
            continue;
        }
        seen.push(namespace);
        out.warn(format!(
            "{namespace} is outside the mystructure namespace; \
             Construct's in-game list will not show its structures"
        ));
    }
}

fn check_name_against_paths(args: &ImportArgs) -> failure::Result {
    if args.name.is_none() {
        return Ok(());
    }
    if let Some(dir) = args.paths.iter().find(|p| p.is_dir()) {
        return Err(Failure::usage(
            format!(
                "--name renames a single import, but {} is a folder",
                dir.display()
            ),
            "A folder imports every structure under it, keeping its tree. \
             Drop --name, or name a single file to rename it.",
        ));
    }
    if args.paths.len() > 1 {
        return Err(Failure::usage(
            format!(
                "--name renames a single import, but {} files were given",
                args.paths.len()
            ),
            "Drop --name to derive each name from its file stem, or import them \
             one at a time.",
        ));
    }
    Ok(())
}

pub fn dispatch(args: &ImportArgs, ctx: &Context, out: &mut Out) -> failure::Result {
    check_name_against_paths(args)?;
    let world = ctx.optional_world(args.world.as_deref())?;
    let installation = ctx.installation_for(world.as_ref())?;
    let opts = Options {
        paths: &args.paths,
        name: args.name.as_deref(),
        force: args.force,
    };
    Ok(run(world.as_ref(), installation, &opts, out)?)
}

pub struct Options<'a> {
    pub paths: &'a [PathBuf],
    pub name: Option<&'a str>,
    pub force: bool,
}

fn run(
    world: Option<&World>,
    installation: &Installation,
    opts: &Options,
    out: &mut Out,
) -> Result<()> {
    let mut sources: Vec<(PathBuf, String, Vec<u8>)> = Vec::new();
    for Planned { file, id } in expand(opts.paths, opts.name)? {
        let bytes = std::fs::read(&file)?;
        sources.push((file, id, bytes));
    }

    for i in 1..sources.len() {
        if let Some((earlier, _, _)) = sources[..i].iter().find(|(_, id, _)| *id == sources[i].1) {
            return Err(CoreError::BadStructureName {
                name: sources[i].1.clone(),
                reason: format!(
                    "two files would import under this name: {} and {}",
                    earlier.display(),
                    sources[i].0.display()
                ),
            });
        }
    }

    let home = match world {
        Some(w) => home_for_write(w, installation, out)?,
        None => pack::Home {
            dir: pack::for_installation(installation)?.pack.dir,
            kind: pack::HomeKind::SharedConstruct,
        },
    };

    let mut plan = Vec::new();
    for (file, id, bytes) in sources {
        let target = structures::path_for(&home.dir, &id)?;
        plan.push((file, id, bytes, target));
    }
    if !opts.force {
        for (_, _, _, target) in &plan {
            if target.exists() {
                return Err(CoreError::TargetExists {
                    path: target.clone(),
                });
            }
        }
    }

    let ids: Vec<String> = plan.iter().map(|(_, id, _, _)| id.clone()).collect();
    warn_about_namespaces_outside_the_default(&ids, out);

    let mut written = Vec::new();
    for (file, id, bytes, _) in plan {
        if let Some(w) = world {
            warn_if_another_pack_has_it(w, installation, &home.dir, &id, out);
        }
        let path = structures::write(&home.dir, &id, &bytes, opts.force)?;

        out.line(format!(
            "imported {} as {}",
            file.display(),
            key::display_name(&id)
        ));
        out.line(format!("  {}", path.display()));

        written.push(Written {
            name: key::display_name(&id).to_string(),
            id,
            path: path.display().to_string(),
            bytes: bytes.len() as u64,
        });
    }

    out.line(format!(
        "into {}",
        pack_phrase(home.kind, world.map(|w| w.display_name.as_str()))
    ));
    out.line("Reload the world before Construct sees it.");
    out.emit(Payload {
        pack: home.dir.display().to_string(),
        target: target_field(home.kind),
        written,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Discovery;

    fn args(paths: &[&str], name: Option<&str>) -> ImportArgs {
        ImportArgs {
            paths: paths.iter().map(PathBuf::from).collect(),
            world: None,
            name: name.map(str::to_string),
            force: false,
            discovery: Discovery { path: Vec::new() },
        }
    }

    fn message_of(result: failure::Result) -> String {
        match result {
            Err(Failure::Usage { message, .. }) => message,
            Err(_) => panic!("expected a usage refusal, got another failure"),
            Ok(()) => panic!("expected a usage refusal, got Ok"),
        }
    }

    #[test]
    fn no_name_never_refuses() {
        assert!(check_name_against_paths(&args(&["a.mcstructure", "b.mcstructure"], None)).is_ok());
    }

    #[test]
    fn name_with_one_file_is_what_it_is_for() {
        assert!(check_name_against_paths(&args(&["a.mcstructure"], Some("castle"))).is_ok());
    }

    #[test]
    fn name_with_several_files_is_refused_and_counts_them() {
        let message = message_of(check_name_against_paths(&args(
            &["a.mcstructure", "b.mcstructure"],
            Some("castle"),
        )));
        assert_eq!(
            message,
            "--name renames a single import, but 2 files were given"
        );
    }

    #[test]
    fn name_with_a_folder_is_refused_as_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        let args = ImportArgs {
            paths: vec![dir.path().to_path_buf()],
            world: None,
            name: Some("castle".to_string()),
            force: false,
            discovery: Discovery { path: Vec::new() },
        };
        let message = message_of(check_name_against_paths(&args));
        assert!(message.starts_with("--name renames a single import, but "));
        assert!(message.ends_with(" is a folder"));
    }
}
