local must_fail = require("modules/assert").must_fail

local dir = package.path:match("^(.-)/%?%.lua;")

must_fail("failed to load", function()
  return font("missing.ttf")
end)

local file = dir .. "/not-a-font.ttf"
local out = assert(io.open(file, "wb"))
out:write("this is not a font file")
out:close()

must_fail("not a valid font", function()
  return font("not-a-font.ttf")
end)

assert(os.remove(file), "the junk file is cleaned up")

return {
  render = function()
  end,
}
