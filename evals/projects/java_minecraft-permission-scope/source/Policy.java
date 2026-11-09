import java.util.*;
public class Policy { public static boolean allowed(Set<String> grants,String node){return grants!=null&&grants.stream().anyMatch(node::startsWith);} }
