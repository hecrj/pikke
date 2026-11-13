local base = blank({ width = 256, height = 256, color = "#808080" })
local branch = base:clone()

base:resize({ width = 64 })

assert(base.width == 64, base.width)
assert(branch.width == 256, branch.width)

local pixel = branch:pixel(100, 100)
assert(pixel.r == 128, pixel.r)
assert(pixel.g == 128, pixel.g)
assert(pixel.b == 128, pixel.b)
assert(pixel.a == 255, pixel.a)

return {
  render = function()
    yield(branch)
  end,
}
