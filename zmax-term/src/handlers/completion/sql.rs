//! SQL keyword completion inside injected SQL strings (JetBrains-style language
//! injection). When the tree-sitter injection layer at the cursor resolves to
//! `sql` — e.g. a `sql`-tagged template literal, a `/* sql */`-hinted string, or
//! a string passed to a `query`/`execute`/… method (see the SQL rules in
//! `runtime/queries/ecma/injections.scm`, and the built-in sql injections for
//! Rust/PHP/etc.) — this source offers SQL keyword completions.
//!
//! It is intentionally lightweight: a static keyword list, not a schema-aware
//! SQL language server. It mirrors the word-completion source
//! ([`super::word`]) in shape and lifecycle.

use std::{borrow::Cow, sync::Arc};

use zmax_core::{self as core, chars::char_is_word, completion::CompletionProvider, Transaction};
use zmax_event::TaskHandle;
use zmax_view::{document::SavePoint, handlers::completion::ResponseContext, Editor};

use super::{request::TriggerKind, CompletionItem, CompletionItems, CompletionResponse, Trigger};

const COMPLETION_KIND: &str = "sql";

/// Common SQL keywords/clauses/types/functions offered as completions.
const SQL_KEYWORDS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "INSERT INTO",
    "VALUES",
    "UPDATE",
    "SET",
    "DELETE",
    "CREATE TABLE",
    "ALTER TABLE",
    "DROP TABLE",
    "TRUNCATE",
    "JOIN",
    "INNER JOIN",
    "LEFT JOIN",
    "RIGHT JOIN",
    "FULL JOIN",
    "CROSS JOIN",
    "ON",
    "USING",
    "GROUP BY",
    "ORDER BY",
    "HAVING",
    "LIMIT",
    "OFFSET",
    "DISTINCT",
    "AS",
    "AND",
    "OR",
    "NOT",
    "NULL",
    "IS",
    "IN",
    "BETWEEN",
    "LIKE",
    "ILIKE",
    "EXISTS",
    "UNION",
    "UNION ALL",
    "INTERSECT",
    "EXCEPT",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "END",
    "ASC",
    "DESC",
    "PRIMARY KEY",
    "FOREIGN KEY",
    "REFERENCES",
    "DEFAULT",
    "UNIQUE",
    "INDEX",
    "CONSTRAINT",
    "CASCADE",
    "RETURNING",
    "WITH",
    "COUNT",
    "SUM",
    "AVG",
    "MIN",
    "MAX",
    "COALESCE",
    "NULLIF",
    "CAST",
    "NOW",
    "CURRENT_TIMESTAMP",
    "INTEGER",
    "BIGINT",
    "SERIAL",
    "VARCHAR",
    "TEXT",
    "BOOLEAN",
    "TIMESTAMP",
    "DATE",
    "NUMERIC",
    "TRUE",
    "FALSE",
];

pub(super) fn completion(
    editor: &Editor,
    trigger: Trigger,
    handle: TaskHandle,
    savepoint: Arc<SavePoint>,
) -> Option<impl FnOnce() -> CompletionResponse> {
    let (view, doc) = current_ref!(editor);
    let text = doc.text().slice(..);
    let selection = doc.selection(view.id).clone();
    let pos = selection.primary().cursor(text);
    if pos == 0 {
        return None;
    }

    // Only fire inside a tree-sitter-injected SQL region. Detect at the char just
    // before the cursor (the text being typed), which is inside the injection.
    let byte = text.char_to_byte(pos.saturating_sub(1));
    let is_sql = {
        let loader = editor.syn_loader.load();
        doc.language_config_at(&loader, byte)
            .map(|c| c.language_id.as_str())
            == Some("sql")
    };
    if !is_sql {
        return None;
    }

    // The identifier prefix typed so far (letters/digits/underscore before cursor).
    let mut start = pos;
    while start > 0 && char_is_word(text.char(start - 1)) {
        start -= 1;
    }
    let prefix: String = text.slice(start..pos).chars().collect();
    // Avoid noise: only auto-fire once a prefix has been typed; a manual trigger
    // (e.g. C-x C-o) with no prefix lists everything.
    if prefix.is_empty() && trigger.kind != TriggerKind::Manual {
        return None;
    }
    let prefix_upper = prefix.to_ascii_uppercase();
    let edit_diff = pos - start;

    if handle.is_canceled() {
        return None;
    }

    let rope = doc.text().clone();

    let future = move || {
        let text = rope.slice(..);
        let items = SQL_KEYWORDS
            .iter()
            .filter(|kw| prefix_upper.is_empty() || kw.starts_with(prefix_upper.as_str()))
            .filter(|kw| !kw.eq_ignore_ascii_case(&prefix))
            .map(|&kw| {
                let transaction = Transaction::change_by_selection(&rope, &selection, |range| {
                    let cursor = range.cursor(text);
                    (cursor - edit_diff, cursor, Some(kw.into()))
                });
                CompletionItem::Other(core::CompletionItem {
                    transaction,
                    label: Cow::Borrowed(kw),
                    kind: Cow::Borrowed(COMPLETION_KIND),
                    documentation: None,
                    provider: CompletionProvider::Sql,
                })
            })
            .collect();

        CompletionResponse {
            items: CompletionItems::Other(items),
            provider: CompletionProvider::Sql,
            context: ResponseContext {
                is_incomplete: false,
                priority: 0,
                savepoint,
            },
        }
    };

    Some(future)
}
