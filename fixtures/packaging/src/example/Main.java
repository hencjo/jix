package example;

import java.lang.annotation.*;
import java.util.List;
import java.util.function.Supplier;

@Retention(RetentionPolicy.RUNTIME)
@interface Label { Class<?> value(); }

@Label(Main.class)
public final class Main {
    public record Box<T>(T value) {}
    public static void main(String[] args) throws Exception {
        Supplier<Box<String>> factory = () -> new Box<>("relocated");
        List<Box<String>> values = List.of(factory.get());
        if (Main.class.getAnnotation(Label.class).value() != Main.class) throw new AssertionError();
        if (Class.forName("example.Main") != Main.class) throw new AssertionError();
        if (!Box.class.isRecord()) throw new AssertionError();
        try (var resource = Main.class.getResourceAsStream("message.txt")) {
            if (resource == null) throw new AssertionError("package resource missing");
            System.out.print(values.getFirst().value() + ":" + new String(resource.readAllBytes()));
        }
    }
}
