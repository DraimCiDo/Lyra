def quantities(lines):
    result={}
    for sku,qty in lines:
        if not isinstance(qty,int) or isinstance(qty,bool) or qty<=0: raise ValueError('quantity')
        result[sku]=result.get(sku,0)+qty
    return result
