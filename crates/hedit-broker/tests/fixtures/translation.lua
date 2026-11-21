assert(io == nil and os == nil and package == nil and require == nil)
return function(ctx)
    if ctx.broker.response == nil then
        return {intent = {https = {endpoint = 'translation',
            body = hedit.json_encode({text = ctx.document.text, target = 'fr'})}}}
    end
    local response = ctx.broker.response.https
    assert(response and response.status == 200)
    local result = hedit.json_decode(response.body)
    assert(type(result.text) == 'string')
    return {intent = {finish = {transaction = hedit.replace(result.text)}}}
end
