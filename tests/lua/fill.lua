local must_fail = require("modules/assert").must_fail

local face = font("../../fonts/NotoSans-Regular.ttf")

local output = blank({ width = 4, height = 4 })
local region = rectangle({ x = 0, y = 0, size = { 2, 2 } })

must_fail("not a #RRGGBB hex color", function()
  output:fill(region, "808080")
end)
must_fail("not a #RRGGBB hex color", function()
  output:fill(region, "#12345")
end)
must_fail("not a #RRGGBB hex color", function()
  output:fill(region, "#GGGGGG")
end)
must_fail("not a #RRGGBB hex color", function()
  output:fill(region, "#1234567")
end)
must_fail("an `Oklch`, or a hex string", function()
  output:fill(region, 42)
end)
must_fail("an `Oklch`, or a hex string", function()
  output:fill(region, {})
end)
must_fail("a `Text`, or a `Path`", function()
  output:fill(output, "#000000")
end)

output:fill(region, "#FF0000")
local pixel = output:pixel(0, 0)
assert(pixel.r == 255, pixel.r)
assert(pixel.g == 0, pixel.g)
assert(pixel.b == 0, pixel.b)
assert(pixel.a == 255, pixel.a)
assert(output:pixel(3, 3).a == 0, "the fill stays inside the region")

local layout = text("Noto", { font = face, size = 20 })
output:fill(layout, "#000000")

local source = blank({ width = 4, height = 4, color = "#659DFB" })
local hex = blank({ width = 4, height = 4 })
local oklch = blank({ width = 4, height = 4 })
local all = rectangle({ size = { 4, 4 } })

hex:fill(all, "#659DFB")
oklch:fill(all, source:dominant_in(all):to_oklch())

local hex_pixel = hex:pixel(0, 0)
local oklch_pixel = oklch:pixel(0, 0)
assert(hex_pixel.r == oklch_pixel.r, "the Oklch paints the hex")
assert(hex_pixel.g == oklch_pixel.g, "the Oklch paints the hex")
assert(hex_pixel.b == oklch_pixel.b, "the Oklch paints the hex")
assert(hex_pixel.a == oklch_pixel.a, "the Oklch paints the hex")

return {
  render = function()
  end,
}
