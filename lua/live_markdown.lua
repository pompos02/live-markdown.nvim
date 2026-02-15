local M = {}

local native = nil

local function core()
    if native ~= nil then
        return native
    end

    native = require("live_markdown_native")
    return native
end

function M.setup(opts)
    return core().setup(opts or {})
end

function M.stop(all)
    return core().stop(all)
end

function M.toggle()
    return core().toggle()
end

function M.open()
    return core().open()
end

function M.follow()
    return core().follow()
end

function M.shutdown()
    return core().shutdown()
end

return M
