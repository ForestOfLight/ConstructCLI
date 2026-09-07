use crate::cli::{CompletionsArgs, ShellArg};
use crate::context::Context;
use crate::failure;
use clap_complete::env::EnvCompleter;
use std::io::Write;

pub fn dispatch(args: &CompletionsArgs, _ctx: &Context) -> failure::Result {
    Ok(run(args.shell)?)
}

fn run(shell: ShellArg) -> construct_core::Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();

    let res = match shell {
        ShellArg::Bash => clap_complete::env::Bash.write_registration(
            "COMPLETE",
            "construct",
            "construct",
            "construct",
            &mut handle,
        ),
        ShellArg::Zsh => clap_complete::env::Zsh.write_registration(
            "COMPLETE",
            "construct",
            "construct",
            "construct",
            &mut handle,
        ),
        ShellArg::Fish => clap_complete::env::Fish.write_registration(
            "COMPLETE",
            "construct",
            "construct",
            "construct",
            &mut handle,
        ),
        ShellArg::Elvish => clap_complete::env::Elvish.write_registration(
            "COMPLETE",
            "construct",
            "construct",
            "construct",
            &mut handle,
        ),
        ShellArg::PowerShell => clap_complete::env::Powershell.write_registration(
            "COMPLETE",
            "construct",
            "construct",
            "construct",
            &mut handle,
        ),
    };

    res.map_err(construct_core::CoreError::Io)?;
    handle.flush().map_err(construct_core::CoreError::Io)?;
    Ok(())
}
