use crate::{
    model::{ModelBuilder, finish, manage},
    *,
};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

#[derive(Clone)]
struct Node {
    dep: Dependency,
    declared_scope: String,
    parent: Option<usize>,
    children: Vec<usize>,
    depth: usize,
}

/// Resolve named classpaths against an explicit repository provider.
///
/// Implements the release-only, strict-descriptor contract in COMPATIBILITY.md.
pub fn resolve<R: RepositoryProvider>(
    request: &DependencySpec,
    repository: &mut R,
) -> Result<ResolutionResult> {
    resolve_with_contextual_models(request, repository).map(|(result, _)| result)
}

pub type ContextualModels = BTreeMap<String, BTreeMap<String, EffectiveModel>>;

/// Compatibility API for callers inspecting a single effective model per coordinate.
pub fn resolve_with_models<R: RepositoryProvider>(
    request: &DependencySpec,
    repository: &mut R,
) -> Result<(ResolutionResult, BTreeMap<String, EffectiveModel>)> {
    let (result, contexts) = resolve_with_contextual_models(request, repository)?;
    let mut models = BTreeMap::new();
    for context in contexts.into_values() {
        for (path, model) in context {
            if models.get(&path).is_some_and(|old| old != &model) {
                return Err(Error::unsupported(
                    "Multiple effective models; use resolve_with_contextual_models",
                ));
            }
            models.insert(path, model);
        }
    }
    Ok((result, models))
}

