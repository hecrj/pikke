return {
  render = function()
    for _ = 1, 3 do
      local frame = blank({ width = 16, height = 16, color = "#00FF00" })

      local pixel = frame:pixel(0, 0)
      assert(pixel.r == 0, pixel.r)
      assert(pixel.g == 255, pixel.g)
      assert(pixel.b == 0, pixel.b)
      assert(pixel.a == 255, pixel.a)

      yield(frame)
    end
  end,
}
