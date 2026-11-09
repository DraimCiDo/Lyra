from validation import quantities
def checkout(stock,lines):
    requested=quantities(lines)
    if any(sku not in stock or stock[sku]<qty for sku,qty in requested.items()): raise ValueError('stock')
    for sku,qty in requested.items(): stock[sku]-=qty
    return sum(requested.values())
