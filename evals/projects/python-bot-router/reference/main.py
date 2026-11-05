from commands import parse
def dispatch(text, handlers):
    parsed=parse(text)
    if parsed is None: return None
    name,args=parsed
    handler=handlers.get(name)
    return handler(args) if handler else None
