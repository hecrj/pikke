return {
  must_fail = function(pattern, call)
    local ok, error = pcall(call)
    assert(not ok, "must have failed")
    assert(tostring(error):find(pattern) ~= nil, tostring(error))
  end,
}
