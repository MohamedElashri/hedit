assert(io == nil and os == nil and package == nil and require == nil)
local invocations = 0
return function(ctx)
    invocations = invocations + 1
    assert(invocations == 1) -- Every continuation uses a fresh confined VM.
    local response = ctx.broker.response
    if response == nil then
        return {intent = {discover = {scope = ctx.broker.scope, query = '.txt'}}}
    end
    if response.discovered then
        assert(not response.discovered.truncated)
        local paths = {}
        for _, entry in ipairs(response.discovered.entries) do
            if not entry.directory then paths[#paths + 1] = entry.relative end
        end
        assert(#paths > 0 and #paths <= 32)
        return {intent = {open_many = {scope = ctx.broker.scope, paths = paths}}}
    end
    if response.opened_many then
        local changes = {}
        for _, file in ipairs(response.opened_many.files) do
            local edits, offset = {}, 1
            while true do
                local start, finish = string.find(file.text, ctx.arguments.before, offset, true)
                if start == nil then break end
                edits[#edits + 1] = hedit.edit(start - 1, finish, ctx.arguments.after)
                offset = finish + 1
            end
            if #edits > 0 then
                changes[#changes + 1] = {resource = file.resource.resource,
                    version = file.resource.version, edits = edits}
            end
        end
        assert(#changes > 0)
        return {intent = {review_edits = {scope = ctx.broker.scope, changes = changes}}}
    end
    assert(response.edited and response.edited.files == 2)
    return {intent = {finish = {}}}
end
