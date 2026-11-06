package example;

import java.nio.charset.StandardCharsets;
import org.junit.platform.commons.util.StringUtils;

public final class Main {
    public static void main(String[] args) throws Exception {
        try (var stream = Main.class.getResourceAsStream("/message.txt")) {
            if (stream == null) throw new IllegalStateException("Missing bundled resource");
            String message = new String(stream.readAllBytes(), StandardCharsets.UTF_8).strip();
            if (StringUtils.isBlank(message)) throw new IllegalStateException("Empty greeting");
            System.out.println(message);
        }
    }
}
