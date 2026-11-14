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

do
  local stream = stream("python3", 100, 100, { "-c", producer, "--frames", 3 })

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

must_fail("stream: width must be a finite positive number", function()
  stream("python3", 0, 100, { "-c", producer })
end)
must_fail("stream: height must be a finite positive number", function()
  stream("python3", 100, 0, { "-c", producer })
end)
must_fail("stream: width must be a finite positive number", function()
  stream("python3", math.huge, 100, { "-c", producer })
end)
must_fail("is a reserved argument", function()
  stream("python3", 100, 100, { "-c", producer, "--width" })
end)
must_fail("must be a string or a number", function()
  stream("python3", 100, 100, { true })
end)

must_fail("mid%-frame", function()
  local stream = stream("python3", 100, 100, {
    "-c",
    producer,
    "--frames",
    "3",
    "--truncate",
  })
  while stream() do end
end)
must_fail("the producer failed", function()
  local stream = stream("python3", 100, 100, {
    "-c",
    producer,
    "--frames",
    "2",
    "--fail",
  })
  while stream() do end
end)
must_fail("failed to spawn", function()
  stream("pikke-no-such-producer", 100, 100, {})
end)

return {
  render = function()
  end,
}
