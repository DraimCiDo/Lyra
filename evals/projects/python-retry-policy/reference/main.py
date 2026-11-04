class Retryable(Exception): pass
def retry(fn, attempts):
    if attempts < 1: raise ValueError('attempts')
    for i in range(attempts):
        try: return fn()
        except Retryable:
            if i+1 == attempts: raise
