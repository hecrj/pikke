local image = blank({ width = 10, height = 10, color = "#FF0000" })

assert(image.width == 10, image.width)
assert(image.height == 10, image.height)
assert(
    math.abs(image:luma_in(rectangle({ size = { 10, 10 } })) - 0.2126 * 255) < 1,
    "the luma of solid red is the 709 red coefficient, on the 0-255 scale"
)

return {
  render = function()
    yield(image)
  end,
}
