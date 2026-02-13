local M = {}

local native = nil

local function core()
  if native ~= nil then
    return native
  end

  native = require("markdown_render_native")
  return native
end

function M.setup(opts)
  return core().setup(opts or {})
end

function M.start()
  return core().start()
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

function M.shutdown()
  return core().shutdown()
end

return M
