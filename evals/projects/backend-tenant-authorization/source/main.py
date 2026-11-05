def can_access(user,resource):
    return bool(user and (user.get('admin') or user.get('id')==resource.get('owner')))
