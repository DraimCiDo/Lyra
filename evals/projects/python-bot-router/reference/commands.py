def parse(text):
    fields=text.split()
    if not fields or not fields[0].startswith('/') or len(fields[0])==1: return None
    return fields[0][1:].split('@',1)[0].lower(),fields[1:]
