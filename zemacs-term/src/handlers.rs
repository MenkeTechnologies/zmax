use std::sync::Arc;

use arc_swap::ArcSwap;
use diagnostics::PullAllDocumentsDiagnosticHandler;
use zemacs_event::AsyncHook;

use crate::config::Config;
use crate::events;
use crate::handlers::auto_save::AutoSaveHandler;
use crate::handlers::diagnostics::PullDiagnosticsHandler;
use crate::handlers::signature_help::SignatureHelpHandler;

pub use zemacs_view::handlers::{word_index, Handlers};

use self::document_colors::DocumentColorsHandler;
use self::document_links::DocumentLinksHandler;

mod ai_ghost;
mod auto_save;
mod closed_files;
mod code_action_hint;
pub mod completion;
pub mod diagnostics;
mod document_colors;
mod document_highlight;
mod document_links;
mod prompt;
mod recent_files;
mod signature_help;
mod snippet;
mod workspace_trust;

pub fn setup(config: Arc<ArcSwap<Config>>) -> Handlers {
    events::register();

    let event_tx = completion::CompletionHandler::new(config).spawn();
    let signature_hints = SignatureHelpHandler::new().spawn();
    let auto_save = AutoSaveHandler::new().spawn();
    let code_action_hint = code_action_hint::Handler::default().spawn();
    let document_colors = DocumentColorsHandler::default().spawn();
    let document_links = DocumentLinksHandler::default().spawn();
    let word_index = word_index::Handler::spawn();
    let pull_diagnostics = PullDiagnosticsHandler::default().spawn();
    let pull_all_documents_diagnostics = PullAllDocumentsDiagnosticHandler::default().spawn();
    let ai_ghost = ai_ghost::GhostHandler::new().spawn();

    let handlers = Handlers {
        completions: zemacs_view::handlers::completion::CompletionHandler::new(event_tx),
        signature_hints,
        auto_save,
        document_colors,
        document_links,
        word_index,
        pull_diagnostics,
        pull_all_documents_diagnostics,
        code_action_hint,
        ai_ghost,
    };

    zemacs_view::handlers::register_hooks(&handlers);
    ai_ghost::register_hooks(&handlers);
    completion::register_hooks(&handlers);
    signature_help::register_hooks(&handlers);
    document_highlight::register_hooks(&handlers);
    code_action_hint::register_hooks(&handlers);
    auto_save::register_hooks(&handlers);
    diagnostics::register_hooks(&handlers);
    snippet::register_hooks(&handlers);
    document_colors::register_hooks(&handlers);
    document_links::register_hooks(&handlers);
    prompt::register_hooks(&handlers);
    workspace_trust::register_hooks(&handlers);
    recent_files::register_hooks(&handlers);
    crate::vim_modeline::register_hooks();
    closed_files::register_hooks(&handlers);
    handlers
}
