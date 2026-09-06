use clap::{Parser, Subcommand, ValueEnum};
use clap_complete::engine::ArgValueCandidates;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "construct",
    version,
    about = "Move structures between Minecraft Bedrock worlds"
)]
pub struct Cli {
    /// Extra com.mojang root to probe. Repeatable.
    #[arg(long, global = true, value_name = "PATH", value_hint = clap::ValueHint::DirPath)]
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

    /// Disambiguate a structure name present in both packs a world sees —
    /// its own and the installation's shared Construct.
    #[arg(long, global = true, value_enum)]
    pub pack: Option<PackArg>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
pub enum SourceArg {
    World,
    Pack,
}

/// Which pack, when a world sees one name in both of the ones serving it.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum PackArg {
    /// The world's own pack — its structures pack, or its own copy of
    /// Construct when that is what it runs.
    World,
    /// The installation's shared copy of Construct, which every world using
    /// it sees.
    Shared,
}

impl From<PackArg> for construct_core::pack::Scope {
    fn from(p: PackArg) -> Self {
        match p {
            PackArg::World => Self::WorldLocal,
            PackArg::Shared => Self::Shared,
        }
    }
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
    /// Add a com.mojang or world folder to the automatic search.
    Add {
        /// Directory to add to the appropriate configuration list.
        #[arg(value_name = "PATH", value_hint = clap::ValueHint::DirPath)]
        path: PathBuf,
    },

    /// List discovered worlds.
    Worlds,

    /// List the structures in a world, or in the shared pack with no world.
    List {
        /// World name, qualified reference, or path. Without one, the
        /// installation's shared Construct pack is listed on its own.
        #[arg(add = ArgValueCandidates::new(crate::complete::complete_worlds))]
        world: Option<String>,
    },

    /// Write structures out as .mcstructure files.
    Export {
        /// World name, qualified reference, or path.
        #[arg(add = ArgValueCandidates::new(crate::complete::complete_worlds))]
        world: String,
        /// One or more structure names.
        #[arg(required = true, add = ArgValueCandidates::new(crate::complete::complete_structures))]
        structures: Vec<String>,
        /// Output file. Only valid with a single structure.
        #[arg(short = 'o', long, value_hint = clap::ValueHint::FilePath)]
        output: Option<PathBuf>,
        /// Combine the structures into one, reassembled at their saved world
        /// positions. Requires -o.
        #[arg(long)]
        merge: bool,
        /// How to resolve positions where two structures both have a block.
        #[arg(long, value_name = "MODE", default_value = "last")]
        on_overlap: OverlapArg,
    },

    /// Copy .mcstructure files into Construct's structures folder.
    Import {
        /// One or more .mcstructure files to import.
        #[arg(required = true, value_hint = clap::ValueHint::FilePath)]
        files: Vec<PathBuf>,
        /// Target this world's Construct copy.
        #[arg(short = 'w', long, value_name = "WORLD", add = ArgValueCandidates::new(crate::complete::complete_worlds))]
        world: Option<String>,
        /// Override the name derived from the file stem. Only valid with a
        /// single file.
        #[arg(long, value_name = "NAME")]
        name: Option<String>,
    },

    /// Copy structures into another world's Construct.
    Copy {
        /// Source world name, qualified reference, or path.
        #[arg(add = ArgValueCandidates::new(crate::complete::complete_worlds))]
        src_world: String,
        /// Destination world name, qualified reference, or path.
        #[arg(add = ArgValueCandidates::new(crate::complete::complete_worlds))]
        dst_world: String,
        /// One or more structure names.
        #[arg(required = true, add = ArgValueCandidates::new(crate::complete::complete_structures))]
        structures: Vec<String>,
    },

    /// Remove imported structures. `--source world` is not implemented yet.
    Delete {
        /// World name, qualified reference, or path.
        #[arg(add = ArgValueCandidates::new(crate::complete::complete_worlds))]
        world: String,
        /// One or more structure names.
        #[arg(required = true, add = ArgValueCandidates::new(crate::complete::complete_structures))]
        structures: Vec<String>,
    },

    /// Turn a world's Beta APIs (`gametest`) experiment on.
    EnableBetaApis {
        /// World name, qualified reference, or path.
        #[arg(add = ArgValueCandidates::new(crate::complete::complete_worlds))]
        world: String,
    },

    /// Download and install Construct.
    Install {
        /// A specific version, e.g. 1.2.0. Defaults to the latest release.
        #[arg(long, value_name = "VERSION")]
        version: Option<String>,
        /// Also enable Construct in this world and turn Beta APIs on.
        #[arg(short = 'w', long, value_name = "WORLD", add = ArgValueCandidates::new(crate::complete::complete_worlds))]
        world: Option<String>,
    },

    /// Show the installed version, the latest available, and where it's enabled.
    Status,

    /// Generate shell tab-completion scripts.
    Completions {
        /// Shell to generate completions for.
        #[arg(value_enum)]
        shell: ShellArg,
    },
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
pub enum ShellArg {
    Bash,
    Elvish,
    Fish,
    #[value(name = "powershell", alias = "power-shell")]
    PowerShell,
    Zsh,
}

#[derive(Copy, Clone, PartialEq, Eq, ValueEnum)]
pub enum OverlapArg {
    /// The structure named later on the command line wins.
    Last,
    /// The structure named earlier on the command line wins.
    First,
    /// Refuse the merge.
    Error,
}

impl From<OverlapArg> for construct_core::merge::OnOverlap {
    fn from(v: OverlapArg) -> Self {
        match v {
            OverlapArg::Last => Self::Last,
            OverlapArg::First => Self::First,
            OverlapArg::Error => Self::Error,
        }
    }
}
