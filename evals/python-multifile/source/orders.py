from pricing import subtotal, discounted


def checkout(items, percent=0):
    return round(discounted(subtotal(items), percent), 2)
