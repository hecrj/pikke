local must_fail = require("modules/assert").must_fail

local all = rectangle({ size = { 4, 4 } })

-- opaque source, recolor to black at 0.5: flat black at alpha 128
local opaque = blank({ width = 4, height = 4, color = "#FFFFFF" })
local shadow = opaque:clone()
shadow:recolor(srgb(0.0, 0.0, 0.0, 0.5))
local pixel = shadow:pixel(0, 0)
assert(pixel.r == 0 and pixel.g == 0 and pixel.b == 0, "the color is flat black")
assert(pixel.a == 128, "the alpha is the source alpha * the color alpha")

-- the result keeps the source's alpha shape, scaled by the color's alpha
local semi = blank({ width = 4, height = 4 })
semi:fill(all, srgb(1.0, 1.0, 1.0, 0.5))
semi:recolor(srgb(0.0, 0.0, 0.0, 0.5))
assert(semi:pixel(0, 0).a == 64, "0.5 * 0.5 rounds to 64")
assert(semi:pixel(0, 0).r == 0, "flat black")

-- a transparent source stays transparent
local clear = blank({ width = 4, height = 4 })
clear:recolor(srgb(1.0, 0.0, 0.0))
assert(clear:pixel(0, 0).a == 0, "transparent stays transparent")

-- recolor is flat: a source with varying luma loses its luma (unlike monochrome)
local varying = blank({ width = 2, height = 2 })
varying:fill(rectangle({ size = { 1, 1 } }), "#FFFFFF")
varying:fill(rectangle({ x = 1, y = 1, size = { 1, 1 } }), "#000000")
assert(varying:pixel(0, 0).r > varying:pixel(1, 1).r, "the source has varying luma")
varying:recolor(srgb(1.0, 0.0, 0.0))
assert(varying:pixel(0, 0).r == varying:pixel(1, 1).r, "the luma is discarded (flat)")
assert(varying:pixel(0, 0).r == 255 and varying:pixel(0, 0).g == 0, "the color is red")

-- a non-black color lands in the expected sRGB value
local colored = blank({ width = 1, height = 1, color = "#FFFFFF" })
colored:recolor(srgb(1.0, 0.0, 0.0))
local pixel = colored:pixel(0, 0)
assert(pixel.r == 255 and pixel.g == 0 and pixel.b == 0, "red")

-- a color alpha of 1 keeps the source alpha
local full = blank({ width = 4, height = 4 })
full:fill(all, srgb(0.5, 0.5, 0.5, 0.5))
full:recolor(srgb(0.0, 0.0, 1.0))
local pixel = full:pixel(0, 0)
assert(pixel.a == 128, "the source alpha is kept (color alpha 1)")
assert(pixel.b > pixel.r and pixel.b > pixel.g, "the hue is blue")

-- a hex string is accepted
local hex = blank({ width = 1, height = 1, color = "#FFFFFF" })
hex:recolor("#0000FF")
local pixel = hex:pixel(0, 0)
assert(pixel.b == 255 and pixel.r == 0, "blue from a hex string")
assert(pixel.a == 255, "the opaque source stays opaque")

must_fail("an `Oklch`, or a hex string", function()
  opaque:recolor(42)
end)
must_fail("an `Oklch`, or a hex string", function()
  opaque:recolor({})
end)

return {
  render = function()
  end,
}
