# markdown-render

Real-time Markdown preview for Neovim, powered by Rust.

`markdown-render` starts a local HTTP server, renders your active Markdown buffer, and keeps the browser preview synced with both edits and cursor movement.

## Features

- Live preview without saving files
- Cursor-synced scrolling
- Local image rendering from markdown-relative paths
- Smooth auto-scroll cursor following
- SSE updates with reconnect-safe snapshot flow
- `:MarkdownRenderStart`, `:MarkdownRenderStop`, `:MarkdownRenderToggle`, `:MarkdownRenderOpen`

## Requirements

- Neovim `>= 0.10`
- Cargo (*if you want to build it yourself*)

## Install

The repo comes bundled with the Binary, if you want to build it yourself run the build helper so the native module is copied to `lua/markdown_render_native.so`.

### lazy.nvim example

```lua
{
  "pompos02/markdown-render",
  build = "./scripts/build-nvim-module.sh release",
  config = function()
    require("markdown_render").setup({
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

- `:MarkdownRenderStart` - start preview for current buffer
- `:MarkdownRenderStop` - stop preview for current buffer
- `:MarkdownRenderStop!` - stop all active preview sessions
- `:MarkdownRenderToggle` - toggle preview for current buffer
- `:MarkdownRenderOpen` - print current preview URL


## Development

```bash
cargo fmt
cargo test
./scripts/build-nvim-module.sh release
```

Then add this repo root to `runtimepath` and open Neovim.
