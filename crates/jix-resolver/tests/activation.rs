mod support;
use jix_resolver::*;
use serde_json::json;
use support::*;

fn context() -> ResolutionContext {
    serde_json::from_value(json!({"systemProperties":{"java.version":"21.0.12","os.name":"Linux","os.arch":"amd64","os.version":"6.6","path.separator":":"}})).unwrap()
}

#[test]
fn declared_context_is_required_and_diagnostics_locate_profile() {
    let f = Fixture::new();
    f.pom("a","1","<profiles><profile><id>jdk</id><activation><jdk>21</jdk></activation></profile></profiles>");
    let mut r = f.request(json!(["fixture:a:1"]));
    let error = f.resolve(&r).unwrap_err().to_string();
    assert!(
        error.contains("fixture/a/1/a-1.pom")
            && error.contains("jdk")
            && error.contains("resolutionContext"),
        "{error}"
    );
    r.resolution_context = Some(ResolutionContext::default());
    assert!(
        f.resolve(&r)
            .unwrap_err()
            .to_string()
            .contains("java.version")
    );
    r.resolution_context = Some(context());
    f.resolve(&r).unwrap();
}

#[test]
fn absolute_file_lookup_is_never_silently_skipped() {
    let f = Fixture::new();
    // The false JDK condition must not suppress evaluation of the file activator.
    f.pom("a","1","<profiles><profile><id>lookup</id><activation><jdk>1.8</jdk><file><exists>/tmp/jix-profile-probe</exists></file></activation></profile></profiles>");
    let mut r = f.request(json!(["fixture:a:1"]));
    r.resolution_context = Some(context());
    let error = f.resolve(&r).unwrap_err().to_string();
    assert!(
        error.contains("lookup")
            && error.contains("/tmp/jix-profile-probe")
            && error.contains("filesystem"),
        "{error}"
    );
}

#[test]
fn whole_context_replacement_and_model_isolation() {
    let f = Fixture::new();
    f.pom("b", "1", "");
    f.pom("b", "2", "");
    f.pom("a","1",&format!("<profiles><profile><id>default</id><activation><activeByDefault>true</activeByDefault></activation>{}</profile><profile><id>switch</id><activation><property><name>flag</name></property></activation>{}</profile></profiles>",deps(&dep("b","1","")),deps(&dep("b","2",""))));
    let mut r = f.request(json!(["fixture:a:1"]));
    let mut shared = context();
    shared.user_properties.insert("flag".into(), "true".into());
    r.resolution_context = Some(shared);
    let mut other = r.classpaths["main"].clone();
    other.resolution_context = Some(ResolutionContext::default());
    r.classpaths.insert("other".into(), other);
    let normalized = r.normalized();
    assert!(normalized.resolution_context.is_none());
    assert!(
        normalized.classpaths["other"]
            .resolution_context
            .as_ref()
            .unwrap()
            .system_properties
            .is_empty()
    );
    let mut repo = DirectoryRepository::new(vec![f.directory.path().into()]).unwrap();
    let (result, models) = resolve_with_contextual_models(&r, &mut repo).unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(
        result.classpaths["main"].artifacts[1].id,
        "fixture:b:jar::2"
    );
    assert_eq!(
        result.classpaths["other"].artifacts[1].id,
        "fixture:b:jar::1"
    );
}

#[test]
fn unused_ranges_are_metadata_not_resolved_versions() {
    let f = Fixture::new();
    for scope in ["test", "provided"] {
        f.pom(
            "a",
            "1",
            &deps(&dep(
                "unused",
                "[4.13.1,)",
                &format!("<scope>{scope}</scope>"),
            )),
        );
        f.resolve(&f.request(json!(["fixture:a:1"]))).unwrap();
    }
    f.pom("a", "1", &deps(&dep("unused", "[4.13.1,)", "")));
    assert!(matches!(
        f.resolve(&f.request(json!(["fixture:a:1"])))
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    ));
}

#[test]
fn optional_system_metadata_still_requires_valid_declared_properties() {
    let f = Fixture::new();
    f.pom("a","1",&deps(&dep("tools","1","<scope>system</scope><optional>true</optional><systemPath>${java.home}/lib/jrt-fs.jar</systemPath>")));
    let mut request = f.request(json!(["fixture:a:1"]));
    request.resolution_context = Some(context());
    assert!(f.resolve(&request).is_err());
    request
        .resolution_context
        .as_mut()
        .unwrap()
        .system_properties
        .insert("java.home".into(), "/declared/jdk".into());
    assert_eq!(f.ids(&request), vec!["fixture:a:jar::1"]);
}

#[test]
fn synthetic_packaging_is_explicitly_unsupported_not_session_property() {
    let f = Fixture::new();
    f.pom("a", "1", "<profiles><profile><id>packaging</id><activation><property><name>packaging</name><value>jar</value></property></activation></profile></profiles>");
    let mut r = f.request(json!(["fixture:a:1"]));
    r.resolution_context = Some(context());
    assert!(
        f.resolve(&r)
            .unwrap_err()
            .to_string()
            .contains("synthetic packaging")
    );
}
