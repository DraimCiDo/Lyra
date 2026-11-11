def paginate(items,page,size):
    if not isinstance(page,int) or not isinstance(size,int) or isinstance(page,bool) or isinstance(size,bool) or page<1 or not 1<=size<=100: raise ValueError('pagination')
    start=(page-1)*size
    return {'items':items[start:start+size],'total':len(items)}
