local rect = rectangle({ size = 100.5 })
assert(rect.x == 0, rect.x)
assert(rect.y == 0, rect.y)
assert(rect.width == 100.5, rect.width)
assert(rect.height == 100.5, rect.height)

local rect = rectangle({ x = 10.5, y = -10.5, size = { 100.5, 50.5 } })
assert(rect.x == 10.5, rect.x)
assert(rect.y == -10.5, rect.y)
assert(rect.width == 100.5, rect.width)
assert(rect.height == 50.5, rect.height)

local ok = pcall(rectangle, {})
assert(not ok, "a rect needs a size")
ok = pcall(rectangle, { size = { 100.0 } })
assert(not ok, "a size table needs both dims")

local box = rectangle({ x = 10, y = 20, size = { 100, 50 } })

local padded = box:padded(10)
assert(padded.x == 0, padded.x)
assert(padded.y == 10, padded.y)
assert(padded.width == 120, padded.width)
assert(padded.height == 70, padded.height)

local padded = box:padded({ 10, 5 })
assert(padded.x == 5, padded.x)
assert(padded.y == 10, padded.y)
assert(padded.width == 110, padded.width)
assert(padded.height == 70, padded.height)

local padded = box:padded({ left = 1, top = 2, right = 3, bottom = 4 })
assert(padded.x == 9, padded.x)
assert(padded.y == 18, padded.y)
assert(padded.width == 104, padded.width)
assert(padded.height == 56, padded.height)

ok = pcall(box.padded, box, { 10 })
assert(not ok, "a padded table needs both dims")
ok = pcall(box.padded, box, {})
assert(not ok, "padded needs a value")

return {
  render = function()
  end,
}
