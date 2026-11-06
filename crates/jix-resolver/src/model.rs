use crate::{Dependency, Error, ErrorKind, RepositoryProvider, Result};
use roxmltree::Node;
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, Default)]
struct RawDependency {
    dep: Dependency,
    optional: Option<String>,
    system_path: Option<String>,
}
#[derive(Debug, Clone, Default)]
struct RawModel {
    group: String,
    artifact: String,
    version: String,
    parent: Option<Dependency>,
    properties: BTreeMap<String, String>,
    dependencies: Vec<RawDependency>,
    management: Vec<RawDependency>,
    relocation: Option<(String, String, String)>,
}
/// The dependency-relevant portion of an effective published POM.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EffectiveModel {
    pub dependencies: Vec<Dependency>,
    pub management: Vec<Dependency>,
    pub relocation: Option<Dependency>,
}

/// Inspect a published POM without collecting its transitive dependency graph.
pub fn inspect_model<R: RepositoryProvider>(
    coordinate: &Dependency,
    repository: &mut R,
) -> Result<EffectiveModel> {
    ModelBuilder::new(repository, None).build(coordinate)
}

pub(crate) struct ModelBuilder<'a, R> {
    repository: &'a mut R,
    context: Option<crate::ResolutionContext>,
    cache: HashMap<String, EffectiveModel>,
    stack: Vec<String>,
}
impl<'a, R: RepositoryProvider> ModelBuilder<'a, R> {
    pub fn new(repository: &'a mut R, context: Option<crate::ResolutionContext>) -> Self {
        Self {
            repository,
            context,
            cache: HashMap::new(),
            stack: Vec::new(),
        }
    }
    pub fn models(&self) -> BTreeMap<String, EffectiveModel> {
        self.cache
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
    pub fn build(&mut self, coordinate: &Dependency) -> Result<EffectiveModel> {
        coordinate.validate(true)?;
        let path = coordinate.pom_path();
        if let Some(model) = self.cache.get(&path) {
            return Ok(model.clone());
        }
        if self.stack.contains(&path) {
            return Err(Error::new(
                ErrorKind::Cycle,
                format!("Model cycle: {} -> {path}", self.stack.join(" -> ")),
            ));
        }
        if self.stack.len() >= 128 {
            return Err(Error::new(ErrorKind::Limit, "Model depth exceeds 128"));
        }
        self.stack.push(path.clone());
        let result = self.effective(coordinate);
        self.stack.pop();
        if let Ok(model) = &result {
            self.cache.insert(path, model.clone());
        }
        result
    }
    fn assemble(&mut self, coordinate: &Dependency, parents: &mut Vec<String>) -> Result<RawModel> {
        let path = coordinate.pom_path();
        if parents.contains(&path) {
            return Err(Error::new(
                ErrorKind::Cycle,
                format!("Parent cycle at {path}"),
            ));
        }
        if parents.len() >= 128 {
            return Err(Error::new(ErrorKind::Limit, "Parent depth exceeds 128"));
        }
        let bytes = self
            .repository
            .read(&path)?
            .ok_or_else(|| Error::new(ErrorKind::Missing, format!("Missing POM: {path}")))?;
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(Error::new(ErrorKind::Limit, "POM exceeds 4 MiB"));
        }
        let xml = std::str::from_utf8(&bytes).map_err(|e| Error::invalid(e.to_string()))?;
        let mut raw = parse(xml, self.context.as_ref()).map_err(|mut e| {
            e.message = format!("{path}: {}", e.message);
            e
        })?;
        if let Some(parent) = &raw.parent {
            let mut parent = parent.clone();
            interpolate_dependency(&mut parent, &raw.properties)?;
            parent.validate(true)?;
            parents.push(path);
            let inherited = self.assemble(&parent, parents)?;
            parents.pop();
            if raw.group.is_empty() {
                raw.group = inherited.group;
            }
            if raw.version.is_empty() {
                raw.version = inherited.version;
            }
            let mut props = inherited.properties;
            props.extend(raw.properties);
            raw.properties = props;
            raw.dependencies = merge_raw(raw.dependencies, inherited.dependencies);
            raw.management = merge_raw(raw.management, inherited.management);
        }
        Ok(raw)
    }
    fn effective(&mut self, coordinate: &Dependency) -> Result<EffectiveModel> {
        let mut raw = self.assemble(coordinate, &mut Vec::new())?;
        if let Some(context) = &self.context {
            let mut properties = context.properties();
            properties.extend(raw.properties);
            raw.properties = properties;
        }
        for (key, value) in [
            ("groupId", &raw.group),
            ("artifactId", &raw.artifact),
            ("version", &raw.version),
        ] {
            raw.properties
                .insert(format!("project.{key}"), value.clone());
            raw.properties.insert(format!("pom.{key}"), value.clone());
        }
        if let Some(parent) = &raw.parent {
            for (key, value) in [
                ("groupId", &parent.group),
                ("artifactId", &parent.artifact),
                ("version", &parent.version),
            ] {
                raw.properties
                    .insert(format!("project.parent.{key}"), value.clone());
                raw.properties
                    .insert(format!("parent.{key}"), value.clone());
            }
        }
        let mut local_management = Vec::new();
        let mut imports = Vec::new();
        for mut item in raw.management {
            interpolate_dependency(&mut item.dep, &raw.properties)?;
            if let Some(optional) = &item.optional {
                item.dep.optional =
                    interpolate(optional, &raw.properties)?.eq_ignore_ascii_case("true");
                item.dep.optional_value = Some(item.dep.optional);
            }
            if item.dep.scope == "import" && item.dep.kind == "pom" {
                imports.push(item.dep);
            } else {
                local_management.push(item.dep);
            }
        }
        // Local declarations dominate imports regardless of where an import appears.
        let mut management = local_management;
        for import in imports {
            for dep in self.build(&import)?.management {
                if !management.iter().any(|d| d.model_key() == dep.model_key()) {
                    management.push(dep);
                }
            }
        }
        let mut dependencies = Vec::new();
        for mut item in raw.dependencies {
            interpolate_dependency(&mut item.dep, &raw.properties)?;
            manage(&mut item.dep, &management, false);
            if let Some(optional) = item.optional {
                item.dep.optional =
                    interpolate(&optional, &raw.properties)?.eq_ignore_ascii_case("true");
                item.dep.optional_value = Some(item.dep.optional);
            }
            if item.dep.scope.is_empty() {
                item.dep.scope = "compile".into();
            }
            // Validate exact versions at graph admission, not for unused metadata.
            let mut structural = item.dep.clone();
            if !structural.version.is_empty() {
                structural.version = "1".into();
            }
            if structural.optional && structural.scope == "system" {
                let path = interpolate(item.system_path.as_deref().unwrap_or(""), &raw.properties)?;
                if !path.starts_with('/') {
                    return Err(Error::unsupported(
                        "Optional system dependency requires an explicit absolute systemPath",
                    ));
                }
                structural.scope = "compile".into();
            }
            structural.validate(true)?;
            if dependencies
                .iter()
                .any(|d: &Dependency| d.key() == item.dep.key())
            {
                return Err(Error::unsupported(
                    "Duplicate dependency identities after type mapping/interpolation",
                ));
            }
            dependencies.push(item.dep);
        }
        let relocation = if let Some((group, artifact, version)) = raw.relocation {
            let mut target = coordinate.clone();
            if !group.is_empty() {
                target.group = interpolate(&group, &raw.properties)?;
            }
            if !artifact.is_empty() {
                target.artifact = interpolate(&artifact, &raw.properties)?;
            }
            if !version.is_empty() {
                target.version = interpolate(&version, &raw.properties)?;
            }
            target.validate(true)?;
            Some(target)
        } else {
            None
        };
        Ok(EffectiveModel {
            dependencies,
            management,
            relocation,
        })
    }
}

