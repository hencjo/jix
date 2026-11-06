package plain;
public final class Main {
    public static void main(String[] args) throws Exception {
        Class.forName("plugin.Extra");
        System.out.println(parser.Text.message());
    }
}
