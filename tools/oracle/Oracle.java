import com.google.gson.*;
import java.nio.file.*;
import java.util.*;
import org.codehaus.plexus.*;
import org.apache.maven.execution.*;
import org.apache.maven.internal.aether.DefaultRepositorySystemSessionFactory;
import org.apache.maven.model.*;
import org.apache.maven.model.io.xpp3.MavenXpp3Writer;
import org.apache.maven.project.*;
import org.eclipse.aether.graph.DependencyNode;
import org.eclipse.aether.util.filter.DependencyFilterUtils;

/** Test oracle only. Maven supplies all model building and graph decisions. */
public final class Oracle {
  static final Gson JSON = new GsonBuilder().setPrettyPrinting().create();
  static String string(JsonObject o, String key, String fallback) {
    return o.has(key) ? o.get(key).getAsString() : fallback;
  }
  static Dependency dependency(JsonElement value) {
    JsonObject o = value.isJsonPrimitive() ? new JsonObject() : value.getAsJsonObject();
    String module = value.isJsonPrimitive() ? value.getAsString() : o.get("module").getAsString();
    String[] p = module.split(":", -1);
    if (p.length < 2 || p.length > 3) throw new IllegalArgumentException("Invalid coordinate: " + module);
    Dependency d = new Dependency();
    d.setGroupId(p[0]); d.setArtifactId(p[1]);
    if (p.length == 3) d.setVersion(p[2]);
    d.setType(string(o, "extension", "jar"));
    d.setClassifier(string(o, "classifier", null));
    d.setScope("compile");
    if (o.has("exclude")) for (JsonElement e : o.getAsJsonArray("exclude")) {
      String[] parts = e.getAsString().split(":", -1);
      Exclusion exclusion = new Exclusion();
      exclusion.setGroupId(parts[0]); exclusion.setArtifactId(parts[1]); d.addExclusion(exclusion);
    }
    return d;
  }
  static String id(org.eclipse.aether.artifact.Artifact a) {
    return a.getGroupId() + ":" + a.getArtifactId() + ":" + a.getExtension() + ":" + a.getClassifier() + ":" + a.getVersion();
  }
  static Map<String,Object> depModel(Dependency d) {
    Map<String,Object> m = new TreeMap<>();
    m.put("group", d.getGroupId()); m.put("artifact", d.getArtifactId());
    m.put("version", d.getVersion()); m.put("type", d.getType());
    m.put("classifier", d.getClassifier() == null ? "" : d.getClassifier());
    m.put("scope", d.getScope() == null ? "compile" : d.getScope());
    m.put("optional", d.isOptional());
    if (d.getOptional() != null) m.put("optionalValue", d.isOptional());
    m.put("exclusions", d.getExclusions().stream().map(e -> e.getGroupId()+":"+e.getArtifactId()).toList());
    return m;
  }
  static void visit(DependencyNode node, List<DependencyNode> parents, String scope,
      List<Map<String,Object>> artifacts, List<Map<String,Object>> edges, Set<String> seen, String parent) {
    String nextParent = parent;
    if (node.getDependency() != null) {
      String key = id(node.getArtifact());
      edges.add(Map.of("from", parent, "to", key, "scope", node.getDependency().getScope()));
      if (DependencyFilterUtils.classpathFilter(scope).accept(node, parents) && seen.add(key)) {
        artifacts.add(Map.of("id", key, "scope", node.getDependency().getScope()));
      }
      nextParent = key;
    }
    List<DependencyNode> next = new ArrayList<>(parents); next.add(node);
    for (DependencyNode child : node.getChildren()) visit(child, next, scope, artifacts, edges, seen, nextParent);
  }
  static String failureKind(Throwable e) {
    String message = e.toString().toLowerCase(Locale.ROOT);
    if (message.contains("cycle") || message.contains("recursive")) return "cycle";
    if (message.contains("non-parseable") || message.contains("malformed") || message.contains("must be a valid") || message.contains("is missing") || message.contains("invalid pom")) return "invalid";
    if (message.contains("could not find artifact") || message.contains("absent") || message.contains("non-resolvable")) return "missing";
    if (e.getCause() != null) return failureKind(e.getCause());
    return "invalid";
  }
  static Properties contextProperties(JsonObject context) {
    Properties result = new Properties();
    for (String field : List.of("systemProperties", "userProperties")) {
      if (context.has(field)) for (var entry : context.getAsJsonObject(field).entrySet())
        result.setProperty(entry.getKey(), entry.getValue().getAsString());
    }
    return result;
  }
  // Each declared context gets its own JVM: Maven Os caches path.separator statically.
  // The activators/model builder themselves remain the unmodified pinned Maven classes.
  static boolean contextual(JsonObject request, java.io.PrintStream output) throws Exception {
    if (Boolean.getBoolean("jix.oracle.worker")) return false;
    boolean declared = request.has("resolutionContext") && !request.get("resolutionContext").isJsonNull();
    for (var entry : request.getAsJsonObject("classpaths").entrySet())
      declared |= entry.getValue().getAsJsonObject().has("resolutionContext");
    if (!declared) return false;
    JsonObject combined = new JsonObject();
    for (String key : List.of("classpaths", "models", "origins")) combined.add(key, new JsonObject());
    for (var entry : request.getAsJsonObject("classpaths").entrySet()) {
      JsonObject single = request.deepCopy();
      JsonObject cps = new JsonObject(); cps.add(entry.getKey(), entry.getValue()); single.add("classpaths", cps);
      JsonElement context = entry.getValue().getAsJsonObject().get("resolutionContext");
      if (context == null || context.isJsonNull()) context = request.get("resolutionContext");
      if (context != null) single.add("resolutionContext", context);
      Path input = Files.createTempFile("jix-oracle-context-", ".json");
      try {
        Files.writeString(input, single.toString());
        var child = new ProcessBuilder(Path.of(System.getProperty("java.home"), "bin", "java").toString(),
            "-Djix.oracle.worker=true", "-Dorg.slf4j.simpleLogger.defaultLogLevel=error",
            "-cp", System.getProperty("java.class.path"), "Oracle", input.toString())
            .redirectError(ProcessBuilder.Redirect.INHERIT).start();
        String data = new String(child.getInputStream().readAllBytes(), java.nio.charset.StandardCharsets.UTF_8);
        if (child.waitFor() != 0) throw new IllegalStateException("Context oracle failed: " + data);
        JsonObject value = JsonParser.parseString(data).getAsJsonObject();
        for (String key : List.of("classpaths", "models", "origins")) if (value.has(key))
          for (var item : value.getAsJsonObject(key).entrySet()) combined.getAsJsonObject(key).add(item.getKey(), item.getValue());
      } finally { Files.deleteIfExists(input); }
    }
    if (request.has("inspectModels")) { combined.remove("classpaths"); combined.remove("origins"); }
    else { combined.remove("models"); if (!request.has("origins")) combined.remove("origins"); }
    output.println(JSON.toJson(combined));
    return true;
  }
  public static void main(String[] args) throws Exception {
    if (args.length != 1) throw new IllegalArgumentException("Usage: oracle request.json");
    java.io.PrintStream output = System.out;
    System.setOut(System.err); // Maven diagnostics must never contaminate JSON stdout.
    JsonObject request = JsonParser.parseString(Files.readString(Path.of(args[0]))).getAsJsonObject();
    if (contextual(request, output)) return;
    Properties declared = request.has("resolutionContext") && !request.get("resolutionContext").isJsonNull()
        ? contextProperties(request.getAsJsonObject("resolutionContext")) : new Properties();
    if (declared.containsKey("path.separator")) System.setProperty("path.separator", declared.getProperty("path.separator"));
    Path work = Files.createTempDirectory("jix-oracle-");
    DefaultPlexusContainer container = new DefaultPlexusContainer(new DefaultContainerConfiguration()
        .setClassPathScanning(PlexusConstants.SCANNING_INDEX).setAutoWiring(true));
    try {
      var execution = new DefaultMavenExecutionRequest();
      execution.setLocalRepositoryPath(work.resolve("repository").toFile());
      execution.setSystemProperties(declared); execution.setUserProperties(new Properties());
      execution.setInteractiveMode(false); execution.setIgnoreTransitiveRepositories(true);
      container.lookup(MavenExecutionRequestPopulator.class).populateDefaults(execution);
      var session = container.lookup(DefaultRepositorySystemSessionFactory.class).newRepositorySession(execution);
      session.setIgnoreArtifactDescriptorRepositories(true);
      session.setArtifactDescriptorPolicy(new org.eclipse.aether.util.repository.SimpleArtifactDescriptorPolicy(false, false));
      Set<String> allowed = new HashSet<>();
      for (JsonElement url : request.getAsJsonArray("repositories")) allowed.add(url.getAsString());
      session.setMirrorSelector(repository -> allowed.contains(repository.getUrl()) ? null :
          new org.eclipse.aether.repository.RemoteRepository.Builder("jix-denied", "default", work.resolve("denied").toUri().toString()).build());
      Map<String,Object> origins = new TreeMap<>();
      session.setRepositoryListener(new org.eclipse.aether.AbstractRepositoryListener() {
        @Override public void artifactDownloaded(org.eclipse.aether.RepositoryEvent e) {
          if (e.getException() == null && e.getRepository() instanceof org.eclipse.aether.repository.RemoteRepository r)
            origins.put(id(e.getArtifact()), r.getUrl());
        }
      });
      Map<String,Object> classpaths = new TreeMap<>();
      Map<String,Object> models = new TreeMap<>();
      for (var entry : request.getAsJsonObject("classpaths").entrySet()) {
        JsonObject cp = entry.getValue().getAsJsonObject();
        Model root = new Model(); root.setModelVersion("4.0.0");
        root.setGroupId("jix.synthetic"); root.setArtifactId("root"); root.setVersion("1");
        for (JsonElement d : cp.getAsJsonArray("dependencies")) root.addDependency(dependency(d));
        if (cp.has("platforms")) {
          DependencyManagement dm = new DependencyManagement();
          for (JsonElement p : cp.getAsJsonArray("platforms")) {
            Dependency d = dependency(p); d.setType("pom"); d.setScope("import"); dm.addDependency(d);
          }
          root.setDependencyManagement(dm);
        }
        int index = 0;
        for (JsonElement url : request.getAsJsonArray("repositories")) {
          Repository repo = new Repository(); repo.setId(index++ == 0 ? "central" : "repo-" + index);
          repo.setUrl(url.getAsString());
          RepositoryPolicy release = new RepositoryPolicy(); release.setChecksumPolicy("ignore"); repo.setReleases(release);
          RepositoryPolicy snapshots = new RepositoryPolicy(); snapshots.setEnabled(false); repo.setSnapshots(snapshots);
          root.addRepository(repo);
        }
        Path pom = work.resolve("pom.xml");
        try (var writer = Files.newBufferedWriter(pom)) { new MavenXpp3Writer().write(writer, root); }
        var building = new DefaultProjectBuildingRequest();
        building.setRepositorySession(session); building.setProcessPlugins(false); building.setResolveDependencies(true);
        building.setSystemProperties(declared); building.setUserProperties(new Properties());
        ProjectBuildingResult built = container.lookup(ProjectBuilder.class).build(pom.toFile(), building);
        var resolved = built.getDependencyResolutionResult();
        if (!resolved.getCollectionErrors().isEmpty()) throw resolved.getCollectionErrors().get(0);
        for (var unresolved : resolved.getUnresolvedDependencies()) {
          var errors = resolved.getResolutionErrors(unresolved);
          if (!errors.isEmpty()) throw errors.get(0);
          throw new IllegalStateException("Could not find artifact " + unresolved);
        }
        if (request.has("inspectModels")) {
          var inspection = new DefaultProjectBuildingRequest(building);
          inspection.setResolveDependencies(false);
          inspection.setRepositoryMerging(ProjectBuildingRequest.RepositoryMerging.REQUEST_DOMINANT);
          inspection.setRemoteRepositories(built.getProject().getRemoteArtifactRepositories());
          var legacy = container.lookup(org.apache.maven.repository.RepositorySystem.class);
          for (JsonElement item : request.getAsJsonArray("inspectModels")) {
            String[] gav = item.getAsString().split(":");
            Model effective = container.lookup(ProjectBuilder.class).build(
                legacy.createProjectArtifact(gav[0],gav[1],gav[2]), inspection).getProject().getModel();
            Map<String,Object> model = new TreeMap<>();
            model.put("dependencies", effective.getDependencies().stream().map(Oracle::depModel).toList());
            model.put("management", effective.getDependencyManagement() == null ? List.of() :
                effective.getDependencyManagement().getDependencies().stream().map(d -> {
                  var value = depModel(d); value.put("scope", d.getScope() == null ? "" : d.getScope()); return value;
                }).toList());
            var relocation = effective.getDistributionManagement() == null ? null : effective.getDistributionManagement().getRelocation();
            if (relocation != null) {
              Dependency target = dependency(new JsonPrimitive(item.getAsString()));
              if (relocation.getGroupId() != null) target.setGroupId(relocation.getGroupId());
              if (relocation.getArtifactId() != null) target.setArtifactId(relocation.getArtifactId());
              if (relocation.getVersion() != null) target.setVersion(relocation.getVersion());
              model.put("relocation", depModel(target));
            } else { model.put("relocation", null); }
            models.put(item.getAsString(), model);
          }
        }
        List<Map<String,Object>> artifacts = new ArrayList<>(), edges = new ArrayList<>();
        visit(built.getDependencyResolutionResult().getDependencyGraph(), List.of(), string(cp,"scope","compile"), artifacts, edges, new HashSet<>(), "root");
        classpaths.put(entry.getKey(), Map.of("artifacts", artifacts, "edges", edges,
            "dependencies", built.getProject().getDependencies().stream().map(Oracle::depModel).toList()));
      }
      output.println(JSON.toJson(request.has("inspectModels") ? Map.of("models", models) : request.has("origins") ? Map.of("classpaths", classpaths, "origins", origins) : Map.of("classpaths", classpaths)));
    } catch (Exception e) {
      output.println(JSON.toJson(Map.of("error", failureKind(e), "message", e.toString())));
      throw e;
    } finally {
      container.dispose();
      try (var paths = Files.walk(work)) {
        for (Path p : paths.sorted(Comparator.reverseOrder()).toList()) Files.delete(p);
      }
    }
  }
}
