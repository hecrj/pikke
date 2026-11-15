local width = 2048
local height = 2048

local canvas = blank({ width = width, height = height, color = "#C81E3C" })
local paint = canvas:dominant_in(rectangle({ size = { width, height } }))

local probe = blank({ width = 4, height = 4 })
probe:fill(rectangle({ size = { 4, 4 } }), paint)

local pixel = probe:pixel(0, 0)
assert(math.abs(pixel.r - 200) <= 1, pixel.r)
assert(math.abs(pixel.g - 30) <= 1, pixel.g)
assert(math.abs(pixel.b - 60) <= 1, pixel.b)
assert(pixel.a == 255, pixel.a)

assert(canvas.width == width, canvas.width)

return {
  render = function()
  end,
}
