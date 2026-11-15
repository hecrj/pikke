local width = 2048
local height = 2048

local canvas = blank({ width = width, height = height, color = "#659DFB" })
local luma = canvas:luma_in(rectangle({ size = { width, height } }))

assert(luma > 0, luma)
assert(canvas.width == width, canvas.width)
assert(canvas.height == height, canvas.height)

return {
  render = function()
  end,
}
