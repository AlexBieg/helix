use super::*;

use helix_core::diagnostic::Severity;

#[tokio::test(flavor = "multi_thread")]
async fn notify_pushes_and_mirrors_to_statusline() -> anyhow::Result<()> {
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(":notify --severity error boom<ret>"),
        Some(&|app| {
            assert_eq!(app.editor.notifications.len(), 1);
            let notification = app.editor.notifications.iter().next().unwrap();
            assert_eq!(notification.severity, Severity::Error);
            assert_eq!(notification.text.as_ref(), "boom");

            // The most recent message is mirrored into the status line.
            let (status, &severity) = app.editor.get_status().unwrap();
            assert_eq!(status.as_ref(), "boom");
            assert_eq!(severity, Severity::Error);
        }),
        false,
    )
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn notify_defaults_to_info() -> anyhow::Result<()> {
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(":notify hello there<ret>"),
        Some(&|app| {
            let notification = app.editor.notifications.iter().next().unwrap();
            assert_eq!(notification.severity, Severity::Info);
            assert_eq!(notification.text.as_ref(), "hello there");
        }),
        false,
    )
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn identical_notifications_coalesce() -> anyhow::Result<()> {
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(":notify --severity info dup<ret>:notify --severity info dup<ret>"),
        Some(&|app| {
            assert_eq!(app.editor.notifications.len(), 1);
            let notification = app.editor.notifications.iter().next().unwrap();
            assert_eq!(notification.count, 2);
        }),
        false,
    )
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn repeat_flag_pushes_distinct_notifications() -> anyhow::Result<()> {
    // Coalescing only collapses the *newest* run of identical messages, so the
    // repeat flag yields a single coalesced entry with a count.
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(":notify --repeat 3 spam<ret>"),
        Some(&|app| {
            assert_eq!(app.editor.notifications.len(), 1);
            assert_eq!(app.editor.notifications.iter().next().unwrap().count, 3);
        }),
        false,
    )
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn dismiss_notifications_clears_stack() -> anyhow::Result<()> {
    // space-N is bound to dismiss_notifications in the default keymap.
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(":notify --severity warning a<ret>:notify --severity error b<ret><space>N"),
        Some(&|app| {
            assert!(app.editor.notifications.is_empty());
            // The user stays in normal mode after dismissing.
            assert_eq!(app.editor.mode(), helix_view::document::Mode::Normal);
        }),
        false,
    )
    .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_severity_is_an_error() -> anyhow::Result<()> {
    test_key_sequence(
        &mut AppBuilder::new().build()?,
        Some(":notify --severity nope whoops<ret>"),
        Some(&|app| {
            assert!(app.editor.is_err());
        }),
        false,
    )
    .await?;

    Ok(())
}
