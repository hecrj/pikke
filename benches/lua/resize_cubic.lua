local width = 1024
local height = 1024

local source = blank({ width = width, height = height, color = "#659DFB" })

return {
  render = function()
    local stretch = source:clone()
    stretch:resize({ width = width / 2, height = height / 2 })

    local cover = source:clone()
    cover:resize({ width = width / 4, height = height / 2, mode = "cover" })

    assert(stretch.width == width / 2, stretch.width)
    assert(stretch.height == height / 2, stretch.height)
    assert(cover.width == width / 4, cover.width)
    assert(cover.height == height / 2, cover.height)
  end,
}
