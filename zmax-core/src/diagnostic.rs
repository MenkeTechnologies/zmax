//! LSP diagnostic utility types.
use std::{fmt, sync::Arc};

use serde::{Deserialize, Serialize};
pub use zmax_stdx::range::Range;

/// Describes the severity level of a [`Diagnostic`].
#[derive(Debug, Clone, Copy, Eq, PartialEq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum Severity {
    #[default]
    Hint,
    Info,
    Warning,
    Error,
}

#[derive(Debug, Eq, Hash, PartialEq, Clone, Deserialize, Serialize)]
pub enum NumberOrString {
    Number(i32),
    String(String),
}

#[derive(Debug, Clone)]
pub enum DiagnosticTag {
    Unnecessary,
    Deprecated,
}

/// Corresponds to [`lsp_types::Diagnostic`](https://docs.rs/lsp-types/0.94.0/lsp_types/struct.Diagnostic.html)
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub range: Range,
    // whether this diagnostic ends at the end of(or inside) a word
    pub ends_at_word: bool,
    pub starts_at_word: bool,
    pub zero_width: bool,
    pub line: usize,
    pub message: String,
    pub severity: Option<Severity>,
    pub code: Option<NumberOrString>,
    pub provider: DiagnosticProvider,
    pub tags: Vec<DiagnosticTag>,
    pub source: Option<String>,
    pub data: Option<serde_json::Value>,
}

/// The source of a diagnostic.
///
/// This type is cheap to clone: all data is either `Copy` or wrapped in an `Arc`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiagnosticProvider {
    Lsp {
        /// The ID of the language server which sent the diagnostic.
        server_id: LanguageServerId,
        /// An optional identifier under which diagnostics are managed by the client.
        ///
        /// `identifier` is a field from the LSP "Pull Diagnostics" feature meant to provide an
        /// optional "namespace" for diagnostics: a language server can respond to a diagnostics
        /// pull request with an identifier and these diagnostics should be treated as separate
        /// from push diagnostics. Rust-analyzer uses this feature for example to provide Cargo
        /// diagnostics with push and internal diagnostics with pull. The push diagnostics should
        /// not clear the pull diagnostics and vice-versa.
        identifier: Option<Arc<str>>,
    },
    // Future internal features can go here...
}

impl DiagnosticProvider {
    pub fn language_server_id(&self) -> Option<LanguageServerId> {
        match self {
            Self::Lsp { server_id, .. } => Some(*server_id),
            // _ => None,
        }
    }
}

// while I would prefer having this in zmax-lsp that necessitates a bunch of
// conversions I would rather not add. I think its fine since this just a very
// trivial newtype wrapper and we would need something similar once we define
// completions in core
slotmap::new_key_type! {
    pub struct LanguageServerId;
}

impl fmt::Display for LanguageServerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl Diagnostic {
    #[inline]
    pub fn severity(&self) -> Severity {
        self.severity.unwrap_or(Severity::Warning)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diagnostic(severity: Option<Severity>) -> Diagnostic {
        Diagnostic {
            range: Range { start: 0, end: 1 },
            ends_at_word: false,
            starts_at_word: false,
            zero_width: false,
            line: 0,
            message: String::new(),
            severity,
            code: None,
            provider: DiagnosticProvider::Lsp {
                server_id: LanguageServerId::default(),
                identifier: None,
            },
            tags: Vec::new(),
            source: None,
            data: None,
        }
    }

    /// `Severity` derives `Ord` from its declaration order, and callers take the
    /// `max` to decide what a line's gutter and the statusline show. Reordering
    /// the variants -- alphabetically, say -- would compile cleanly and quietly
    /// rank an error below a hint.
    #[test]
    fn severity_orders_hint_below_error() {
        assert!(Severity::Hint < Severity::Info);
        assert!(Severity::Info < Severity::Warning);
        assert!(Severity::Warning < Severity::Error);

        let worst = [Severity::Info, Severity::Error, Severity::Hint]
            .into_iter()
            .max();
        assert_eq!(worst, Some(Severity::Error));
    }

    /// The two defaults in this file deliberately disagree, which reads as a bug
    /// until you know it is not: an absent `severity` field deserializes to
    /// `Hint`, but a `Diagnostic` whose severity the server left unset is treated
    /// as a `Warning`, so unlabelled diagnostics stay visible.
    #[test]
    fn an_unset_severity_reads_as_a_warning_though_the_type_defaults_to_hint() {
        assert_eq!(Severity::default(), Severity::Hint);
        assert_eq!(diagnostic(None).severity(), Severity::Warning);
        assert_eq!(diagnostic(Some(Severity::Hint)).severity(), Severity::Hint);
    }

    /// The serialized names are lowercase and reach user config and session
    /// files; renaming a variant without its serde name would silently stop
    /// matching what users have written.
    #[test]
    fn severities_serialize_under_their_lowercase_names() {
        for (severity, name) in [
            (Severity::Hint, "\"hint\""),
            (Severity::Info, "\"info\""),
            (Severity::Warning, "\"warning\""),
            (Severity::Error, "\"error\""),
        ] {
            assert_eq!(serde_json::to_string(&severity).unwrap(), name);
            assert_eq!(
                serde_json::from_str::<Severity>(name).unwrap(),
                severity,
                "{name} does not come back"
            );
        }

        assert!(serde_json::from_str::<Severity>("\"Error\"").is_err());
    }

    /// Every provider today is a language server, but the accessor exists so
    /// future non-LSP sources return `None` rather than a bogus id -- and the
    /// pull-diagnostics `identifier` is not part of the id.
    #[test]
    fn the_provider_reports_its_language_server_id() {
        let server_id = LanguageServerId::default();
        let pull = DiagnosticProvider::Lsp {
            server_id,
            identifier: Some("cargo".into()),
        };
        let push = DiagnosticProvider::Lsp {
            server_id,
            identifier: None,
        };

        assert_eq!(pull.language_server_id(), Some(server_id));
        assert_eq!(push.language_server_id(), Some(server_id));
        // Same server, different namespaces: they must not compare equal, or
        // push diagnostics would clear pull diagnostics.
        assert_ne!(pull, push);
    }
}
