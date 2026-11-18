local must_fail = require("modules/assert").must_fail

local function red_blue(width, height)
  local image = blank({ width = width, height = height, color = "#FF0000" })
  image:fill(rectangle({ x = width / 2, y = 0, size = { width / 2, height } }), "#0000FF")
  return image
end

local function blue_red(height)
  local image = blank({ width = 50, height = height, color = "#FF0000" })
  image:fill(rectangle({ x = 0, y = height / 2, size = { 50, height / 2 } }), "#0000FF")
  return image
end

-- A cover resize to a shorter box crops the width; align_x picks the window.
local left = red_blue(100, 50)
left:resize({ width = 50, height = 50, mode = "cover", align_x = "left" })
assert(left:pixel(10, 25).r == 255 and left:pixel(40, 25).r == 255, "left keeps the left edge")
assert(left:pixel(10, 25).b == 0 and left:pixel(40, 25).b == 0, "left keeps the left edge")

local center = red_blue(100, 50)
center:resize({ width = 50, height = 50, mode = "cover" })
assert(center:pixel(10, 25).r == 255, "the default is a center crop")
assert(center:pixel(40, 25).b == 255, "the default is a center crop")

local right = red_blue(100, 50)
right:resize({ width = 50, height = 50, mode = "cover", align_x = "right" })
assert(right:pixel(10, 25).b == 255 and right:pixel(40, 25).b == 255, "right keeps the right edge")
assert(right:pixel(10, 25).r == 0 and right:pixel(40, 25).r == 0, "right keeps the right edge")

-- A cover resize to a taller box crops the height; align_y picks the window.
local top = blue_red(100)
top:resize({ width = 50, height = 50, mode = "cover", align_y = "top" })
assert(top:pixel(25, 10).r == 255 and top:pixel(25, 40).r == 255, "top keeps the top edge")
assert(top:pixel(25, 10).b == 0 and top:pixel(25, 40).b == 0, "top keeps the top edge")

local bottom = blue_red(100)
bottom:resize({ width = 50, height = 50, mode = "cover", align_y = "bottom" })
assert(bottom:pixel(25, 10).b == 255 and bottom:pixel(25, 40).b == 255, "bottom keeps the bottom edge")
assert(bottom:pixel(25, 10).r == 0 and bottom:pixel(25, 40).r == 0, "bottom keeps the bottom edge")

must_fail("unknown align_x", function()
  red_blue(100, 50):resize({ width = 50, height = 50, mode = "cover", align_x = "middle" })
end)
must_fail("unknown align_y", function()
  blue_red(100):resize({ width = 50, height = 50, mode = "cover", align_y = "middle" })
end)
must_fail("unknown filter", function()
  red_blue(100, 50):resize({ width = 50, height = 50, filter = "bilinear" })
end)

local hard = blank({ width = 16, height = 16, color = "#000000" })
hard:fill(rectangle({ x = 4, y = 4, size = { 8, 8 } }), "#FFFFFF")
hard:resize({ width = 11, height = 13, filter = "nearest" })

for x = 0, 10 do
  for y = 0, 12 do
    local pixel = hard:pixel(x, y)
    local pure = (pixel.r == 0 and pixel.g == 0 and pixel.b == 0)
      or (pixel.r == 255 and pixel.g == 255 and pixel.b == 255)
    assert(pure, "nearest must not blend")
  end
end

local strip = blank({ width = 4, height = 1 })
strip:fill(rectangle({ x = 0, size = { 1, 1 } }), "#FFFFFF")
strip:fill(rectangle({ x = 1, size = { 1, 1 } }), "#000000")
strip:fill(rectangle({ x = 2, size = { 1, 1 } }), "#FFFFFF")
strip:fill(rectangle({ x = 3, size = { 1, 1 } }), "#000000")
strip:resize({ width = 8, height = 1, filter = "nearest" })
assert(strip:pixel(1, 0).r == 255, "the 2× upscale doubles each source pixel")
assert(strip:pixel(2, 0).r == 0, "the 2× upscale doubles each source pixel")
assert(strip:pixel(5, 0).r == 255, "the 2× upscale doubles each source pixel")
assert(strip:pixel(7, 0).r == 0, "the 2× upscale doubles each source pixel")

local flat = blank({ width = 5, height = 3 })
flat:fill(rectangle({ size = { 5, 3 } }), "#808080")
flat:resize({ width = 7, height = 4, filter = "nearest" })
assert(
  flat:pixel(6, 3).r == 128 and flat:pixel(6, 3).g == 128 and flat:pixel(6, 3).b == 128,
  "a constant plane stays constant"
)

local half = blank({ width = 5, height = 3 })
half:fill(rectangle({ size = { 5, 3 } }), srgb(0, 0, 0, 0.5))
half:resize({ width = 7, height = 4, filter = "nearest" })
assert(
  half:pixel(6, 3).r == 0 and half:pixel(6, 3).g == 0 and half:pixel(6, 3).b == 0
    and half:pixel(6, 3).a == 128,
  "the alpha passes through unblended"
)

return {
  render = function()
    yield(bottom)
  end,
}
