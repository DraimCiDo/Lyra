from rows import parse_rows
def load(text):
    return {row['id']: row['name'] for row in parse_rows(text)}
