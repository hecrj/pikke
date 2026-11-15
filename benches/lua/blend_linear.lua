local width = 2048
local height = 2048

local base = blank({ width = width, height = height, color = "#659DFB" })
local layer = blank({ width = width, height = height, color = "#F6C17B" })

return {
  render = function()
    local target = base:clone()
    target:blend(layer, { color_space = "linear" })

    local pixel = target:pixel(0, 0)
    assert(pixel.r == 246, pixel.r)
    assert(pixel.g == 193, pixel.g)
    assert(pixel.b == 123, pixel.b)

    assert(target.width == width, target.width)
    assert(target.height == height, target.height)
  end,
}
