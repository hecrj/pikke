local all = rectangle({ size = { 4, 4 } })

local solid = blank({ width = 4, height = 4, color = "#808080" })
assert(solid:coverage_in(all) == 16, solid:coverage_in(all))
assert(solid:coverage_in(rectangle({ x = -2, y = -2, size = { 6, 6 } })) == 16, "clipped to the image")

local clear = blank({ width = 4, height = 4 })
assert(clear:coverage_in(all) == 0, clear:coverage_in(all))
assert(clear:coverage_in(rectangle({ x = 10, y = 0, size = 4 })) == 0, "outside the image")

local quarter = blank({ width = 4, height = 4 })
quarter:fill(rectangle({ size = { 2, 2 } }), "#808080")
assert(quarter:coverage_in(all) == 4, quarter:coverage_in(all))
assert(quarter:coverage_in(rectangle({ x = 2, y = 2, size = 2 })) == 0, "the unfilled corner")
assert(quarter:coverage_in(rectangle({ x = 1, y = 1, size = 2 })) == 1, "the overlap")

local half = blank({ width = 4, height = 4 })
local paint = solid:dominant_in(all):to_oklch()
paint.a = 0.5
half:fill(all, paint)
assert(half:coverage_in(all) == 16, "a nonzero alpha is coverage")

assert(solid:coverage_in(rectangle({ size = 0 })) == 0, "a zero-size region")
assert(solid:coverage_in(rectangle({ size = { 4, -4 } })) == 0, "a negative-size region")

return {
  render = function()
  end,
}
