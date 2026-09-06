use clap::{Args, Parser, Subcommand, ValueEnum};
use clap_complete::engine::ArgValueCandidates;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "construct",
    version,
    about = "Move structures between Minecraft Bedrock worlds"
)]
pub struct Cli {
    /// Emit one JSON document on stdout instead of human output.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub command: Command,
}

/// Where to look for Minecraft, flattened into every command that searches.
///
/// Declared here rather than globally on `Cli` so `add` and `completions` —
/// the two commands that never run discovery — refuse it instead of parsing
/// it and doing nothing with it.
#[derive(Args)]
pub struct Discovery {
    /// Extra com.mojang root or world folder to discover.
    #[arg(long, value_name = "PATH", value_hint = clap::ValueHint::DirPath)]
    pub path: Vec<PathBuf>,
}

/// Where a structure lives: a world's database, or one of the two packs that
/// can hold it.
///
/// One axis, not two: the old `--source`/`--pack` pair could not name a single
/// place without both flags, and `--source world-pack` on a world seeing two
/// packs named neither of them.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum SourceArg {
    /// The world's own leveldb database.
    WorldDb,
    /// A pack serving that world alone — its structures pack, or its own copy
    /// of Construct.
    WorldPack,
    /// The shared copy of Construct, which every world using it sees.
    SharedPack,
}

impl SourceArg {
    /// The spelling this value has on the command line and in JSON.
    pub fn as_str(self) -> &'static str {
        construct_core::catalog::Source::from(self).as_str()
    }

    /// Whether this names a place inside one world, which only a command with
    /// a world in hand can reach.
    pub fn needs_a_world(self) -> bool {
        matches!(self, SourceArg::WorldDb | SourceArg::WorldPack)
    }
}

impl From<SourceArg> for construct_core::catalog::Source {
    fn from(s: SourceArg) -> Self {
        match s {
            SourceArg::WorldDb => Self::WorldDb,
            SourceArg::WorldPack => Self::WorldPack,
            SourceArg::SharedPack => Self::SharedPack,
        }
    }
}

#[derive(Subcommand)]
pub enum Command {
    /// Add a com.mojang or world folder to automatic discovery.
    Add {
        /// Directory to add to the appropriate configuration list.
        #[arg(value_name = "PATH", value_hint = clap::ValueHint::DirPath)]
        path: PathBuf,
    },

    /// List all discovered worlds.
    Worlds {
        #[command(flatten)]
        discovery: Discovery,
    },

    /// List the structures in a world, or in the shared copy of Construct.
    Structures {
        /// List this world's structures, by name, qualified reference, or
        /// path.
        ///
        /// Omitted, the shared copy of Construct is listed on its own.
        #[arg(short = 'w', long, value_name = "WORLD", add = ArgValueCandidates::new(crate::complete::complete_worlds))]
        world: Option<String>,
        /// Show only structures from this place.
        ///
        /// world-db and world-pack need --world; shared-pack is what a
        /// listing with no --world already shows.
        #[arg(long, value_enum)]
        source: Option<SourceArg>,
        #[command(flatten)]
        discovery: Discovery,
    },

    /// Write structures out as .mcstructure files.
    Export {
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
        /// Read from this world only
        ///
        /// Omitted, the shared copy of Construct is the source — the same way
        /// `import` places into the shared copy unless told otherwise.
        #[arg(short = 'w', long, value_name = "WORLD", add = ArgValueCandidates::new(crate::complete::complete_worlds))]
        world: Option<String>,
        /// Read only from this place.
        ///
        /// Disambiguates a name present in both a world's database and its
        /// pack. world-db and world-pack need --world; shared-pack is the
        /// only place a read with no --world looks.
        #[arg(long, value_enum)]
        source: Option<SourceArg>,
        /// Overwrite the output file if it already exists.
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        discovery: Discovery,
    },

