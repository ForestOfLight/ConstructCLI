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

    /// Read settings from this file instead of the default location.
    ///
    /// The flag layer of the documented precedence: it beats
    /// `CONSTRUCT_CONFIG`, which beats the platform config directory.
    #[arg(long, global = true, value_name = "PATH", value_hint = clap::ValueHint::FilePath)]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Args)]
pub struct Discovery {
    /// Extra com.mojang root or world folder to discover.
    #[arg(long, value_name = "PATH", value_hint = clap::ValueHint::DirPath)]
    pub path: Vec<PathBuf>,
}

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
    pub fn as_str(self) -> &'static str {
        construct_core::catalog::Source::from(self).as_str()
    }

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
    Add(AddArgs),

    /// List all discovered worlds.
    Worlds(WorldsArgs),

    /// List the structures in a world, or in the shared copy of Construct.
    Structures(StructuresArgs),

    /// Write structures out as .mcstructure files.
    Export(ExportArgs),

    /// Import .mcstructure files into a world or the shared Construct pack.
    Import(ImportArgs),

    /// Copy structures from one world into another world.
    Copy(CopyArgs),

    /// Remove structures from the shared Construct, or from one world.
    Delete(DeleteArgs),

    /// Enable a world's Beta APIs experiment.
    EnableBetaApis(EnableBetaApisArgs),

    /// Install or upgrade Construct
    Install(InstallArgs),

    /// Show the installed version, the latest available, and where it's enabled.
    Status(StatusArgs),

    /// Generate shell tab-completion scripts.
    Completions(CompletionsArgs),
}

#[derive(Args)]
pub struct AddArgs {
    /// Directory to add to the appropriate configuration list.
    #[arg(value_name = "PATH", value_hint = clap::ValueHint::DirPath)]
    pub path: PathBuf,
}

#[derive(Args)]
pub struct WorldsArgs {
    #[command(flatten)]
    pub discovery: Discovery,
}

#[derive(Args)]
pub struct StructuresArgs {
    /// List this world's structures, by name, qualified reference, or
    /// path.
    ///
    /// Omitted, the shared copy of Construct is listed on its own.
    #[arg(short = 'w', long, value_name = "WORLD", add = ArgValueCandidates::new(crate::complete::complete_worlds))]
    pub world: Option<String>,
    /// Show only structures from this place.
    ///
    /// world-db and world-pack need --world; shared-pack is what a
    /// listing with no --world already shows.
    #[arg(long, value_enum)]
    pub source: Option<SourceArg>,
    #[command(flatten)]
    pub discovery: Discovery,
}

#[derive(Args)]
pub struct ExportArgs {
    /// One or more structure names.
    #[arg(required = true, add = ArgValueCandidates::new(crate::complete::complete_structures))]
    pub structures: Vec<String>,
    /// Name for the output file. Only valid with a single structure.
    #[arg(short = 'n', long, value_name = "NAME", value_hint = clap::ValueHint::FilePath)]
    pub name: Option<PathBuf>,
    /// Combine the structures into one, reassembled at their saved world
    /// positions. Requires -n.
    #[arg(long)]
    pub merge: bool,
    /// How to resolve positions where two structures both have a block.
    #[arg(long, value_name = "MODE", default_value = "last")]
    pub on_overlap: OverlapArg,
    /// Read from this world only
    ///
    /// Omitted, the shared copy of Construct is the source — the same way
    /// `import` places into the shared copy unless told otherwise.
    #[arg(short = 'w', long, value_name = "WORLD", add = ArgValueCandidates::new(crate::complete::complete_worlds))]
    pub world: Option<String>,
    /// Read only from this place.
    ///
    /// Disambiguates a name present in both a world's database and its
    /// pack. world-db and world-pack need --world; shared-pack is the
    /// only place a read with no --world looks.
    #[arg(long, value_enum)]
    pub source: Option<SourceArg>,
    /// Overwrite the output file if it already exists.
    #[arg(long)]
    pub force: bool,
    #[command(flatten)]
    pub discovery: Discovery,
}

