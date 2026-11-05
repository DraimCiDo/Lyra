class Bucket:
    def __init__(self,capacity,rate,clock):
        if capacity<=0 or rate<0: raise ValueError('limits')
        self.capacity,self.rate,self.clock=capacity,rate,clock
        self.tokens,self.last=capacity,clock()
    def take(self,amount=1):
        if amount<=0: raise ValueError('amount')
        now=self.clock();elapsed=max(0,now-self.last);self.last=max(now,self.last)
        self.tokens=min(self.capacity,self.tokens+elapsed*self.rate)
        if amount>self.tokens: return False
        self.tokens-=amount;return True
