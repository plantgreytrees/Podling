use std::path::PathBuf;

/// Every failure the core can report. Library code returns this; the CLI adds
/// human context with `anyhow`.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("I/O error at {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("JSON serialisation failed")]
    Json(#[from] serde_json::Error),

    #[error("source {path}: {message}")]
    Source { path: PathBuf, message: String },

    #[error("configuration error: {message}")]
    Config { message: String },

    #[error("provider {plugin} failed: {message}")]
    Provider {
        plugin: String,
        kind: ProviderFailure,
        message: String,
    },

    #[error("stage {stage} got invalid output from its provider: {message}")]
    InvalidProviderOutput {
        stage: &'static str,
        message: String,
    },

    #[error("stage {stage} failed")]
    Stage {
        stage: &'static str,
        #[source]
        source: Box<CoreError>,
    },
}

/// What kind of provider failure it was, so callers can react (say, suggest
/// a fix) without parsing the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderFailure {
    /// The server answered with this non-2xx status.
    Http(u16),
    /// No answer: connection refused, DNS failure, TLS error and the like.
    Unreachable,
    TimedOut,
    /// The reply stopped at the output token limit before it was finished.
    /// The LLM stages treat it as a rejected reply, not a failure.
    CutOff,
    /// Anything else, such as a reply too large or not shaped like the API.
    Other,
}

impl CoreError {
    /// The provider failure behind this error, looking through stage wrappers.
    pub fn provider_failure(&self) -> Option<ProviderFailure> {
        self.provider().map(|(_, kind)| kind)
    }

    /// Which plugin failed and how, looking through stage wrappers.
    pub fn provider(&self) -> Option<(&str, ProviderFailure)> {
        match self {
            Self::Provider { plugin, kind, .. } => Some((plugin, *kind)),
            Self::Stage { source, .. } => source.provider(),
            _ => None,
        }
    }

    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}

pub type Result<T, E = CoreError> = std::result::Result<T, E>;
