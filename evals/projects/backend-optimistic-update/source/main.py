from store import update
def put(db,key,revision,patch):
    try: return 200,update(db,key,revision,patch)
    except KeyError: return 409,None
