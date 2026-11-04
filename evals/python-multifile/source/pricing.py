def subtotal(items):
    return sum(price for price, quantity in items)


def discounted(total, percent):
    return total * percent / 100
