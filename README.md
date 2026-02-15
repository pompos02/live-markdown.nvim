# live-markdown.nvim

Real-time Markdown preview for Neovim, powered by Rust.

`live-markdown.nvim` starts a local HTTP server, renders your active Markdown buffer, and keeps the browser preview synced with both edits and cursor movement.

## Features

- Live preview without saving files
- Cursor-synced scrolling
- Local image rendering from markdown-relative paths
- Smooth auto-scroll cursor following
- SSE updates with reconnect-safe snapshot flow
- `:LiveMarkdownFollow`, `:LiveMarkdownStop`, `:LiveMarkdownToggle`, `:LiveMarkdownOpen`

## Requirements

- Neovim `>= 0.10`
- Cargo (*if you want to build it yourself*)

## Install

The repo comes bundled with the linux Binary, if you want to build it yourself run the build helper so the native module is copied to `lua/live_markdown_native.so`.

### lazy.nvim example

```lua
{
  "pompos02/live-markdown.nvim",
  build = "./scripts/build-nvim-module.sh release",
  config = function()
    require("live_markdown").setup({
      port = 6419,
      debounce_ms_content = 100,
      throttle_ms_cursor = 24,
      bind_address = "127.0.0.1",
      auto_scroll = true,
      scroll_comfort_top = 0.25,
      scroll_comfort_bottom = 0.65,
    })
  end,
}
```

## Commands

- `:LiveMarkdownStop` - stop preview for current buffer
- `:LiveMarkdownStop!` - stop all active preview sessions
- `:LiveMarkdownToggle` - toggle preview for current buffer (starts in follow mode)
- `:LiveMarkdownOpen` - print current preview URL
- `:LiveMarkdownFollow` - start preview and keep the browser synced to markdown buffer switches (default behavior)


## Development

```bash
cargo fmt
cargo test
./scripts/build-nvim-module.sh release
```

Then add this repo root to `runtimepath` and open Neovim.
