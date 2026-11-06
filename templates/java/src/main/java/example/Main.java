package example;

public final class Main {
    public static void main(String[] args) throws Exception {
        try (var input = Main.class.getResourceAsStream("/message.txt")) {
            System.out.print(new String(input.readAllBytes(), java.nio.charset.StandardCharsets.UTF_8));
        }
    }
}
