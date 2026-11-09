import java.util.*;import java.util.function.*;
public class Policy {public static Optional<String> dispatch(String text,Map<String,Function<List<String>,String>> handlers){List<String> parts=Parser.parse(text);return Optional.of(handlers.get(parts.get(0)).apply(parts.subList(1,parts.size())));}}
