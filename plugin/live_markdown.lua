if vim.g.loaded_live_markdown == 1 then
    return
end

vim.g.loaded_live_markdown = 1

if vim.g.live_markdown_disable_auto_setup == true then
    return
end

local ok, live_markdown = pcall(require, "live_markdown")
if not ok then
    vim.schedule(function()
        vim.notify(
            "[live-markdown.nvim] failed to load module: " .. tostring(live_markdown),
            vim.log.levels.ERROR
        )
    end)
    return
end

local opts = vim.g.live_markdown
if opts ~= nil and type(opts) ~= "table" then
    opts = nil
end

local setup_ok, setup_err = pcall(live_markdown.setup, opts)
if not setup_ok then
    vim.schedule(function()
        vim.notify(
            "[live-markdown.nvim] setup failed: " .. tostring(setup_err),
            vim.log.levels.ERROR
        )
    end)
end
