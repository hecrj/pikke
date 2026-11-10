noto_sans = font("../fonts/NotoSans-Regular.ttf")

function render()
  base = blank({width=300, height=100, color="#FFFFFF"})

  hello = text("Hello", {font=noto_sans, size=80})
  hello:translate({x=(base.width - hello.width) / 2, y=(base.height - hello.height) / 2})

  base:fill(hello, "#000000")

  yield(base)
end
