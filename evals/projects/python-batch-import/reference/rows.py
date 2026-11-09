import csv,io
def parse_rows(text):
    reader=csv.DictReader(io.StringIO(text))
    if reader.fieldnames is None: return []
    reader.fieldnames=[name.strip() for name in reader.fieldnames]
    if len(set(reader.fieldnames)) != len(reader.fieldnames): raise ValueError('headers')
    return list(reader)