pub fn resolve_with_contextual_models<R: RepositoryProvider>(
    request: &DependencySpec,
    repository: &mut R,
) -> Result<(ResolutionResult, ContextualModels)> {
    let request = request.normalized();
    if request.repositories.is_empty() {
        return Err(Error::invalid("At least one repository is required"));
    }
    let mut models = ContextualModels::new();
    let mut classpaths = BTreeMap::new();
    for (name, cp) in &request.classpaths {
        let mut builder = ModelBuilder::new(repository, cp.resolution_context.clone());
        if name.trim().is_empty() {
            return Err(Error::invalid("Classpath names must not be empty"));
        }
        let mut management = Vec::new();
        for platform in &cp.platforms {
            let mut dep =
                Dependency::from_spec(&DependencyDeclaration::Coordinate(platform.clone()))?;
            dep.kind = "pom".into();
            dep.validate(true)?;
            for managed in builder.build(&dep)?.management {
                if !management
                    .iter()
                    .any(|d: &Dependency| d.model_key() == managed.model_key())
                {
                    management.push(managed);
                }
            }
        }
        let mut dependencies = Vec::new();
        let mut nodes = Vec::new();
        let mut queue = VecDeque::new();
        let mut roots = Vec::new();
        for spec in &cp.dependencies {
            let mut dep = Dependency::from_spec(spec)?;
            manage(&mut dep, &management, false);
            finish(&mut dep)?;
            if dependencies
                .iter()
                .any(|d: &Dependency| d.key() == dep.key())
            {
                return Err(Error::unsupported(
                    "Duplicate root dependencies must be normalized explicitly",
                ));
            }
            dependencies.push(dep.clone());
            roots.push(nodes.len());
            queue.push_back(nodes.len());
            nodes.push(Node {
                declared_scope: dep.scope.clone(),
                dep,
                parent: None,
                children: Vec::new(),
                depth: 1,
            });
        }
        // Breadth-first selection implements nearest definition / first declaration.
        // Output traversal is preorder, not selection order.
        let mut winners: HashMap<String, usize> = HashMap::new();
        while let Some(index) = queue.pop_front() {
            if nodes.len() > 10_000 || nodes[index].depth > 128 {
                return Err(Error::new(
                    ErrorKind::Limit,
                    "Dependency graph exceeds safety limits",
                ));
            }
            let mut dep = nodes[index].dep.clone();
            let mut relocated = HashSet::new();
            let model = loop {
                if !relocated.insert(dep.id()) {
                    return Err(Error::new(ErrorKind::Cycle, "Artifact relocation cycle"));
                }
                if relocated.len() > 128 {
                    return Err(Error::new(ErrorKind::Limit, "Relocation depth exceeds 128"));
                }
                let model = builder.build(&dep)?;
                if let Some(target) = model.relocation {
                    dep.group = target.group;
                    dep.artifact = target.artifact;
                    dep.version = target.version;
                } else {
                    break model;
                }
            };
            nodes[index].dep = dep.clone();
            if winners.contains_key(&dep.key()) {
                continue;
            }
            winners.insert(dep.key(), index);
            // Maven treats these artifact types as containing their own dependencies.
            if matches!(dep.kind.as_str(), "war" | "ear" | "rar" | "par") {
                continue;
            }
            for mut child in model.dependencies {
                // Maven's selector excludes provided/test and optional transitive dependencies.
                if child.optional || matches!(child.scope.as_str(), "test" | "provided") {
                    continue;
                }
                manage(&mut child, &management, true);
                finish(&mut child)?;
                let mut ancestor = Some(index);
                let mut excluded = false;
                while let Some(a) = ancestor {
                    excluded |= nodes[a]
                        .dep
                        .exclusions
                        .iter()
                        .any(|e| matches_exclusion(e, &child));
                    ancestor = nodes[a].parent;
                }
                if excluded {
                    continue;
                }
                let declared_scope = child.scope.clone();
                child.scope = match dep.scope.as_str() {
                    "runtime" => "runtime".into(),
                    "test" => "test".into(),
                    "provided" => "provided".into(),
                    _ => child.scope,
                };
                let next = nodes.len();
                nodes[index].children.push(next);
                nodes.push(Node {
                    dep: child,
                    declared_scope,
                    parent: Some(index),
                    children: Vec::new(),
                    depth: nodes[index].depth + 1,
                });
                queue.push_back(next);
            }
        }
        // A nearer version does not imply its narrower scope wins. Scope selection
        // combines surviving paths; widening a parent must reach its children.
        // Reference: Maven Resolver 1.9.24 JavaScopeSelector / JavaScopeDeriver.
        for _ in 0..128 {
            let mut scopes: HashMap<String, String> = HashMap::new();
            for node in &nodes {
                let derived = if let Some(parent) = node.parent {
                    if nodes[parent].dep.scope == "runtime" {
                        "runtime"
                    } else {
                        &node.declared_scope
                    }
                } else {
                    &node.declared_scope
                };
                let winner = winners[&node.dep.key()];
                let selected = if nodes[winner].parent.is_none() {
                    nodes[winner].declared_scope.as_str()
                } else {
                    derived
                };
                scopes
                    .entry(node.dep.key())
                    .and_modify(|s| {
                        if selected == "compile" {
                            *s = selected.into();
                        }
                    })
                    .or_insert_with(|| selected.into());
            }
            let mut changed = false;
            for index in winners.values() {
                let scope = &scopes[&nodes[*index].dep.key()];
                changed |= nodes[*index].dep.scope != *scope;
                nodes[*index].dep.scope = scope.clone();
            }
            if !changed {
                break;
            }
        }
        let mut artifacts = Vec::new();
        let mut edges = Vec::new();
        for root in roots {
            emit(
                root,
                "root",
                &nodes,
                &winners,
                cp.scope,
                &mut artifacts,
                &mut edges,
            );
        }
        classpaths.insert(
            name.clone(),
            ClasspathResult {
                dependencies,
                artifacts,
                edges,
            },
        );
        models
            .entry(context_hash(cp.resolution_context.as_ref()))
            .or_default()
            .extend(builder.models());
    }
    Ok((ResolutionResult { classpaths }, models))
}
fn matches_exclusion(exclusion: &str, dep: &Dependency) -> bool {
    exclusion
        .split_once(':')
        .is_some_and(|(g, a)| (g == "*" || g == dep.group) && (a == "*" || a == dep.artifact))
}
fn emit(
    index: usize,
    parent: &str,
    nodes: &[Node],
    winners: &HashMap<String, usize>,
    scope: ClasspathScope,
    artifacts: &mut Vec<SelectedArtifact>,
    edges: &mut Vec<Edge>,
) {
    let dep = &nodes[index].dep;
    if winners.get(&dep.key()) != Some(&index) {
        return;
    }
    let id = dep.id();
    edges.push(Edge {
        from: parent.into(),
        to: id.clone(),
        scope: dep.scope.clone(),
    });
    if scope.includes(&dep.scope) {
        artifacts.push(SelectedArtifact {
            id: id.clone(),
            scope: dep.scope.clone(),
        });
    }
    for child in &nodes[index].children {
        emit(*child, &id, nodes, winners, scope, artifacts, edges);
    }
}
