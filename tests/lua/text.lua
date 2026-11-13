local must_fail = require("modules/assert").must_fail

local face = font("../../fonts/NotoSans-Regular.ttf")

local layout = text("Noto", { font = face, size = 100 })
assert(layout.x == 0, layout.x)
assert(layout.y == 0, layout.y)
assert(layout.width > 0, "the ink box is laid out")
assert(layout.height > 0, "the ink box is laid out")

local placed = text("Noto", { font = face, size = 100, x = 7, y = 3 })
assert(placed.x == 7, placed.x)
assert(placed.y == 3, placed.y)
assert(placed.bounds.x == 7, placed.bounds.x)
assert(placed.bounds.y == 3, placed.bounds.y)
assert(
  math.abs(placed.bounds.width - layout.width) < 0.000001,
  "the bounds match the laid-out width"
)

layout:translate({ x = 7, y = 3 })
assert(layout.x == 7, layout.x)
assert(layout.y == 3, layout.y)
assert(layout.bounds.y == 3, "the bounds follow the move")

layout:translate({ y = -5 })
assert(layout.x == 7, "a partial offset keeps the rest")
assert(layout.y == -2, layout.y)

layout:translate({})
assert(layout.y == -2, "an empty offset moves nothing")

local moved = text("Noto", { font = face, size = 100, x = 20, y = 40 })
moved:move_to({ x = 1.5, y = -2.5 })
assert(moved.x == 1.5, moved.x)
assert(moved.y == -2.5, moved.y)

moved:move_to({})
assert(moved.x == 0, "a missing x lands at zero")
assert(moved.y == 0, "a missing y lands at zero")

local empty = text("", { font = face, size = 100 })
assert(empty.width == 0, "an empty string is degenerate")
assert(empty.height == 0, "an empty string is degenerate")

must_fail("text: 'font' is missing or not a `Font`", function()
  return text("Noto", { size = 100 })
end)
must_fail("text: 'size' is missing or not a number", function()
  return text("Noto", { font = face })
end)
must_fail("text: 'size' is missing or not a number", function()
  return text("Noto", { font = face, size = "big" })
end)
must_fail("text: 'size' must be positive", function()
  return text("Noto", { font = face, size = -1 })
end)
must_fail("text: 'weight' is missing or not a number", function()
  return text("Noto", { font = face, size = 100, weight = "bold" })
end)
must_fail("text: 'x' is missing or not a number", function()
  return text("Noto", { font = face, size = 100, x = "left" })
end)
must_fail("text: 'y' must be finite", function()
  return text("Noto", { font = face, size = 100, y = math.huge })
end)

must_fail("translate: 'x' is not a number", function()
  layout:translate({ x = "left" })
end)
must_fail("translate: 'y' must be finite", function()
  layout:translate({ y = math.huge })
end)

must_fail("move_to: 'x' is not a number", function()
  layout:move_to({ x = "left" })
end)
must_fail("move_to: 'y' must be finite", function()
  layout:move_to({ y = math.huge })
end)

return {
  render = function()
  end,
}
