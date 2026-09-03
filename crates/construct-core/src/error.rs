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
    WorldInUse { world: PathBuf },

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

    #[error("not a usable pack at {}: {reason}", path.display())]
    BadPack { path: PathBuf, reason: String },

    #[error("Construct is not installed")]
    ConstructNotInstalled { searched: Vec<PathBuf> },

    #[error("unusable structure name {name:?}: {reason}")]
    BadStructureName { name: String, reason: String },

    #[error("{what} is not implemented yet")]
    NotImplemented { what: String },

    #[error("no platform data directory for backups; set [backups] dir in config.toml")]
    NoBackupDir,

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, CoreError>;
