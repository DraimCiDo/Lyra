from commands import parse
def dispatch(text, handlers):
    name,args=parse(text)
    return handlers[name](args)
