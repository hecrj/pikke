local source = blank({ width = 4, height = 4, color = "#C81E3C" })
local region = rectangle({ size = { 4, 4 } })

local paint = source:dominant_in(region)
local target = blank({ width = 4, height = 4 })
target:fill(region, paint)

local pixel = target:pixel(0, 0)
assert(pixel.r == 200, pixel.r)
assert(pixel.g == 30, pixel.g)
assert(pixel.b == 60, pixel.b)
assert(pixel.a == 255, pixel.a)

local probe = blank({ width = 4, height = 4, color = "#FF0000" })
probe:fill(
  region,
  source:dominant_in(rectangle({ x = 10, y = 10, size = { 2, 2 } }))
)
assert(probe:pixel(0, 0).r == 255, "a transparent fill is a no-op")

local clear = blank({ width = 4, height = 4 })
probe:fill(region, clear:dominant_in(region))
assert(probe:pixel(0, 0).r == 255, "a transparent fill is a no-op")

return {
  render = function()
  end,
}