pub(crate) fn finish(dep: &mut Dependency) -> Result<()> {
    if dep.scope.is_empty() {
        dep.scope = "compile".into();
    }
    dep.validate(true)
}
/// Root dependency management overrides transitive declarations, but fills missing
/// fields only when constructing the root model itself.
pub(crate) fn manage(dep: &mut Dependency, management: &[Dependency], transitive: bool) {
    if let Some(m) = management.iter().find(|m| m.model_key() == dep.model_key()) {
        if (dep.version.is_empty() || transitive) && !m.version.is_empty() {
            dep.version = m.version.clone();
        }
        if (dep.scope.is_empty() || transitive) && !m.scope.is_empty() {
            dep.scope = m.scope.clone();
        }
        if let Some(value) = m.optional_value
            && (transitive || dep.optional_value.is_none())
        {
            dep.optional = value;
            dep.optional_value = Some(value);
        }
        for e in &m.exclusions {
            if !dep.exclusions.contains(e) {
                dep.exclusions.push(e.clone());
            }
        }
    }
}
fn merge_profile(mut base: Vec<RawDependency>, profile: Vec<RawDependency>) -> Vec<RawDependency> {
    for item in profile {
        if let Some(index) = base
            .iter()
            .position(|d| d.dep.model_key() == item.dep.model_key())
        {
            base[index] = merge_raw(vec![item], vec![base[index].clone()]).remove(0);
        } else {
            base.push(item);
        }
    }
    base
}
fn merge_raw(mut child: Vec<RawDependency>, parent: Vec<RawDependency>) -> Vec<RawDependency> {
    for inherited in parent {
        if let Some(existing) = child
            .iter_mut()
            .find(|d| d.dep.model_key() == inherited.dep.model_key())
        {
            if existing.dep.version.is_empty() {
                existing.dep.version = inherited.dep.version;
            }
            if existing.dep.scope.is_empty() {
                existing.dep.scope = inherited.dep.scope;
            }
            if existing.system_path.is_none() {
                existing.system_path = inherited.system_path;
            }
            if existing.optional.is_none() {
                existing.optional = inherited.optional;
                existing.dep.optional = inherited.dep.optional;
            }
            for e in inherited.dep.exclusions {
                if !existing.dep.exclusions.contains(&e) {
                    existing.dep.exclusions.push(e);
                }
            }
        } else {
            child.push(inherited);
        }
    }
    child
}
fn child<'a, 'input>(node: Node<'a, 'input>, tag: &str) -> Option<Node<'a, 'input>> {
    node.children()
        .find(|n| n.is_element() && n.tag_name().name() == tag)
}
fn text(node: Node<'_, '_>, tag: &str) -> String {
    child(node, tag)
        .and_then(|n| n.text())
        .unwrap_or("")
        .trim()
        .into()
}
fn dependencies(node: Node<'_, '_>) -> Vec<RawDependency> {
    child(node, "dependencies")
        .into_iter()
        .flat_map(|n| n.children())
        .filter(|n| n.has_tag_name("dependency"))
        .map(|n| {
            let kind = text(n, "type");
            let optional = child(n, "optional").map(|_| text(n, "optional"));
            RawDependency {
                dep: Dependency {
                    group: text(n, "groupId"),
                    artifact: text(n, "artifactId"),
                    version: text(n, "version"),
                    kind: if kind.is_empty() { "jar".into() } else { kind },
                    classifier: text(n, "classifier"),
                    scope: text(n, "scope"),
                    optional: false,
                    optional_value: None,
                    exclusions: child(n, "exclusions")
                        .into_iter()
                        .flat_map(|n| n.children())
                        .filter(|n| n.has_tag_name("exclusion"))
                        .map(|n| format!("{}:{}", text(n, "groupId"), text(n, "artifactId")))
                        .collect(),
                },
                optional,
                system_path: child(n, "systemPath").map(|_| text(n, "systemPath")),
            }
        })
        .collect()
}
fn properties(node: Node<'_, '_>) -> BTreeMap<String, String> {
    child(node, "properties")
        .into_iter()
        .flat_map(|n| n.children())
        .filter(Node::is_element)
        .map(|n| {
            (
                n.tag_name().name().into(),
                n.text().unwrap_or("").trim().into(),
            )
        })
        .collect()
}
fn parse(xml: &str, context: Option<&crate::ResolutionContext>) -> Result<RawModel> {
    let doc = roxmltree::Document::parse(xml)
        .map_err(|e| Error::invalid(format!("Invalid POM XML: {e}")))?;
    let root = doc.root_element();
    if root.tag_name().name() != "project" {
        return Err(Error::invalid("Expected a project POM"));
    }
    if text(root, "modelVersion") != "4.0.0" {
        return Err(Error::unsupported(
            "Only POM modelVersion 4.0.0 is supported",
        ));
    }
    for node in root
        .descendants()
        .filter(|n| n.has_tag_name("dependencies"))
    {
        let mut keys = std::collections::HashSet::new();
        for d in node.children().filter(|n| n.has_tag_name("dependency")) {
            for tag in ["groupId", "artifactId", "version", "type", "scope"] {
                if child(d, tag).is_some() && text(d, tag).is_empty() {
                    return Err(Error::invalid(format!(
                        "Empty dependency {tag}; omit managed fields instead"
                    )));
                }
            }
            let key = (
                text(d, "groupId"),
                text(d, "artifactId"),
                text(d, "type"),
                text(d, "classifier"),
            );
            if !keys.insert(key) {
                return Err(Error::unsupported("Duplicate POM dependency declarations"));
            }
        }
    }
    let parent = child(root, "parent").map(|n| Dependency {
        group: text(n, "groupId"),
        artifact: text(n, "artifactId"),
        version: text(n, "version"),
        kind: "pom".into(),
        scope: "compile".into(),
        ..Dependency::default()
    });
    let mut result = RawModel {
        group: text(root, "groupId"),
        artifact: text(root, "artifactId"),
        version: text(root, "version"),
        parent,
        properties: properties(root),
        dependencies: dependencies(root),
        management: child(root, "dependencyManagement")
            .map(dependencies)
            .unwrap_or_default(),
        relocation: child(root, "distributionManagement")
            .and_then(|n| child(n, "relocation"))
            .map(|n| {
                (
                    text(n, "groupId"),
                    text(n, "artifactId"),
                    text(n, "version"),
                )
            }),
    };
    for profile in crate::activation::select(root, context)? {
        result.properties.extend(properties(profile));
        result.dependencies = merge_profile(result.dependencies, dependencies(profile));
        result.management = merge_profile(
            result.management,
            child(profile, "dependencyManagement")
                .map(dependencies)
                .unwrap_or_default(),
        );
    }
    Ok(result)
}
fn interpolate_dependency(
    dep: &mut Dependency,
    properties: &BTreeMap<String, String>,
) -> Result<()> {
    for value in [
        &mut dep.group,
        &mut dep.artifact,
        &mut dep.version,
        &mut dep.kind,
        &mut dep.classifier,
        &mut dep.scope,
    ] {
        *value = interpolate(value, properties)?;
    }
    for value in &mut dep.exclusions {
        *value = interpolate(value, properties)?;
    }
    Ok(())
}
fn interpolate(value: &str, properties: &BTreeMap<String, String>) -> Result<String> {
    let mut value = value.to_string();
    for _ in 0..64 {
        let Some(start) = value.find("${") else {
            return Ok(value);
        };
        let end = value[start + 2..]
            .find('}')
            .ok_or_else(|| Error::invalid("Unclosed property expression"))?
            + start
            + 2;
        let key = &value[start + 2..end];
        if key == "packaging" {
            return Err(Error::unsupported(
                "Maven synthetic packaging property is not supported",
            ));
        }
        let replacement = properties
            .get(key)
            .ok_or_else(|| Error::unsupported(format!("Unresolved or ambient property: {key}")))?;
        value.replace_range(start..=end, replacement);
        if value.len() > 1024 * 1024 {
            return Err(Error::new(
                ErrorKind::Limit,
                "Property expansion exceeds 1 MiB",
            ));
        }
    }
    Err(Error::new(
        ErrorKind::Cycle,
        "Property interpolation cycle or depth exceeds 64",
    ))
}

/// Inspect a published descriptor under an explicitly declared resolution context.
pub fn inspect_model_with_context<R: RepositoryProvider>(
    coordinate: &Dependency,
    repository: &mut R,
    context: &crate::ResolutionContext,
) -> Result<EffectiveModel> {
    ModelBuilder::new(repository, Some(context.clone())).build(coordinate)
}
