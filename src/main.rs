use live_markdown_native::plugin::LiveMarkdownPlugin;
use live_markdown_native::plugin::commands::live_markdown_start;
use live_markdown_native::server::ServerConfig;
use live_markdown_native::session::BufferSnapshot;
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

    let source_path = args.get(1).cloned();
    let markdown = if let Some(path) = args.get(1) {
        fs::read_to_string(path)?
    } else {
        String::from("# Live Markdown\n\nOpen a file path argument to preview file contents.")
    };

    let plugin = LiveMarkdownPlugin::new(ServerConfig::default());
    let url = live_markdown_start(
        &plugin,
        BufferSnapshot {
            bufnr: 1,
            changedtick: 1,
            markdown,
            cursor_line: 1,
            cursor_col: 0,
            source_path,
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
    println!("live-markdown.nvim [path/to/file.md]");
    println!("Starts preview server and serves live markdown snapshot for the provided file.");
}
