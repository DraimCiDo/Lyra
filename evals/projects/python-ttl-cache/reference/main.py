class Cache:
    def __init__(self, clock): self.clock, self.items = clock, {}
    def put(self, key, value, ttl): self.items[key] = (value, self.clock()+ttl)
    def get(self, key, fallback=None):
        if key not in self.items: return fallback
        value, deadline = self.items[key]
        if self.clock() >= deadline:
            del self.items[key]
            return fallback
        return value
