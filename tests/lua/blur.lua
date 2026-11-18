local must_fail = require("modules/assert").must_fail

local solid = blank({ width = 8, height = 8, color = "#808080" })
solid:blur(2)
local center = solid:pixel(4, 4)
assert(center.r == 128 and center.g == 128 and center.b == 128, "the interior is unchanged")
assert(center.a == 255, "the interior stays opaque")
local corner = solid:pixel(0, 0)
assert(corner.a < 255, "the corner fades toward transparent")
assert(corner.a > 0, "the corner is not erased")
assert(solid:pixel(0, 4).a < 255, "the edge fades")

-- a uniform color blurs to the same color with a faded alpha (the RGB is preserved)
local flat = blank({ width = 8, height = 8, color = "#808080" })
flat:blur(2)
local edge = flat:pixel(0, 4)
assert(edge.r == 128 and edge.g == 128 and edge.b == 128, "the flat color is preserved")
assert(edge.a < 255, "the alpha fades at the edge")

-- a colored silhouette keeps its hue as the alpha fades
local red = blank({ width = 8, height = 8, color = "#FF0000" })
red:blur(2)
local red_edge = red:pixel(0, 4)
assert(red_edge.r == 255 and red_edge.g == 0 and red_edge.b == 0, "the hue holds at the edge")
assert(red_edge.a < 255, "the alpha fades at the edge")

-- radius 0 is a no-op
local untouched = blank({ width = 8, height = 8, color = "#808080" })
untouched:blur(0)
assert(untouched:pixel(0, 0).r == 128, "radius 0 leaves the color")
assert(untouched:pixel(0, 0).a == 255, "radius 0 leaves the corner opaque")

-- a fully transparent image stays transparent
local clear = blank({ width = 8, height = 8 })
clear:blur(4)
assert(clear:pixel(4, 4).a == 0, "transparent stays transparent")

must_fail("'radius' must be a non%-negative number", function()
  solid:blur(-1)
end)
must_fail("'radius' must be a non%-negative number", function()
  solid:blur(math.huge)
end)

return {
  render = function()
  end,
}
