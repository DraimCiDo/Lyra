from validation import quantities
def checkout(stock,lines):
    total=0
    for sku,qty in quantities(lines).items():
        if stock[sku]<qty: raise ValueError('stock')
        stock[sku]-=qty;total+=qty
    return total
