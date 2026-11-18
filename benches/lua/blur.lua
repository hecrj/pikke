local width = 1024
local height = 1024

local source = blank({ width = width, height = height, color = "#659DFB" })

return {
  render = function()
    local target = source:clone()
    target:blur(8)

    local pixel = target:pixel(width / 2, height / 2)
    assert(pixel.r == 101, pixel.r)
    assert(pixel.a == 255, pixel.a)

    assert(target.width == width, target.width)
    assert(target.height == height, target.height)
  end,
}
