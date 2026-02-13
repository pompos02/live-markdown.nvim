use super::{MarkdownRenderPlugin, PluginError};
use crate::session::BufferSnapshot;

pub async fn markdown_render_start(
    plugin: &MarkdownRenderPlugin,
    snapshot: BufferSnapshot,
) -> Result<String, PluginError> {
    plugin.start_preview(snapshot).await
}

pub async fn markdown_render_stop(
    plugin: &MarkdownRenderPlugin,
    bufnr: i64,
) -> Result<bool, PluginError> {
    plugin.stop_preview(bufnr).await
}

pub async fn markdown_render_toggle(
    plugin: &MarkdownRenderPlugin,
    snapshot: BufferSnapshot,
) -> Result<Option<String>, PluginError> {
    plugin.toggle_preview(snapshot).await
}

pub async fn markdown_render_open(
    plugin: &MarkdownRenderPlugin,
    bufnr: i64,
) -> Result<Option<String>, PluginError> {
    plugin.open_preview(bufnr).await
}
