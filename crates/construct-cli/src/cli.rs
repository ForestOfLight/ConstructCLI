use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "construct",
    version,
    about = "Move structures between Minecraft Bedrock worlds"
)]
pub struct Cli {
    /// Extra com.mojang root to probe. Repeatable.
    #[arg(long, global = true, value_name = "PATH")]
    pub com_mojang: Vec<PathBuf>,

    /// Emit one JSON document on stdout instead of human output.
    #[arg(long, global = true)]
    pub json: bool,

    /// Overwrite an existing target file. Never relaxes the world-in-use refusal.
    #[arg(long, global = true)]
    pub force: bool,

    /// Disambiguate a structure name present in both a world and a pack.
    #[arg(long, global = true, value_enum)]
    pub source: Option<SourceArg>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
pub enum SourceArg {
    World,
    Pack,
}

impl From<SourceArg> for construct_core::catalog::Source {
    fn from(s: SourceArg) -> Self {
        match s {
            SourceArg::World => Self::World,
            SourceArg::Pack => Self::Pack,
        }
    }
}

#[derive(Subcommand)]
pub enum Command {
    /// List discovered worlds.
    Worlds,

    /// List the structures in a world.
    List {
        /// World name, qualified reference, or path.
        world: String,
    },

    /// Write structures out as .mcstructure files.
    Export {
        /// World name, qualified reference, or path.
        world: String,
        /// One or more structure names.
        #[arg(required = true)]
        structures: Vec<String>,
        /// Output file. Only valid with a single structure.
        #[arg(short = 'o', long)]
        output: Option<PathBuf>,
    },

    /// Copy a .mcstructure file into Construct's structures folder.
    Import {
        /// The .mcstructure file to import.
        file: PathBuf,
        /// Target this world's Construct copy.
        #[arg(long, value_name = "WORLD")]
        world: Option<String>,
        /// Override the name derived from the file stem.
        #[arg(long, value_name = "NAME")]
        name: Option<String>,
    },
}
