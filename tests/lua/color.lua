local must_fail = require("modules/assert").must_fail

local all = rectangle({ size = { 2, 2 } })
local gray = blank({ width = 2, height = 2, color = "#808080" }):dominant_in(all)

local function sample(color)
  local image = blank({ width = 1, height = 1 })
  image:fill(rectangle({ size = { 1, 1 } }), color)
  return image:pixel(0, 0)
end

local even = sample(gray:with_luma(0.215744))
assert(even.r == 128, even.r)
assert(even.g == 128, even.g)
assert(even.b == 128, even.b)
assert(even.a == 255, even.a)

local flat = sample(gray:with_luma(0.0))
assert(flat.r == 0 and flat.g == 0 and flat.b == 0, "the zero target is black")

local white = blank({ width = 2, height = 2, color = "#FFFFFF" }):dominant_in(all)
local bright = sample(white:with_luma(2.0))
assert(bright.r == 255 and bright.g == 255 and bright.b == 255, "the target clamps to one")
local dark = sample(white:with_luma(-1.0))
assert(dark.r == 0 and dark.g == 0 and dark.b == 0, "the target clamps to zero")

must_fail("with_luma: 'luma' is not a number", function()
  gray:with_luma("x")
end)
must_fail("with_luma: 'luma' must be finite", function()
  gray:with_luma(math.huge)
end)

return {
  render = function()
  end,
}
