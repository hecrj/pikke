local dir = package.path:match("^(.-)/%?%.lua;")
local fixture = dir .. "/fixtures/shared-data.png"
local data = dir .. "/shared-data.png"

return {
  render = function()
    local in_file = assert(io.open(fixture, "rb"))
    local bytes = in_file:read("a")
    in_file:close()

    local out_file = assert(io.open(data, "wb"))
    out_file:write(bytes)
    out_file:close()

    local base = image("shared-data.png")
    local left = base:clone()
    local right = base:clone()

    left:fill(rectangle({ x = 0, y = 0, size = { 16, 32 } }), "#FFFFFF")
    right:fill(rectangle({ x = 16, y = 0, size = { 16, 32 } }), "#000000")

    assert(left:pixel(0, 0).r == 255, "the left half is white")
    assert(left:pixel(16, 0).r == 200, "the left keeps the base past the fill")
    assert(right:pixel(0, 0).r == 200, "the right keeps the base")
    assert(right:pixel(16, 0).r == 0, "the right half is black")

    yield(left)

    assert(os.remove(data), "the data file is gone")
    yield(right)
  end,
}
