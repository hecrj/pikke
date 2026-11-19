return {
  arguments = { "layout" },

  render = function(args)
    local layout = args.layout

    if layout == nil then
      return
    end

    assert(type(layout) == "table", "layout is a table")
    assert(layout.width == 32, "width")
    assert(layout.height == 64, "height")
    assert(layout.ghost == nil, "null binds nil")
    assert(type(layout.size) == "table", "a nested object is a table")
    assert(layout.size.width == 8, "a nested width")
    assert(layout.size.label == "small", "a nested string")
  end,
}
