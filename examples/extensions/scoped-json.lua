return function(ctx)
  if ctx.request and ctx.request.trigger == "close" then
    return {intent = {finish = {result = "closed"}}}
  end
  if ctx.broker.step == 0 then
    if ctx.request then
      return {intent = {language_server = {
        profile = "json", method = ctx.request.kind, position = ctx.request.position
      }}}
    end
    return {intent = {tool = {
      profile = "json", input = ctx.document.text, arguments = {}
    }}}
  end
  local response = ctx.broker.response
  assert(not response.failed)
  if ctx.request then return {intent = {finish = {result = response.language_server.result}}} end
  assert(response.tool.status == 0)
  return {intent = {finish = {transaction = hedit.replace(response.tool.output)}}}
end
