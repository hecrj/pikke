local face = font("../../fonts/NotoSans-Regular.ttf")

local layout = text("Noto", { font = face, size = 100 })
assert(layout.x == 0, layout.x)
assert(layout.y == 0, layout.y)
assert(layout.width > 0, "width")
assert(layout.height > 0, "height")

layout:translate({ x = 7, y = 3 })
assert(layout.x == 7, layout.x)
assert(layout.y == 3, layout.y)

local bounds = layout.bounds
assert(bounds.x == 7, bounds.x)
assert(bounds.y == 3, bounds.y)
assert(math.abs(bounds.width - layout.width) < 0.000001, "the bounds span the ink box")

local empty = text("", { font = face, size = 100 })
assert(empty.width == 0, "an empty string is degenerate")
assert(empty.height == 0, "an empty string is degenerate")

local ok, error = pcall(text, "Noto", { size = 100 })
assert(not ok, "a missing font must fail")
assert(tostring(error):find("font") ~= nil, tostring(error))

return {
  render = function()
  end,
}
