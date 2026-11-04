def parse_rows(text):
    lines=text.strip().splitlines()
    return [dict(zip(lines[0].split(','),line.split(','))) for line in lines[1:]]
