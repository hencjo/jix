import java.nio.file.*;
import java.util.*;
import java.util.concurrent.atomic.AtomicBoolean;
import org.jetbrains.jps.model.*;
import org.jetbrains.jps.model.java.*;
import org.jetbrains.jps.model.serialization.*;
import org.jetbrains.jps.cmdline.*;
import org.jetbrains.jps.incremental.*;
import org.jetbrains.jps.incremental.messages.*;
import org.jetbrains.jps.incremental.fs.*;
import org.jetbrains.jps.api.*;

/** Loads generated files with IntelliJ's real JPS model and runs its native builder.
 * SDKs are registered only in this in-memory test model, never global IDE settings.
 */
public final class JpsSmoke {
    public static void main(String[] args) throws Exception {
        Path project = Path.of(args[0]);
        JpsModel model = JpsElementFactory.getInstance().createModel();
        JpsProjectLoader.loadProject(model.getProject(), Map.of(), project);
        var service = JpsJavaExtensionService.getInstance();
        for (var module : model.getProject().getModules()) {
            var ref = module.getSdkReferencesTable().getSdkReference(JpsJavaSdkType.INSTANCE);
            service.addJavaSdk(model.getGlobal(), ref.getSdkName(), args[1]);
        }
        var compiler = service.getCompilerConfiguration(model.getProject());
        if (!compiler.useReleaseOption() || compiler.getDefaultAnnotationProcessingProfile().isEnabled())
            throw new AssertionError("Compiler policy lost");
        for (var module : model.getProject().getModules()) {
            String options = compiler.getCurrentCompilerOptions().ADDITIONAL_OPTIONS_OVERRIDE.get(module.getName());
            if (options == null || !options.contains("-proc:none") || !options.contains("-implicit:none"))
                throw new AssertionError("Compiler options lost: " + options);
        }
        var error = new AtomicBoolean();
        MessageHandler handler = message -> {
            System.out.println(message);
            if (message.getKind() == BuildMessage.Kind.ERROR) error.set(true);
        };
        BuildRunner runner = new BuildRunner(() -> model);
        var descriptor = runner.load(handler, project.resolve(".jix/jps-test-cache"), new BuildFSState(true));
        try {
            var scope = CmdlineRemoteProto.Message.ControllerMessage.ParametersMessage.TargetTypeBuildScope.newBuilder()
                .setTypeId("java-production").setAllTargets(true).setForceBuild(true).build();
            runner.runBuild(descriptor, CanceledStatus.NULL, handler, BuildType.BUILD, List.of(scope), true);
        } finally { descriptor.release(); }
        if (error.get()) throw new AssertionError("Native JPS build failed");
        if (args.length > 2) {
            var owner = model.getProject().getModules().stream().filter(m -> m.getName().equals(args[2])).findFirst().orElseThrow();
            String cp = String.join(java.io.File.pathSeparator, JpsJavaExtensionService.dependencies(owner)
                .recursively().runtimeOnly().productionOnly().withoutSdk().classes().getRoots().stream()
                .map(java.io.File::getPath).toList());
            if (!Files.isRegularFile(service.getOutputDirectoryPath(owner, false).resolve(args[3].replace('.', '/') + ".class")))
                throw new AssertionError("JPS did not compile the main class");
            String moduleName = null;
            for (var source : owner.getSourceRoots()) {
                Path info = Path.of(source.getFile().getPath()).resolve("module-info.java");
                if (Files.isRegularFile(info)) {
                    var matcher = java.util.regex.Pattern.compile("module\\s+([\\w.]+)").matcher(Files.readString(info));
                    if (!matcher.find()) throw new AssertionError("Invalid module descriptor");
                    moduleName = matcher.group(1);
                }
            }
            String entry = moduleName == null ? args[3] : moduleName + "/" + args[3];
            var command = moduleName == null ? List.of(args[1] + "/bin/java", "-cp", cp, entry)
                : List.of(args[1] + "/bin/java", "--module-path", cp, "--module", entry);
            var result = new ProcessBuilder(command)
                .directory(project.toFile()).redirectErrorStream(true).start();
            String output = new String(result.getInputStream().readAllBytes());
            if (result.waitFor() != 0 || !output.contains("Hello")) throw new AssertionError("Native launch failed: " + output);
            debug(args[1], cp, args[3], moduleName);
        }
    }

    private static void debug(String home, String cp, String main, String module) throws Exception {
        var connector = com.sun.jdi.Bootstrap.virtualMachineManager().defaultConnector();
        var arguments = connector.defaultArguments();
        arguments.get("home").setValue(home);
        arguments.get("main").setValue(module == null ? main : "--module " + module + "/" + main);
        arguments.get("options").setValue((module == null ? "-cp" : "--module-path") + " \"" + cp + "\"");
        var vm = connector.launch(arguments);
        try {
            var prepare = vm.eventRequestManager().createClassPrepareRequest();
            prepare.addClassFilter(main);
            prepare.setSuspendPolicy(com.sun.jdi.request.EventRequest.SUSPEND_ALL);
            prepare.enable();
            // Resume the queued VMStart event once, not both here and in the event loop.
            boolean hit = false, done = false;
            long deadline = System.nanoTime() + 30_000_000_000L;
            while (!done && System.nanoTime() < deadline) {
                var events = vm.eventQueue().remove(1000);
                if (events == null) continue;
                for (var event : events) {
                    if (event instanceof com.sun.jdi.event.ClassPrepareEvent loaded) {
                        var location = loaded.referenceType().methodsByName("main").getFirst().allLineLocations().getFirst();
                        vm.eventRequestManager().createBreakpointRequest(location).enable();
                    } else if (event instanceof com.sun.jdi.event.BreakpointEvent stopped) {
                        hit = stopped.thread().frame(0).location().method().name().equals("main");
                    } else if (event instanceof com.sun.jdi.event.VMDeathEvent || event instanceof com.sun.jdi.event.VMDisconnectEvent) {
                        done = true;
                    }
                }
                if (!done) events.resume();
            }
            if (!hit) throw new AssertionError("Native source breakpoint was not hit");
            System.out.println("Native JPS build/run and source breakpoint passed");
        } finally { try { vm.exit(0); } catch (com.sun.jdi.VMDisconnectedException ignored) {} }
    }
}
