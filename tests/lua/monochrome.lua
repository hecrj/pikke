local must_fail = require("modules/assert").must_fail

local all = rectangle({ size = { 4, 4 } })

local gray = blank({ width = 4, height = 4, color = "#808080" })
local before = gray:luma_in(all)

must_fail("not a #RRGGBB hex color", function()
  gray:monochrome("808080")
end)
must_fail("not a #RRGGBB hex color", function()
  gray:monochrome("#GGGGGG")
end)
must_fail("an `Oklch`, or a hex string", function()
  gray:monochrome(42)
end)
must_fail("an `Oklch`, or a hex string", function()
  gray:monochrome({})
end)

gray:monochrome("#FF0000")
local pixel = gray:pixel(0, 0)
assert(pixel.r == 255, "the shade falls short to the hue's peak")
assert(pixel.g == 0, pixel.g)
assert(pixel.b == 0, pixel.b)
assert(pixel.a == 255, pixel.a)
assert(math.abs(gray:luma_in(all) - before) < 1, "the luma is unchanged")

local black = blank({ width = 4, height = 4, color = "#000000" })
black:monochrome("#FF0000")
assert(black:pixel(0, 0).r == 0, "black stays black")

local clear = blank({ width = 4, height = 4 })
clear:monochrome("#FF0000")
assert(clear:pixel(0, 0).a == 0, "the transparent pixel stays transparent")

local source = blank({ width = 4, height = 4, color = "#659DFB" })

local half = blank({ width = 4, height = 4 })
local paint = source:dominant_in(all):to_oklch()
paint.a = 0.5
half:fill(all, paint)
local before = half:luma_in(all)

half:monochrome("#FF0000")
local pixel = half:pixel(0, 0)
assert(pixel.a == 128, "the coverage is unchanged")
assert(pixel.g == 0 and pixel.b == 0, "the hue is the tint's")
assert(pixel.r > 0, pixel.r)
assert(math.abs(half:luma_in(all) - before) < 1, "the luma is unchanged")

local hex = blank({ width = 4, height = 4, color = "#808080" })
local dominant = blank({ width = 4, height = 4, color = "#808080" })
local oklch = blank({ width = 4, height = 4, color = "#808080" })

hex:monochrome("#659DFB")
dominant:monochrome(source:dominant_in(all))
oklch:monochrome(source:dominant_in(all):to_oklch())

local hex_pixel = hex:pixel(0, 0)
local dominant_pixel = dominant:pixel(0, 0)
local oklch_pixel = oklch:pixel(0, 0)
for _, channel in ipairs({ "r", "g", "b", "a" }) do
  assert(hex_pixel[channel] == dominant_pixel[channel], "the Color paints the hex")
  assert(hex_pixel[channel] == oklch_pixel[channel], "the Oklch paints the hex")
end

return {
  render = function()
  end,
}
