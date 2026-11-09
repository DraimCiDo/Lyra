class Cache:
    def __init__(self, clock): self.clock, self.items = clock, {}
    def put(self, key, value, ttl): self.items[key] = (value, self.clock()+ttl)
    def get(self, key, fallback=None):
        value, deadline = self.items.get(key, (fallback, self.clock()+1))
        return value if self.clock() <= deadline and value is not None else fallback
