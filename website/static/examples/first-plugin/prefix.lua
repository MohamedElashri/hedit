return function(ctx)
  return hedit.replace(ctx.arguments.prefix .. ctx.document.text)
end
