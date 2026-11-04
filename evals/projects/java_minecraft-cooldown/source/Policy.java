public class Policy {public static long remaining(long now,long last,long duration){return Math.max(0,last+duration-now);}}
