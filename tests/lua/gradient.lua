local must_fail = require("modules/assert").must_fail

local black_to_white = gradient(90):stop(0, "#000000"):stop(1, "#FFFFFF")

local band = blank({ width = 100, height = 1 })
band:fill(rectangle({ size = { 100, 1 } }), black_to_white)

local west = band:pixel(0, 0)
assert(west.r == 16 and west.g == 16 and west.b == 16, west.r)
assert(west.a == 255, west.a)
local middle = band:pixel(50, 0)
assert(middle.r == 188 and middle.g == 188 and middle.b == 188, middle.r)
local east = band:pixel(99, 0)
assert(east.r == 254 and east.g == 254 and east.b == 254, east.r)

local white_to_black = gradient(0):stop(0, "#000000"):stop(1, "#FFFFFF")
local column = blank({ width = 1, height = 100 })
column:fill(rectangle({ size = { 1, 100 } }), white_to_black)

assert(column:pixel(0, 0).r == 254, "the last stop faces the angle")
assert(column:pixel(0, 99).r == 16, "the first stop faces the opposite side")

local red_to_black = gradient(90):stop(0, srgb(1, 0, 0)):stop(1, srgb(0, 0, 0))
local strip = blank({ width = 2, height = 1 })
strip:fill(rectangle({ size = { 2, 1 } }), red_to_black)

local west_end = strip:pixel(0, 0)
assert(west_end.r == 225, west_end.r)
assert(west_end.g == 0 and west_end.b == 0, "the stops keep the hue")
assert(west_end.a == 255, west_end.a)
assert(strip:pixel(1, 0).r == 137, "the color advances toward the angle")

local face = font("../../fonts/NotoSans-Regular.ttf")
local layout = text("Noto", { font = face, size = 100 })
local box = layout.bounds
local page = blank({ width = box.width + 4, height = box.height + 4 })
page:fill(layout, black_to_white)

local half = box.width / 2
local left_luma = page:luma_in(rectangle({ x = box.x, y = box.y, size = { half, box.height } }))
local right_luma = page:luma_in(
  rectangle({ x = box.x + half, y = box.y, size = { half, box.height } })
)
assert(right_luma > left_luma, "the gradient crosses the ink")

must_fail("stop: 'offset' must be between 0 and 1", function()
  gradient(90):stop(-0.5, "#000000")
end)
must_fail("stop: 'offset' must be finite", function()
  gradient(90):stop(math.huge, "#000000")
end)
must_fail("gradient: 'angle' is not a number", function()
  gradient({ 90 })
end)
must_fail("a color or a `Gradient`", function()
  blank({ width = 1, height = 1 }):fill(rectangle({ size = 1 }), {})
end)

return {
  render = function()
  end,
}
