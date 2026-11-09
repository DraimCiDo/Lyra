def can_access(user,resource):
    return bool(user and user.get('id') is not None and user.get('tenant') is not None
        and user.get('tenant')==resource.get('tenant')
        and (user.get('admin') is True or user.get('id')==resource.get('owner')))
