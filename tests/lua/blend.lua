local base = blank({ width = 8, height = 8 })
local layer = blank({ width = 8, height = 8, color = "#0000FF" })

-- The full layer would cover the whole base; `at` limits the window to 4x4 at (2, 3).
base:blend(layer, { at = rectangle({ x = 2, y = 3, size = { 4, 4 } }) })

assert(base:pixel(2, 3).b == 255, "the window's top-left corner")
assert(base:pixel(5, 6).b == 255, "the window's bottom-right corner")
assert(base:pixel(6, 3).a == 0, "right of the window is untouched")
assert(base:pixel(5, 7).a == 0, "below the window is untouched")
assert(base:pixel(0, 0).a == 0, "outside the window is untouched")

-- No `at` blends the whole layer at the origin.
local base = blank({ width = 8, height = 8 })
base:blend(layer)
assert(base:pixel(0, 0).b == 255, "omitting `at` blends the whole layer")
assert(base:pixel(7, 7).b == 255, "omitting `at` blends the whole layer")

-- A zero-sized `at` blends nothing.
local base = blank({ width = 8, height = 8 })
base:blend(layer, { at = rectangle({ x = 0, y = 0, size = 0 }) })
assert(base:pixel(0, 0).a == 0, "a zero window blends nothing")
assert(base:pixel(7, 7).a == 0, "a zero window blends nothing")

return {
  render = function()
    yield(base)
  end,
}
