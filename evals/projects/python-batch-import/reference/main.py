from rows import parse_rows
def load(text):
    result={}
    for row in parse_rows(text):
        key=row.get('id','').strip()
        if not key or key in result: raise ValueError('id')
        result[key]=row['name']
    return result
