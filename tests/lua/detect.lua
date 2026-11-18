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

return {
	render = function() end,
}
