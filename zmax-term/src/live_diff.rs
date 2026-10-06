//! Buffers diffed against each other as they are edited — the JetBrains
//! blank diff window ("Open Blank Diff Window"), where two (or three) empty
//! editors sit side by side and their differences show as you type or paste.
//!
//! Each buffer of a linked group takes the text of the one before it as its
//! diff base (the first takes the second's), so the change gutter and
//! next/previous change work on every side. After each command and each
//! typed character the bases are brought up to date with any side whose text
//! changed.

use std::sync::Mutex;

use zmax_view::{DocumentId, Editor};

/// A linked group: its buffers, and the version of each the bases were last
/// taken from.
struct Group {
    docs: Vec<DocumentId>,
    versions: Vec<i32>,
}

static GROUPS: Mutex<Vec<Group>> = Mutex::new(Vec::new());

/// The buffer `docs[i]` is diffed against.
fn partner(i: usize) -> usize {
    if i == 0 {
        1
    } else {
        i - 1
    }
}

/// Diff `docs` against each other from now on. A buffer already in a group
/// leaves it.
pub fn link(editor: &mut Editor, docs: Vec<DocumentId>) {
    let mut groups = GROUPS.lock().unwrap_or_else(|e| e.into_inner());
    for group in groups.iter_mut() {
        group.docs.retain(|d| !docs.contains(d));
    }
    groups.retain(|g| g.docs.len() >= 2);
    let versions = vec![i32::MIN; docs.len()];
    groups.push(Group { docs, versions });
    drop(groups);
    sync(editor);
}

/// Add `doc` to the group `with` is in — JetBrains "Toggle Three-Side Mode"
/// adds a third editor. `false` when `with` is in none.
pub fn join(editor: &mut Editor, with: DocumentId, doc: DocumentId) -> bool {
    let mut groups = GROUPS.lock().unwrap_or_else(|e| e.into_inner());
    let Some(group) = groups.iter_mut().find(|g| g.docs.contains(&with)) else {
        return false;
    };
    group.docs.push(doc);
    group.versions.push(i32::MIN);
    drop(groups);
    sync(editor);
    true
}

/// Take `doc` out of its group, `false` when it is in none; a group left
/// with one buffer ends.
pub fn unlink(doc: DocumentId) -> bool {
    let mut groups = GROUPS.lock().unwrap_or_else(|e| e.into_inner());
    let mut found = false;
    for group in groups.iter_mut() {
        if let Some(i) = group.docs.iter().position(|d| *d == doc) {
            group.docs.remove(i);
            group.versions.remove(i);
            found = true;
        }
    }
    groups.retain(|g| g.docs.len() >= 2);
    found
}

/// Put `new` in `old`'s place in its group. `false` when `old` is in none.
pub fn replace(editor: &mut Editor, old: DocumentId, new: DocumentId) -> bool {
    let mut groups = GROUPS.lock().unwrap_or_else(|e| e.into_inner());
    let Some((group, i)) = groups
        .iter_mut()
        .find_map(|g| g.docs.iter().position(|d| *d == old).map(|i| (g, i)))
    else {
        return false;
    };
    group.docs[i] = new;
    group.versions = vec![i32::MIN; group.docs.len()];
    drop(groups);
    sync(editor);
    true
}

/// The group `doc` is in, if any.
pub fn group_of(doc: DocumentId) -> Option<Vec<DocumentId>> {
    GROUPS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|g| g.docs.contains(&doc))
        .map(|g| g.docs.clone())
}

/// Bring every diff base up to date with the buffers that changed since.
pub fn sync(editor: &mut Editor) {
    let mut groups = GROUPS.lock().unwrap_or_else(|e| e.into_inner());
    // A closed buffer leaves its group.
    for group in groups.iter_mut() {
        let keep: Vec<bool> = group
            .docs
            .iter()
            .map(|d| editor.document(*d).is_some())
            .collect();
        let mut k = keep.iter();
        group.docs.retain(|_| *k.next().unwrap());
        let mut k = keep.iter();
        group.versions.retain(|_| *k.next().unwrap());
    }
    groups.retain(|g| g.docs.len() >= 2);
    for group in groups.iter_mut() {
        let current: Vec<i32> = group
            .docs
            .iter()
            .map(|d| editor.document(*d).map_or(0, |doc| doc.version()))
            .collect();
        let n = group.docs.len();
        for i in 0..n {
            let source = partner(i);
            if current[source] == group.versions[source] && group.versions[i] != i32::MIN {
                continue;
            }
            let base = editor
                .document(group.docs[source])
                .map(|doc| doc.text().to_string().into_bytes())
                .unwrap_or_default();
            if let Some(doc) = editor.document_mut(group.docs[i]) {
                doc.set_diff_base(base);
            }
        }
        group.versions = current;
    }
}

pub fn register_hooks() {
    use crate::events::{PostCommand, PostInsertChar};
    use zmax_event::register_hook;
    register_hook!(move |event: &mut PostCommand<'_, '_>| {
        sync(event.cx.editor);
        Ok(())
    });
    register_hook!(move |event: &mut PostInsertChar<'_, '_>| {
        sync(event.cx.editor);
        Ok(())
    });
}

#[cfg(test)]
mod tests {
    use super::partner;

    #[test]
    fn each_side_is_diffed_against_the_one_before() {
        assert_eq!(1, partner(0));
        assert_eq!(0, partner(1));
        assert_eq!(1, partner(2), "a third side compares with the second");
    }
}
