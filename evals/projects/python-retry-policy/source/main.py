class Retryable(Exception): pass
def retry(fn, attempts):
    for _ in range(attempts+1):
        try: return fn()
        except Exception: pass
    return None
