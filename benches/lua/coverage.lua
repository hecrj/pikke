local width = 2048
local height = 2048

local canvas = blank({ width = width, height = height })
canvas:fill(rectangle({ size = { width / 2, height } }), "#659DFB")

return {
    render = function()
        local coverage = canvas:coverage_in(rectangle({ size = { width, height } }))

        local expected = width * height / 2
        assert(math.abs(coverage - expected) < 4096, coverage)
        assert(canvas.width == width, canvas.width)
    end,
}
