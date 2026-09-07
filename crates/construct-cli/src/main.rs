mod cli;
mod commands;
mod complete;
mod context;
mod failure;
mod output;
mod report;
mod support;

use clap::{CommandFactory, Parser};
use cli::Cli;
use context::Context;
use output::Out;

fn main() {
    clap_complete::CompleteEnv::with_factory(Cli::command).complete();

    let cli = Cli::parse();
    let mut out = Out::new(cli.json);

    if let Err(failure) = run(&cli, &mut out) {
        report::render(&failure, &out);
        std::process::exit(failure.exit_code());
    }
}

fn run(cli: &Cli, out: &mut Out) -> failure::Result {
    let ctx = Context::build(cli, out)?;
    commands::dispatch(&cli.command, &ctx, out)
}
