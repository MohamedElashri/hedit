return function(ctx)
  assert(#ctx.document.selections == 1)
  local selection = ctx.document.selections[1]
  local start, finish = math.min(selection.anchor, selection.head), math.max(selection.anchor, selection.head)
  assert(start < finish)
  if ctx.broker.step == 0 then
    return {intent = {https = {
      endpoint = "github-markdown", body = ctx.document.text:sub(start + 1, finish)
    }}}
  end
  local reply = ctx.broker.response
  assert(not reply.failed and reply.https.status == 200)
  return {intent = {finish = {
    transaction = hedit.transaction({hedit.edit(start, finish, reply.https.body)})
  }}}
end
