local must_fail = require("modules/assert").must_fail

local base = blank({ width = 16, height = 16 })

local detect = base:detect("#FF0000")
assert(detect() == nil)

local reds = {
	rectangle({ x = 2, y = 2, size = { 2, 2 } }),
	rectangle({ x = 6, y = 6, size = { 5, 2 } }),
	rectangle({ x = 3, y = 10, size = { 3, 4 } }),
	rectangle({ x = 10, y = 10, size = { 4, 4 } }),
}

local blues = {
	rectangle({ x = 10, y = 2, size = { 2, 4 } }),
	rectangle({ x = 4, y = 4, size = { 2, 2 } }),
	rectangle({ x = 3, y = 8, size = { 8, 2 } }),
}

for _, red in ipairs(reds) do
	base:fill(red, "#FF0000")
end

for _, blue in ipairs(blues) do
	base:fill(blue, "#0000FF")
end

local detect = base:detect("#FF0000")
local i = 1

for red in detect do
	assert(red == reds[i])
	i = i + 1
end

local detect = base:detect("#0000FF")
local i = 1

for blue in detect do
	assert(blue == blues[i])
	i = i + 1
end

local near = blank({ width = 8, height = 8 })
near:fill(rectangle({ x = 1, y = 1, size = { 2, 2 } }), "#FF0000")
near:fill(rectangle({ x = 3, y = 1, size = { 2, 2 } }), "#FF0100")

local detect = near:detect("#FF0000")
assert(detect() == rectangle({ x = 1, y = 1, size = { 2, 2 } }))
assert(detect() == nil, "the near color is not a match")

local detect = near:detect("#FF0000", { tolerance = 0.001 })
assert(detect() == rectangle({ x = 1, y = 1, size = { 4, 2 } }), "the near color joins the component")
assert(detect() == nil)

local edge = blank({ width = 8, height = 8 })
edge:fill(rectangle({ x = 1, y = 1, size = { 2, 2 } }), "#FF0000")
edge:fill(rectangle({ x = 3, y = 1, size = { 1, 2 } }), srgb(1, 0, 0, 0.5))

local detect = edge:detect("#FF0000", { tolerance = 0.49 })
assert(detect() == rectangle({ x = 1, y = 1, size = { 2, 2 } }))
assert(detect() == nil)

local detect = edge:detect("#FF0000", { tolerance = 0.5 })
assert(detect() == rectangle({ x = 1, y = 1, size = { 3, 2 } }), "the distance is inclusive")
assert(detect() == nil)

local mask = blank({ width = 16, height = 16 })
mask:fill(rectangle({ x = 2, y = 2, size = { 4, 4 } }), "#FF0000")

local enlarged = mask:clone()
enlarged:resize({ width = 24, height = 24 })

local detect = enlarged:detect("#FF0000")
local region = detect()
assert(region ~= nil, "the solid core survives a resize")
assert(region.x >= 3.0 and region.y >= 3.0, "the core is inside the mask's extent")
assert(region.x + region.width <= 9.0 and region.y + region.height <= 9.0, "the core is inside the mask's extent")
assert(region.width < 6.0 and region.height < 6.0, "the anti-aliased edges break the exact match")
assert(detect() == nil)

local detect = enlarged:detect("#FF0000", { tolerance = 0.5 })
assert(detect() == rectangle({ x = 3, y = 3, size = { 6, 6 } }), "a tolerance recovers the mask's extent")
assert(detect() == nil)

local shrunk = mask:clone()
shrunk:resize({ width = 8, height = 8 })

assert(shrunk:detect("#FF0000")() == nil, "a shrunken mask has no exact-match pixels")

local detect = shrunk:detect("#FF0000", { tolerance = 0.5 })
assert(detect() == rectangle({ x = 1, y = 1, size = { 2, 2 } }), "a tolerance recovers the shrunken mask")
assert(detect() == nil)

must_fail("non%-negative", function()
	base:detect("#FF0000", { tolerance = -1 })
end)

must_fail("must be finite", function()
	base:detect("#FF0000", { tolerance = 1 / 0 })
end)

must_fail("not a number", function()
	base:detect("#FF0000", { tolerance = "half" })
end)

return {
	render = function() end,
}
