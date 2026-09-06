use std::path::PathBuf;
use thiserror::Error;

/// Every failure `construct-core` can produce.
///
/// Variants carry the data a caller needs to render a good message — the
/// candidates for an ambiguity, the paths probed for a missing root — rather
/// than a pre-formatted string.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("no Minecraft installation found")]
    NoInstallations { probed: Vec<PathBuf> },

    #[error("world not found: {reference}")]
    WorldNotFound {
        reference: String,
        near: Vec<String>,
    },

    #[error("malformed world reference: {reference}")]
    MalformedReference {
        reference: String,
        /// True when the input looks like a filesystem path that was not found on
        /// disk, so the CLI can say so instead of talking about world names.
        looks_like_path: bool,
    },

    #[error("{reference} matches {} worlds", candidates.len())]
    AmbiguousWorld {
        reference: String,
        candidates: Vec<String>,
    },

    #[error("structure not found: {name}")]
    StructureNotFound { name: String, near: Vec<String> },

    #[error("structure {name} matches {} sources", sources.len())]
    AmbiguousStructure { name: String, sources: Vec<String> },

    #[error("more than one installation; no default configured")]
    AmbiguousInstallation { candidates: Vec<String> },

    #[error("no installation named {name}")]
    InstallationNotFound {
        name: String,
        available: Vec<String>,
    },

    #[error("world is in use: {}", world.display())]
    WorldInUse {
        world: PathBuf,
        /// What the refused write would have put at risk. The two are
        /// different dangers and want different advice.
        at_risk: crate::inuse::AtRisk,
    },

    #[error("not enough space to snapshot {}: need {need} bytes, {available} available", world.display())]
    InsufficientSpace {
        world: PathBuf,
        need: u64,
        available: u64,
    },

    #[error("{} already exists", path.display())]
    TargetExists { path: PathBuf },

    #[error("database error: {0}")]
    Db(String),

    #[error("malformed level.dat at {}: {reason}", path.display())]
    BadLevelDat { path: PathBuf, reason: String },

    #[error("cannot rewrite {}: {reason}", path.display())]
    UnwritableLevelDat {
        path: PathBuf,
        reason: String,
        /// Whether a write already landed on disk before this error was
        /// raised. `to_bytes` refuses before touching disk (`false`); the
        /// post-write verification in `apply_beta_apis` fires only after
        /// `write` has already renamed a new file into place (`true`). The
        /// two cases need different advice: one leaves the world untouched,
        /// the other leaves it in a state nobody asked for.
        written: bool,
    },

    #[error("cannot read {}: {reason}", path.display())]
    UnreadableWorld { path: PathBuf, reason: String },

    #[error("config error in {}: {reason}", path.display())]
    BadConfig { path: PathBuf, reason: String },

    #[error("invalid path {}: {reason}", path.display())]
    InvalidPath { path: PathBuf, reason: String },

    #[error("not a usable pack at {}: {reason}", path.display())]
    BadPack { path: PathBuf, reason: String },

    #[error("malformed structure {what}: {reason}")]
    BadStructureFile { what: String, reason: String },

    #[error("cannot merge: {reason}")]
    MergeRefused { reason: String },

    #[error("Construct is not installed")]
    ConstructNotInstalled { searched: Vec<PathBuf> },

    #[error(
        "install of {} did not finish: {reason}; the new pack is staged at {} for manual recovery",
        dest.display(), staging.display()
    )]
    IncompleteInstall {
        dest: PathBuf,
        staging: PathBuf,
        reason: String,
    },

    #[error("unusable structure name {name:?}: {reason}")]
    BadStructureName { name: String, reason: String },

    #[error("internal invariant violated: {what}")]
    Internal { what: String },

    #[error("no platform data directory for backups; set [backups] dir in config.toml")]
    NoBackupDir,

    #[error("could not reach GitHub: {reason}")]
    Network { reason: String },

    #[error("GitHub rate limit reached")]
    RateLimited,

    #[error("no Construct .mcaddon for {version}")]
    AssetNotFound {
        version: String,
        available: Vec<String>,
    },

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl CoreError {
    /// The stable machine-readable name for this failure, emitted as
    /// `error.kind` under `--json`.
    ///
    /// This is the CLI's whole failure vocabulary. Exit codes carry only 0/1/2
    /// — whether it worked, and whether the input was at fault — so *what* went
    /// wrong is said here and nowhere else. It lives beside the enum rather
    /// than in the CLI because a GUI links this library directly: the kind is
    /// part of the error's identity, not a rendering choice.
    ///
    /// The match is exhaustive on purpose. A new variant must name itself
    /// rather than inherit a wildcard's answer, the same way `Command::paths`
    /// in the CLI forces a new command to answer for itself.
    ///
    /// The spelling is the variant name in kebab-case, and callers branch on
    /// it, so treat these strings as the public contract they are.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::NoInstallations { .. } => "no-installations",
            Self::WorldNotFound { .. } => "world-not-found",
            Self::MalformedReference { .. } => "malformed-reference",
            Self::AmbiguousWorld { .. } => "ambiguous-world",
            Self::StructureNotFound { .. } => "structure-not-found",
            Self::AmbiguousStructure { .. } => "ambiguous-structure",
            Self::AmbiguousInstallation { .. } => "ambiguous-installation",
            Self::InstallationNotFound { .. } => "installation-not-found",
            Self::WorldInUse { .. } => "world-in-use",
            Self::InsufficientSpace { .. } => "insufficient-space",
            Self::TargetExists { .. } => "target-exists",
            Self::Db(..) => "db",
            Self::BadLevelDat { .. } => "bad-level-dat",
            Self::UnwritableLevelDat { .. } => "unwritable-level-dat",
            Self::UnreadableWorld { .. } => "unreadable-world",
            Self::BadConfig { .. } => "bad-config",
            Self::InvalidPath { .. } => "invalid-path",
            Self::BadPack { .. } => "bad-pack",
            Self::BadStructureFile { .. } => "bad-structure-file",
            Self::MergeRefused { .. } => "merge-refused",
            Self::ConstructNotInstalled { .. } => "construct-not-installed",
            Self::IncompleteInstall { .. } => "incomplete-install",
            Self::BadStructureName { .. } => "bad-structure-name",
            Self::Internal { .. } => "internal",
            Self::NoBackupDir => "no-backup-dir",
            Self::Network { .. } => "network",
            Self::RateLimited => "rate-limited",
            Self::AssetNotFound { .. } => "asset-not-found",
            Self::Io(..) => "io",
        }
    }
}

pub type Result<T> = std::result::Result<T, CoreError>;
