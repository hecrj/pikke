local must_fail = require("modules/assert").must_fail

local producer = [[
import sys

args = sys.argv[1:]
width = 0
height = 0
frames = 0
truncate = False
fail = False

i = 0
while i < len(args):
    if args[i] == "--width":
        width = int(args[i + 1])
        i += 2
    elif args[i] == "--height":
        height = int(args[i + 1])
        i += 2
    elif args[i] == "--frames":
        frames = int(args[i + 1])
        i += 2
    elif args[i] == "--truncate":
        truncate = True
        i += 1
    elif args[i] == "--fail":
        fail = True
        i += 1
    else:
        i += 1

out = sys.stdout.buffer
reps = (width * 4 + 1023) // 1024

for frame in range(frames):
    x = bytes((i + frame) % 256 for i in range(256))
    y = bytes((i + frame) % 256 for i in range(256))
    tile = b"".join(
        bytes(v for i in range(256) for v in (x[i], y[j], frame % 256, 255))
        for j in range(256)
    )
    image = b"".join(
        (tile[(j % 256) * 1024:(j % 256 + 1) * 1024] * reps)[: width * 4]
        for j in range(height)
    )

    if truncate and frame == frames - 1:
        sys.stderr.write("truncating the stream\n")
        out.write(image[: len(image) // 2])
    else:
        out.write(image)

out.flush()

if fail:
    sys.stderr.write("the producer failed\n")
    sys.exit(3)
]]

local premultiplied_producer = [[
import sys

args = sys.argv[1:]
width = int(args[args.index("--width") + 1])
height = int(args[args.index("--height") + 1])
frames = int(args[args.index("--frames") + 1])

out = sys.stdout.buffer
out.write(bytes([128, 0, 0, 128]) * width * height * frames)
out.flush()
]]

do
  local stream = stream("python3", { "-c", producer, "--frames", 3 }, {
    width = 100,
    height = 100,
  })

  for frame_number = 0, 2 do
    local frame = stream()
    assert(frame ~= nil, "the frame is pulled")
    assert(frame.width == 100, frame.width)
    assert(frame.height == 100, frame.height)

    local pixel = frame:pixel(3, 7)
    assert(pixel.r == 3 + frame_number, pixel.r)
    assert(pixel.g == 7 + frame_number, pixel.g)
    assert(pixel.b == frame_number, pixel.b)
    assert(pixel.a == 255, pixel.a)
  end
end

do
  local stream = stream("python3", {
    "-c",
    premultiplied_producer,
    "--frames",
    "1",
  }, { width = 4, height = 4, alpha = "premultiplied" })

  local pixel = stream():pixel(1, 1)
  assert(pixel.r == 255, pixel.r)
  assert(pixel.g == 0, pixel.g)
  assert(pixel.b == 0, pixel.b)
  assert(pixel.a == 128, pixel.a)
end

do
  local stream = stream("python3", {
    "-c",
    premultiplied_producer,
    "--frames",
    "1",
  }, { width = 4, height = 4 })

  local pixel = stream():pixel(1, 1)
  assert(pixel.r == 128, pixel.r)
  assert(pixel.g == 0, pixel.g)
  assert(pixel.b == 0, pixel.b)
  assert(pixel.a == 128, pixel.a)
end

must_fail("stream: width must be a positive number", function()
  stream("python3", { "-c", producer }, { width = 0, height = 100 })
end)
must_fail("stream: height must be a positive number", function()
  stream("python3", { "-c", producer }, { width = 100, height = 0 })
end)
must_fail("stream: 'width' must be finite", function()
  stream("python3", { "-c", producer }, { width = math.huge, height = 100 })
end)
must_fail("stream: alpha must be either straight or premultiplied", function()
  stream("python3", { "-c", producer }, {
    width = 100,
    height = 100,
    alpha = "linear",
  })
end)
must_fail("is a reserved argument", function()
  stream("python3", { "-c", producer, "--width" }, { width = 100, height = 100 })
end)
must_fail("must be a string or a number", function()
  stream("python3", { true }, { width = 100, height = 100 })
end)

must_fail("mid%-frame", function()
  local stream = stream("python3", {
    "-c",
    producer,
    "--frames",
    "3",
    "--truncate",
  }, { width = 100, height = 100 })
  while stream() do end
end)
must_fail("the producer failed", function()
  local stream = stream("python3", {
    "-c",
    producer,
    "--frames",
    "2",
    "--fail",
  }, { width = 100, height = 100 })
  while stream() do end
end)
must_fail("failed to spawn", function()
  stream("pikke-no-such-producer", {}, { width = 100, height = 100 })
end)

return {
  render = function()
  end,
}
