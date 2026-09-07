use crate::cli::AddArgs;
use crate::context::Context;
use crate::failure;
use crate::output::Out;
use construct_core::{CoreError, Result, config};
use std::path::{Path, PathBuf};

pub fn dispatch(args: &AddArgs, ctx: &Context, out: &mut Out) -> failure::Result {
    Ok(run(&args.path, ctx.config_flag.as_deref(), out)?)
}

fn run(path: &Path, explicit_config: Option<&Path>, out: &mut Out) -> Result<()> {
    let config_path = match explicit_config {
        Some(p) => p.to_path_buf(),
        None => {
            config::path(&|key| std::env::var(key).ok()).ok_or_else(|| CoreError::BadConfig {
                path: PathBuf::from("config.toml"),
                reason: "could not determine a configuration directory".to_string(),
            })?
        }
    };
    let (mut settings, warnings) = config::load_file(&config_path)?;
    for warning in warnings {
        out.warn(warning);
    }

    let (kind, added) = config::add_path(&mut settings, path)?;
    if added {
        config::save(&config_path, &settings)?;
    }

    let category = match kind {
        config::AddedPath::Root => "roots",
        config::AddedPath::OtherWorld => "other_worlds",
    };
    if added {
        out.line(format!("added {} to {category}", path.display()));
    } else {
        out.line(format!("{} is already in {category}", path.display()));
    }
    Ok(())
}
