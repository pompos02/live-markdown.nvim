use super::{LiveMarkdownPlugin, PluginError};
use crate::session::BufferSnapshot;

pub async fn live_markdown_start(
    plugin: &LiveMarkdownPlugin,
    snapshot: BufferSnapshot,
) -> Result<String, PluginError> {
    plugin.start_preview(snapshot).await
}

pub async fn live_markdown_stop(
    plugin: &LiveMarkdownPlugin,
    bufnr: i64,
) -> Result<bool, PluginError> {
    plugin.stop_preview(bufnr).await
}

pub async fn live_markdown_toggle(
    plugin: &LiveMarkdownPlugin,
    snapshot: BufferSnapshot,
) -> Result<Option<String>, PluginError> {
    plugin.toggle_preview(snapshot).await
}

pub async fn live_markdown_open(
    plugin: &LiveMarkdownPlugin,
    bufnr: i64,
) -> Result<Option<String>, PluginError> {
    plugin.open_preview(bufnr).await
}
