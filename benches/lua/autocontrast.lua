local width = 2048
local height = 2048

local canvas = blank({ width = width, height = height, color = "#659DFB" })
canvas:fill(rectangle({ size = { 16, 16 } }), "#000000")

canvas:autocontrast()

local pixel = canvas:pixel(0, 0)
assert(pixel.r == 0 and pixel.g == 0 and pixel.b == 0, pixel.r)
assert(pixel.a == 255, pixel.a)
assert(canvas.width == width, canvas.width)

return {
  render = function()
  end,
}
