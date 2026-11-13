local must_fail = require("modules/assert").must_fail

local image = blank({ width = 10, height = 10, color = "#FF0000" })

assert(image.width == 10, image.width)
assert(image.height == 10, image.height)

local pixel = image:pixel(3, 7)
assert(pixel.r == 255, pixel.r)
assert(pixel.g == 0, pixel.g)
assert(pixel.b == 0, pixel.b)
assert(pixel.a == 255, pixel.a)

assert(image:pixel(9, 9).r == 255, "the corner keeps the fill")
assert(image:pixel(-1, 0) == nil, "outside the image is nil")
assert(image:pixel(10, 0) == nil, "outside the image is nil")

assert(blank({ width = 4, height = 4 }):pixel(0, 0).a == 0, "an absent color is transparent")

must_fail("blank: needs 'width'", function()
  return blank({ height = 2, color = "#C81E3C" })
end)
must_fail("blank: needs 'height'", function()
  return blank({ width = 4, color = "#C81E3C" })
end)
must_fail("blank: 'width' must be a positive number", function()
  return blank({ width = 0, height = 2, color = "#C81E3C" })
end)
must_fail("blank: 'height' must be a positive number", function()
  return blank({ width = 4, height = -2, color = "#C81E3C" })
end)
must_fail("not a #RRGGBB hex color", function()
  return blank({ width = 4, height = 2, color = "C81E3C" })
end)

return {
  render = function()
    yield(image)
  end,
}
