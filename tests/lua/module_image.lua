local must_fail = require("modules/assert").must_fail

local lena = require("modules/lena")

assert(lena.width == 512, lena.width)

return {
  render = function()
  end,
}