#[derive(Args)]
pub struct ImportArgs {
    /// One or more .mcstructure files, or folders of them.
    ///
    /// A folder imports every .mcstructure under it and keeps its tree:
    /// the folder's own name becomes the namespace, so `Amelix/CF-Q1`
    /// imports as `Amelix:CF-Q1`. Naming the files instead — with a
    /// shell wildcard, say — imports them flat under `mystructure:`.
    #[arg(required = true, value_hint = clap::ValueHint::AnyPath)]
    pub paths: Vec<PathBuf>,
    /// Target this world's Construct copy.
    #[arg(short = 'w', long, value_name = "WORLD", add = ArgValueCandidates::new(crate::complete::complete_worlds))]
    pub world: Option<String>,
    /// Override the name derived from the file stem. Only valid with a
    /// single file, never with a folder.
    #[arg(long, value_name = "NAME")]
    pub name: Option<String>,
    /// Overwrite a structure of the same name already in the target pack.
    #[arg(long)]
    pub force: bool,
    #[command(flatten)]
    pub discovery: Discovery,
}

#[derive(Args)]
pub struct CopyArgs {
    /// Source world name, qualified reference, or path.
    #[arg(add = ArgValueCandidates::new(crate::complete::complete_worlds))]
    pub src_world: String,
    /// Destination world name, qualified reference, or path.
    #[arg(add = ArgValueCandidates::new(crate::complete::complete_worlds))]
    pub dst_world: String,
    /// One or more structure names.
    #[arg(required = true, add = ArgValueCandidates::new(crate::complete::complete_structures))]
    pub structures: Vec<String>,
    /// Where in SRC_WORLD to read from.
    ///
    /// Applies to the source only. Nothing selects the destination —
    /// the write always lands in DST_WORLD's own structures home. Use it
    /// when a name exists in more than one of the three places SRC_WORLD
    /// sees: its database, its own pack, and the shared copy of Construct.
    #[arg(long, value_enum)]
    pub source: Option<SourceArg>,
    /// Overwrite a structure of the same name already in DST_WORLD.
    #[arg(long)]
    pub force: bool,
    #[command(flatten)]
    pub discovery: Discovery,
}

#[derive(Args)]
pub struct DeleteArgs {
    /// One or more structure names.
    #[arg(required = true, add = ArgValueCandidates::new(crate::complete::complete_structures))]
    pub structures: Vec<String>,
    /// Delete from this world only
    ///
    /// Omitted, the shared copy of Construct is the target — the same way
    /// `import` places into the shared copy unless told otherwise.
    #[arg(short = 'w', long, value_name = "WORLD", add = ArgValueCandidates::new(crate::complete::complete_worlds))]
    pub world: Option<String>,
    /// Delete only from this place.
    ///
    /// Narrows a name that is in both a world's database and its pack.
    /// world-db and world-pack need --world; shared-pack is the only
    /// place a delete with no --world touches.
    #[arg(long, value_enum)]
    pub source: Option<SourceArg>,
    #[command(flatten)]
    pub discovery: Discovery,
}

#[derive(Args)]
pub struct EnableBetaApisArgs {
    /// World name, qualified reference, or path.
    #[arg(add = ArgValueCandidates::new(crate::complete::complete_worlds))]
    pub world: String,
    #[command(flatten)]
    pub discovery: Discovery,
}

#[derive(Args)]
pub struct InstallArgs {
    /// A specific version, e.g. 1.2.0. Defaults to the latest release.
    #[arg(long, value_name = "VERSION")]
    pub version: Option<String>,
    /// Also enable Construct in this world and turn Beta APIs on.
    #[arg(short = 'w', long, value_name = "WORLD", add = ArgValueCandidates::new(crate::complete::complete_worlds))]
    pub world: Option<String>,
    /// Reinstall the version already installed instead of doing nothing.
    #[arg(long)]
    pub force: bool,
    #[command(flatten)]
    pub discovery: Discovery,
}

#[derive(Args)]
pub struct StatusArgs {
    #[command(flatten)]
    pub discovery: Discovery,
}

#[derive(Args)]
pub struct CompletionsArgs {
    /// Shell to generate completions for.
    #[arg(value_enum)]
    pub shell: ShellArg,
}

impl Command {
    pub fn paths(&self) -> &[PathBuf] {
        match self {
            Self::Worlds(a) => &a.discovery.path,
            Self::Structures(a) => &a.discovery.path,
            Self::Export(a) => &a.discovery.path,
            Self::Import(a) => &a.discovery.path,
            Self::Copy(a) => &a.discovery.path,
            Self::Delete(a) => &a.discovery.path,
            Self::EnableBetaApis(a) => &a.discovery.path,
            Self::Install(a) => &a.discovery.path,
            Self::Status(a) => &a.discovery.path,
            Self::Add(_) | Self::Completions(_) => &[],
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
