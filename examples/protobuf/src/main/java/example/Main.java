package example;

import example.generated.Greeting;

public class Main {
    public static void main(String[] args) throws Exception {
        var message = Greeting.newBuilder().setMessage("Hello from protobuf").build();
        System.out.println(Greeting.parseFrom(message.toByteArray()).getMessage());
    }
}
