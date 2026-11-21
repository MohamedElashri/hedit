local function identifier(value)
  return #value <= 48 and value:match("^[A-Za-z_][A-Za-z_0-9]*$") ~= nil
end
local function word(byte)
  return byte and (byte >= 128 or byte == 95 or byte >= 48 and byte <= 57
    or byte >= 65 and byte <= 90 or byte >= 97 and byte <= 122)
end
return function(ctx)
  local from, to = ctx.arguments.from, ctx.arguments.to
  assert(identifier(from) and identifier(to) and from ~= to)
  local scope = ctx.broker.scope
  assert(scope)
  if ctx.broker.step == 0 then
    return {intent = {search = {scope = scope, query = from}}}
  end
  local reply = ctx.broker.response
  assert(not reply.failed)
  if ctx.broker.step == 1 then
    assert(not reply.searched.truncated)
    local paths, seen = {}, {}
    for _, match in ipairs(reply.searched.matches) do
      if not seen[match.relative] then
        paths[#paths + 1], seen[match.relative] = match.relative, true
      end
    end
    assert(#paths <= 32)
    if #paths == 0 then return {intent = {finish = {}}} end
    return {intent = {open_many = {scope = scope, paths = paths}}}
  end
  if ctx.broker.step == 2 then
    local changes = {}
    for _, file in ipairs(reply.opened_many.files) do
      local edits, offset = {}, 1
      while true do
        local start, finish = file.text:find(from, offset, true)
        if not start then break end
        if not word(file.text:byte(start - 1)) and not word(file.text:byte(finish + 1)) then
          edits[#edits + 1] = hedit.edit(start - 1, finish, to)
        end
        offset = finish + 1
      end
      assert(#edits <= 64)
      if #edits > 0 then
        changes[#changes + 1] = {resource = file.resource.resource,
          version = file.resource.version, edits = edits}
      end
    end
    if #changes == 0 then return {intent = {finish = {}}} end
    return {intent = {review_edits = {scope = scope, changes = changes}}}
  end
  assert(reply.edited)
  return {intent = {finish = {}}}
end
