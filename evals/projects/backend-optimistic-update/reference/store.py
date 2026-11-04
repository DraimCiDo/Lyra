def update(db,key,revision,patch):
    current=db[key]
    if current['revision'] != revision: raise ValueError('stale')
    changed=dict(current);changed.update(patch);changed['revision']=revision+1
    db[key]=changed
    return dict(changed)
