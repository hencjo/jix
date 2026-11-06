{ maven }:
{
  repositories = [ "https://repo.maven.apache.org/maven2" ];
  classpaths.main = maven.resolve {
    scope = "compile";
    dependencies = [ "com.google.protobuf:protobuf-java:4.33.4" ];
  };
}
