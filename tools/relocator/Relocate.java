package jix;

import java.nio.file.*;
import java.util.*;
import org.objectweb.asm.*;
import org.objectweb.asm.commons.*;

/** Offline class-file transformation only; Rust owns archive policy and resources. */
public final class Relocate {
    public static void main(String[] args) throws Exception {
        Path root = Path.of(args[0]);
        List<String[]> rules = Files.readAllLines(Path.of(args[1])).stream()
            .filter(s -> !s.isEmpty()).map(s -> s.split("\t", -1)).toList();
        boolean strings = Boolean.parseBoolean(args[2]);
        Remapper mapper = new Remapper(Opcodes.ASM9) {
            @Override public String map(String name) {
                for (String[] rule : rules) {
                    if (name.startsWith(rule[0] + "/")) return rule[1] + name.substring(rule[0].length());
                }
                return name;
            }
            @Override public Object mapValue(Object value) {
                if (strings && value instanceof String s && s.matches("[A-Za-z_$][A-Za-z0-9_$]*(?:[./][A-Za-z_$][A-Za-z0-9_$]*)+")) {
                    boolean dotted = s.indexOf('/') < 0;
                    String mapped = map(s.replace('.', '/'));
                    return dotted ? mapped.replace('/', '.') : mapped;
                }
                return super.mapValue(value);
            }
        };
        try (var files = Files.walk(root)) {
            for (Path file : files.filter(p -> p.toString().endsWith(".class")).sorted().toList()) {
                ClassReader reader = new ClassReader(Files.readAllBytes(file));
                ClassWriter writer = new ClassWriter(0);
                reader.accept(new ClassRemapper(writer, mapper), 0);
                Files.write(file, writer.toByteArray());
            }
        }
    }
}
