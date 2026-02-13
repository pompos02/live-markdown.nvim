# markdown-render

Real-time Markdown preview for Neovim, powered by Rust.

`markdown-render` starts a local HTTP server, renders your active Markdown buffer, and keeps the browser preview synced with both edits and cursor movement.

## Features

- Live preview without saving files
- Cursor-synced scrolling via line anchors (`data-line`)
- SSE updates with reconnect-safe snapshot flow
- `:MarkdownRenderStart`, `:MarkdownRenderStop`, `:MarkdownRenderToggle`, `:MarkdownRenderOpen`
- Localhost-only server with per-session tokens for `/snapshot` and `/events`

## Requirements

- Neovim `>= 0.10`
- Rust toolchain (`cargo`)

## Install

Use your plugin manager and run the build helper so the native module is copied to `lua/markdown_render_native.so` (or `.dll` on Windows).

### lazy.nvim example

```lua
{
  "<you>/markdown-render",
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

You can also rely on auto-setup by setting `vim.g.markdown_render` before plugin load:

```lua
vim.g.markdown_render = {
  auto_scroll = true,
}
```

Disable auto-setup with:

```lua
vim.g.markdown_render_disable_auto_setup = true
```

## Commands

- `:MarkdownRenderStart` - start preview for current buffer
- `:MarkdownRenderStop` - stop preview for current buffer
- `:MarkdownRenderStop!` - stop all active preview sessions
- `:MarkdownRenderToggle` - toggle preview for current buffer
- `:MarkdownRenderOpen` - print current preview URL

## Configuration

`setup({...})` keys:

- `port` (`6419`)
- `debounce_ms_content` (`100`)
- `throttle_ms_cursor` (`24`)
- `bind_address` (`"127.0.0.1"`)
- `auto_scroll` (`true`)
- `scroll_comfort_top` (`0.25`)
- `scroll_comfort_bottom` (`0.65`)

## Development

```bash
cargo fmt
cargo test
./scripts/build-nvim-module.sh release
```

Then add this repo root to `runtimepath` and open Neovim.
