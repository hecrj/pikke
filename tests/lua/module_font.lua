local face = require("modules/face").face

local text = text("Noto", { font = face, size = 20 })
assert(text.width > 0, "the face did not register")
assert(text.height > 0, "the face did not register")

return {
	render = function() end,
}
