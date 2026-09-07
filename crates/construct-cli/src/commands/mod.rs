pub mod add;
pub mod completions;
pub mod copy;
pub mod delete;
pub mod enable_beta_apis;
pub mod export;
pub mod import;
pub mod install;
pub mod status;
pub mod structures;
pub mod worlds;

use crate::cli::Command;
use crate::context::Context;
use crate::failure;
use crate::output::Out;

pub fn dispatch(command: &Command, ctx: &Context, out: &mut Out) -> failure::Result {
    match command {
        Command::Add(a) => add::dispatch(a, ctx, out),
        Command::Worlds(a) => worlds::dispatch(a, ctx, out),
        Command::Structures(a) => structures::dispatch(a, ctx, out),
        Command::Export(a) => export::dispatch(a, ctx, out),
        Command::Import(a) => import::dispatch(a, ctx, out),
        Command::Copy(a) => copy::dispatch(a, ctx, out),
        Command::Delete(a) => delete::dispatch(a, ctx, out),
        Command::EnableBetaApis(a) => enable_beta_apis::dispatch(a, ctx, out),
        Command::Install(a) => install::dispatch(a, ctx, out),
        Command::Status(a) => status::dispatch(a, ctx, out),
        Command::Completions(a) => completions::dispatch(a, ctx),
    }
}
