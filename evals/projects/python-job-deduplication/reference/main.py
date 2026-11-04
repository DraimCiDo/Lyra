from collections import deque
class Queue:
    def __init__(self): self.items,self.keys=deque(),set()
    def push(self,key,payload):
        if key in self.keys: return False
        self.keys.add(key);self.items.append((key,payload));return True
    def pop(self):
        if not self.items: return None
        key,payload=self.items.popleft();self.keys.remove(key);return key,payload
