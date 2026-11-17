local width = 2048
local height = 2048

local base = blank({ width = width, height = height, color = "#659DFB", color_space = "srgb" })
local layer = blank({ width = width, height = height, color = "#F6C17B" })

layer:convert("srgb")

return {
	render = function()
		local target = base:clone()
		target:blend(layer)

		local pixel = target:pixel(0, 0)
		assert(math.abs(pixel.r - 246) <= 1, pixel.r)
		assert(math.abs(pixel.g - 193) <= 1, pixel.g)
		assert(math.abs(pixel.b - 123) <= 1, pixel.b)

		assert(target.width == width, target.width)
		assert(target.height == height, target.height)
	end,
}
