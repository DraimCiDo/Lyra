class Idempotency:
    def __init__(self): pass
    def execute(self,tenant,key,fingerprint,fn): raise NotImplementedError()
