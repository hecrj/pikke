local must_fail = require("modules/assert").must_fail

local red = { 255, 0, 0 }
local green = { 0, 255, 0 }
local blue = { 0, 0, 255 }
local yellow = { 255, 255, 0 }
local cyan = { 0, 255, 255 }
local magenta = { 255, 0, 255 }

local function expect(image, x, y, color)
  local pixel = image:pixel(x, y)
  assert(pixel, "no pixel at " .. x .. ", " .. y)
  assert(
    pixel.r == color[1] and pixel.g == color[2] and pixel.b == color[3] and pixel.a == 255,
    string.format("pixel %d, %d is %d %d %d %d", x, y, pixel.r, pixel.g, pixel.b, pixel.a)
  )
end

local function check(image, layout)
  for y, row in ipairs(layout) do
    for x, color in ipairs(row) do
      expect(image, x - 1, y - 1, color)
    end
  end
end

local function source()
  local image = blank({ width = 3, height = 2 })
  image:fill(rectangle({ x = 0, y = 0, size = 1 }), "#FF0000")
  image:fill(rectangle({ x = 1, y = 0, size = 1 }), "#00FF00")
  image:fill(rectangle({ x = 2, y = 0, size = 1 }), "#0000FF")
  image:fill(rectangle({ x = 0, y = 1, size = 1 }), "#FFFF00")
  image:fill(rectangle({ x = 1, y = 1, size = 1 }), "#00FFFF")
  image:fill(rectangle({ x = 2, y = 1, size = 1 }), "#FF00FF")
  return image
end

local source_layout = {
  { red, green, blue },
  { yellow, cyan, magenta },
}
local clockwise90 = {
  { yellow, red },
  { cyan, green },
  { magenta, blue },
}
local counter_clockwise90 = {
  { blue, magenta },
  { green, cyan },
  { red, yellow },
}
local half = {
  { magenta, cyan, yellow },
  { blue, green, red },
}

check(source(), source_layout)

local quarter = source()
quarter:rotate(90)
assert(quarter.width == 2 and quarter.height == 3, "a quarter turn swaps the dims")
check(quarter, clockwise90)

local backward = source()
backward:rotate(-90)
assert(backward.width == 2 and backward.height == 3, "a quarter turn swaps the dims")
check(backward, counter_clockwise90)

local turn = source()
turn:rotate(180)
assert(turn.width == 3 and turn.height == 2, "a half turn keeps the dims")
check(turn, half)

local three_quarters = source()
three_quarters:rotate(270)
check(three_quarters, counter_clockwise90)

local full = source()
full:rotate(360)
check(full, source_layout)

local idle = source()
idle:rotate(0)
check(idle, source_layout)

local extra = source()
extra:rotate(450)
check(extra, clockwise90)

local function square()
  local image = blank({ width = 2, height = 2 })
  image:fill(rectangle({ x = 0, y = 0, size = 1 }), "#FF0000")
  image:fill(rectangle({ x = 1, y = 0, size = 1 }), "#00FF00")
  image:fill(rectangle({ x = 0, y = 1, size = 1 }), "#0000FF")
  image:fill(rectangle({ x = 1, y = 1, size = 1 }), "#FFFF00")
  return image
end

local square_cw = square()
square_cw:rotate(90)
check(square_cw, { { blue, red }, { yellow, green } })

local square_ccw = square()
square_ccw:rotate(-90)
check(square_ccw, { { green, yellow }, { red, blue } })

local srgb_image = blank({ width = 2, height = 1, color = "#0000FF", color_space = "srgb" })
srgb_image:fill(rectangle({ x = 1, size = 1 }), "#FF0000")
srgb_image:rotate(90)
assert(srgb_image.width == 1 and srgb_image.height == 2, "a quarter turn swaps the dims")
expect(srgb_image, 0, 0, blue)
expect(srgb_image, 0, 1, red)

must_fail("must be a multiple of 90", function()
  source():rotate(45)
end)
must_fail("must be a multiple of 90", function()
  source():rotate(90.5)
end)
must_fail("must be a multiple of 90", function()
  source():rotate(math.huge)
end)
must_fail("must be a multiple of 90", function()
  source():rotate(0 / 0)
end)

return {
  render = function()
    yield(quarter)
  end,
}
