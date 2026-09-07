use construct_core::CoreError;

pub enum Failure {
    Core(CoreError),

    Usage { message: String, hint: String },

    AlreadyReported,
}

pub type Result<T = ()> = std::result::Result<T, Failure>;

impl Failure {
    pub fn usage(message: impl Into<String>, hint: impl Into<String>) -> Self {
        Failure::Usage {
            message: message.into(),
            hint: hint.into(),
        }
    }

    pub fn exit_code(&self) -> i32 {
        match self {
            Failure::Usage { .. } => 2,
            Failure::AlreadyReported => 1,
            Failure::Core(err) => match err {
                CoreError::AmbiguousWorld { .. }
                | CoreError::AmbiguousStructure { .. }
                | CoreError::AmbiguousInstallation { .. }
                | CoreError::MalformedReference { .. }
                | CoreError::BadStructureName { .. } => 2,
                _ => 1,
            },
        }
    }
}

impl From<CoreError> for Failure {
    fn from(err: CoreError) -> Self {
        Failure::Core(err)
    }
}

impl From<std::io::Error> for Failure {
    fn from(err: std::io::Error) -> Self {
        Failure::Core(CoreError::Io(err))
    }
}
