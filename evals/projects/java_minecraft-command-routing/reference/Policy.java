import java.util.*;import java.util.function.*;
public class Policy {public static Optional<String> dispatch(String text,Map<String,Function<List<String>,String>> handlers){List<String> parts=Parser.parse(text);if(parts.isEmpty())return Optional.empty();var handler=handlers.get(parts.get(0));return handler==null?Optional.empty():Optional.ofNullable(handler.apply(parts.subList(1,parts.size())));}}