    /// Import .mcstructure files into a world or the shared Construct pack.
    Import {
        /// One or more .mcstructure files, or folders of them.
        ///
        /// A folder imports every .mcstructure under it and keeps its tree:
        /// the folder's own name becomes the namespace, so `Amelix/CF-Q1`
        /// imports as `Amelix:CF-Q1`. Naming the files instead — with a
        /// shell wildcard, say — imports them flat under `mystructure:`.
        #[arg(required = true, value_hint = clap::ValueHint::AnyPath)]
        paths: Vec<PathBuf>,
        /// Target this world's Construct copy.
        #[arg(short = 'w', long, value_name = "WORLD", add = ArgValueCandidates::new(crate::complete::complete_worlds))]
        world: Option<String>,
        /// Override the name derived from the file stem. Only valid with a
        /// single file, never with a folder.
        #[arg(long, value_name = "NAME")]
        name: Option<String>,
        /// Overwrite a structure of the same name already in the target pack.
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        discovery: Discovery,
    },

    /// Copy structures from one world into another world.
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
        /// Where in SRC_WORLD to read from.
        ///
        /// Applies to the source only. Nothing selects the destination —
        /// the write always lands in DST_WORLD's own structures home. Use it
        /// when a name exists in more than one of the three places SRC_WORLD
        /// sees: its database, its own pack, and the shared copy of Construct.
        #[arg(long, value_enum)]
        source: Option<SourceArg>,
        /// Overwrite a structure of the same name already in DST_WORLD.
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        discovery: Discovery,
    },

    /// Remove structures from the shared Construct, or from one world.
    Delete {
        /// One or more structure names.
        #[arg(required = true, add = ArgValueCandidates::new(crate::complete::complete_structures))]
        structures: Vec<String>,
        /// Delete from this world only
        ///
        /// Omitted, the shared copy of Construct is the target — the same way
        /// `import` places into the shared copy unless told otherwise.
        #[arg(short = 'w', long, value_name = "WORLD", add = ArgValueCandidates::new(crate::complete::complete_worlds))]
        world: Option<String>,
        /// Delete only from this place.
        ///
        /// Narrows a name that is in both a world's database and its pack.
        /// world-db and world-pack need --world; shared-pack is the only
        /// place a delete with no --world touches.
        #[arg(long, value_enum)]
        source: Option<SourceArg>,
        #[command(flatten)]
        discovery: Discovery,
    },

    /// Turn a world's Beta APIs experiment on.
    EnableBetaApis {
        /// World name, qualified reference, or path.
        #[arg(add = ArgValueCandidates::new(crate::complete::complete_worlds))]
        world: String,
        #[command(flatten)]
        discovery: Discovery,
    },

    /// Install or upgrade Construct
    Install {
        /// A specific version, e.g. 1.2.0. Defaults to the latest release.
        #[arg(long, value_name = "VERSION")]
        version: Option<String>,
        /// Also enable Construct in this world and turn Beta APIs on.
        #[arg(short = 'w', long, value_name = "WORLD", add = ArgValueCandidates::new(crate::complete::complete_worlds))]
        world: Option<String>,
        /// Reinstall the version already installed instead of doing nothing.
        #[arg(long)]
        force: bool,
        #[command(flatten)]
        discovery: Discovery,
    },

    /// Show the installed version, the latest available, and where it's enabled.
    Status {
        #[command(flatten)]
        discovery: Discovery,
    },

    /// Generate shell tab-completion scripts.
    Completions {
        /// Shell to generate completions for.
        #[arg(value_enum)]
        shell: ShellArg,
    },
}

impl Command {
    /// The extra paths this command was given, for the one discovery pass
    /// `main` runs before dispatching. Each is a com.mojang root or a world
    /// folder; `main` sorts them. Matched exhaustively on purpose: a new
    /// command has to say whether it searches for Minecraft or not.
    pub fn paths(&self) -> &[PathBuf] {
        match self {
            Self::Worlds { discovery }
            | Self::Structures { discovery, .. }
            | Self::Export { discovery, .. }
            | Self::Import { discovery, .. }
            | Self::Copy { discovery, .. }
            | Self::Delete { discovery, .. }
            | Self::EnableBetaApis { discovery, .. }
            | Self::Install { discovery, .. }
            | Self::Status { discovery } => &discovery.path,
            // Neither searches: `add` writes a path into the config file, and
            // `completions` prints a script.
            Self::Add { .. } | Self::Completions { .. } => &[],
        }
    }
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
