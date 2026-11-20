local source = blank({ width = 4, height = 4, color = "#C81E3C" })
local region = rectangle({ size = { 4, 4 } })

local target = blank({ width = 4, height = 4 })
target:fill(region, source:vivid_in(region))

local pixel = target:pixel(0, 0)
assert(pixel.r == 200, pixel.r)
assert(pixel.g == 30, pixel.g)
assert(pixel.b == 60, pixel.b)
assert(pixel.a == 255, pixel.a)

local field = blank({ width = 4, height = 4, color = "#808080" })
field:fill(rectangle({ x = 0, y = 0, size = { 2, 2 } }), "#004080")

local accent = blank({ width = 4, height = 4 })
accent:fill(region, field:vivid_in(region))

local accent_pixel = accent:pixel(0, 0)
assert(accent_pixel.r == 0, accent_pixel.r)
assert(accent_pixel.g == 64, accent_pixel.g)
assert(accent_pixel.b == 128, accent_pixel.b)
assert(accent_pixel.a == 255, accent_pixel.a)

local muted = blank({ width = 4, height = 4, color = "#808080" })
muted:fill(rectangle({ x = 0, y = 0, size = { 2, 2 } }), "#8A8478")

local standin = blank({ width = 4, height = 4 })
standin:fill(region, muted:vivid_in(region))

local standin_pixel = standin:pixel(0, 0)
assert(standin_pixel.r == 128, standin_pixel.r)
assert(standin_pixel.g == 128, standin_pixel.g)
assert(standin_pixel.b == 128, standin_pixel.b)
assert(standin_pixel.a == 255, standin_pixel.a)

source:fill(rectangle({ x = 0, y = 0, size = { 2, 2 } }), "#004080")

local background = blank({ width = 4, height = 4 })
background:fill(region, source:vivid_in(region))

local background_pixel = background:pixel(0, 0)
assert(background_pixel.r == 200, background_pixel.r)
assert(background_pixel.g == 30, background_pixel.g)
assert(background_pixel.b == 60, background_pixel.b)
assert(background_pixel.a == 255, background_pixel.a)

local strip = blank({ width = 2, height = 2 })
strip:fill(rectangle({ x = 0, y = 0, size = { 2, 1 } }), "#00FF00")

local green = blank({ width = 4, height = 4 })
green:fill(region, strip:vivid_in(rectangle({ size = { 2, 2 } })))

local green_pixel = green:pixel(0, 0)
assert(green_pixel.r == 0, green_pixel.r)
assert(green_pixel.g == 255, green_pixel.g)
assert(green_pixel.b == 0, green_pixel.b)
assert(green_pixel.a == 255, green_pixel.a)

local night = blank({ width = 4, height = 4, color = "#101010" })
night:fill(rectangle({ x = 0, y = 0, size = { 1, 1 } }), "#FF0000")

local spark = blank({ width = 4, height = 4 })
spark:fill(region, night:vivid_in(region))

local spark_pixel = spark:pixel(0, 0)
assert(spark_pixel.r == 255, spark_pixel.r)
assert(spark_pixel.g == 0, spark_pixel.g)
assert(spark_pixel.b == 0, spark_pixel.b)
assert(spark_pixel.a == 255, spark_pixel.a)

local probe = blank({ width = 4, height = 4, color = "#FF0000" })
probe:fill(
  region,
  source:vivid_in(rectangle({ x = 10, y = 10, size = { 2, 2 } }))
)
assert(probe:pixel(0, 0).r == 255, "a transparent fill is a no-op")

local clear = blank({ width = 4, height = 4 })
probe:fill(region, clear:vivid_in(region))
assert(probe:pixel(0, 0).r == 255, "a transparent fill is a no-op")

return {
  render = function()
  end,
}
