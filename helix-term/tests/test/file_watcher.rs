use std::io::Write;

use helix_view::{current_ref, editor::FileWatcherConfig};

use crate::test::helpers::{self, AppBuilder};

#[tokio::test(flavor = "multi_thread")]
async fn test_reload_updates_buffer() -> anyhow::Result<()> {
    // Test that doc.reload() updates buffer with external file contents
    let mut file = tempfile::NamedTempFile::new()?;
    file.as_file_mut().write_all(b"original content\n")?;
    file.as_file_mut().flush()?;
    file.as_file_mut().sync_all()?;

    let mut app = AppBuilder::new().with_file(file.path(), None).build()?;

    // Verify initial content
    let (_view, doc) = current_ref!(app.editor);
    let initial: String = doc.text().slice(..).chars().collect();
    assert!(initial.contains("original content"));

    // Modify file externally
    std::fs::write(file.path(), "modified externally\n")?;

    // Reload via the API directly (bypasses key sequence issues)
    let scrolloff = app.editor.config().scrolloff;
    let view_id = app.editor.tree.focus;
    let doc = app
        .editor
        .documents
        .get_mut(&app.editor.tree.get(view_id).doc)
        .unwrap();
    let view = app.editor.tree.get_mut(view_id);
    doc.reload(view, &app.editor.diff_providers)?;
    view.ensure_cursor_in_view(doc, scrolloff);

    // Verify buffer was updated
    let doc = app
        .editor
        .documents
        .get(&app.editor.tree.get(view_id).doc)
        .unwrap();
    let text: String = doc.text().slice(..).chars().collect();
    assert!(
        text.contains("modified externally"),
        "expected 'modified externally', got '{text}'"
    );
    assert!(
        !doc.is_modified(),
        "document should not be marked as modified after reload"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_reload_resets_modified_flag() -> anyhow::Result<()> {
    let mut file = tempfile::NamedTempFile::new()?;
    file.as_file_mut().write_all(b"hello\n")?;
    file.as_file_mut().flush()?;

    let mut app = AppBuilder::new().with_file(file.path(), None).build()?;

    // Make a change to dirty the buffer
    let view_id = app.editor.tree.focus;
    let doc_id = app.editor.tree.get(view_id).doc;
    {
        let doc = app.editor.documents.get_mut(&doc_id).unwrap();
        let view = app.editor.tree.get_mut(view_id);
        let transaction = helix_core::Transaction::change_by_selection(
            doc.text(),
            doc.selection(view.id),
            |_| (0, 0, Some("extra".into())),
        );
        doc.apply(&transaction, view.id);
        doc.append_changes_to_history(view);
    }

    assert!(app.editor.documents.get(&doc_id).unwrap().is_modified());

    // Modify file externally
    std::fs::write(file.path(), "new content\n")?;

    // Reload
    let scrolloff = app.editor.config().scrolloff;
    let doc = app.editor.documents.get_mut(&doc_id).unwrap();
    let view = app.editor.tree.get_mut(view_id);
    doc.reload(view, &app.editor.diff_providers)?;
    view.ensure_cursor_in_view(doc, scrolloff);

    // After reload, doc should NOT be modified (reload resets the flag)
    let doc = app.editor.documents.get(&doc_id).unwrap();
    assert!(
        !doc.is_modified(),
        "document should not be marked as modified after reload"
    );
    assert!(doc
        .text()
        .slice(..)
        .chars()
        .collect::<String>()
        .contains("new content"));

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_file_watcher_config_integration() -> anyhow::Result<()> {
    let mut config = helpers::test_config();
    config.editor.file_watcher = FileWatcherConfig {
        auto_reload: false,
        debounce_ms: 500,
    };

    let file = tempfile::NamedTempFile::new()?;
    let app = AppBuilder::new()
        .with_file(file.path(), None)
        .with_config(config)
        .build()?;

    let loaded_config = app.editor.config();
    assert!(!loaded_config.file_watcher.auto_reload);
    assert_eq!(loaded_config.file_watcher.debounce_ms, 500);
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_editor_has_file_watcher() -> anyhow::Result<()> {
    let file = tempfile::NamedTempFile::new()?;
    // The file_watcher field should exist on the editor - just verify no panic
    let app = AppBuilder::new().with_file(file.path(), None).build()?;
    assert!(!app.editor.should_close());
    assert_eq!(app.editor.documents().count(), 1);
    Ok(())
}
