if vim.g.loaded_markdown_render == 1 then
    return
end

vim.g.loaded_markdown_render = 1

if vim.g.markdown_render_disable_auto_setup == true then
    return
end

local ok, markdown_render = pcall(require, "markdown_render")
if not ok then
    vim.schedule(function()
        vim.notify(
            "[markdown-render] failed to load module: " .. tostring(markdown_render),
            vim.log.levels.ERROR
        )
    end)
    return
end

local opts = vim.g.markdown_render
if opts ~= nil and type(opts) ~= "table" then
    opts = nil
end

local setup_ok, setup_err = pcall(markdown_render.setup, opts)
if not setup_ok then
    vim.schedule(function()
        vim.notify(
            "[markdown-render] setup failed: " .. tostring(setup_err),
            vim.log.levels.ERROR
        )
    end)
end
