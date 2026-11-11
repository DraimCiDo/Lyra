def paginate(items,page,size):
    return {'items':items[page*size:(page+1)*size],'total':len(items[page*size:])}
