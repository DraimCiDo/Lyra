import java.util.*;
public class Policy { public static boolean allowed(Set<String> grants,String node){if(grants==null||node==null||node.isEmpty())return false;if(grants.contains("*")||grants.contains(node))return true;for(int i=node.lastIndexOf('.');i>0;i=node.lastIndexOf('.',i-1))if(grants.contains(node.substring(0,i)+".*"))return true;return false;} }
