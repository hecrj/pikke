return {
  render = function()
    yield(blank({ width = 10, height = 10 }))

    local ok, error = pcall(yield, blank({ width = 20, height = 10 }))
    assert(not ok, "a mismatched frame must fail")
    assert(tostring(error):find("20×10") ~= nil, tostring(error))
    assert(tostring(error):find("10×10") ~= nil, tostring(error))
  end,
}
