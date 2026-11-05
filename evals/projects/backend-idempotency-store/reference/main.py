class Idempotency:
    def __init__(self): self.entries={}
    def execute(self,tenant,key,fingerprint,fn):
        identity=tenant,key
        if identity in self.entries:
            saved,value=self.entries[identity]
            if saved!=fingerprint: raise ValueError('idempotency conflict')
            return value
        value=fn();self.entries[identity]=(fingerprint,value);return value
