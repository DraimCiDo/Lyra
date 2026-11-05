def update(db,key,revision,patch):
    db[key]=dict(patch,revision=revision)
    return db[key]
