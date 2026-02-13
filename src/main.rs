use markdown_render_native::plugin::MarkdownRenderPlugin;
use markdown_render_native::plugin::commands::markdown_render_start;
use markdown_render_native::server::ServerConfig;
use markdown_render_native::session::BufferSnapshot;
use std::env;
use std::error::Error;
use std::fs;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args
        .get(1)
        .is_some_and(|arg| arg == "-h" || arg == "--help")
    {
        print_help();
        return Ok(());
    }

    let markdown = if let Some(path) = args.get(1) {
        fs::read_to_string(path)?
    } else {
        String::from("# Markdown Render\n\nOpen a file path argument to preview file contents.")
    };

    let plugin = MarkdownRenderPlugin::new(ServerConfig::default());
    let url = markdown_render_start(
        &plugin,
        BufferSnapshot {
            bufnr: 1,
            changedtick: 1,
            markdown,
            cursor_line: 1,
            cursor_col: 0,
        },
    )
    .await?;

    println!("Markdown preview running at: {url}");
    println!("Press Ctrl+C to stop.");

    tokio::signal::ctrl_c().await?;
    plugin.shutdown().await;
    Ok(())
}

fn print_help() {
    println!("markdown-render [path/to/file.md]");
    println!("Starts preview server and serves live markdown snapshot for the provided file.");
}
