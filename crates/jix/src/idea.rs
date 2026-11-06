//! Owned IntelliJ project files; pure model rendering separated from publication.
use crate::repository::sha256;
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Model {
    version: u32,
    source_root: PathBuf,
    modules: Vec<Module>,
    applications: Vec<App>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Module {
    name: String,
    sources: Sources,
    resources: Vec<Resource>,
    jdk: String,
    release: u32,
    javac_options: Vec<String>,
    compile: Paths,
    runtime: Paths,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum Sources {
    Legacy(PathBuf),
    Roots(Vec<SourceRoot>),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceRoot {
    path: PathBuf,
    generated: bool,
}
impl Sources {
    fn resolve(&self, root: &Path, source_root: &Path) -> Result<Vec<(PathBuf, bool)>> {
        match self {
            Self::Legacy(path) => Ok(vec![(local(root, source_root, path)?, false)]),
            Self::Roots(sources) => sources
                .iter()
                .map(|source| {
                    if source.generated {
                        ensure!(
                            source.path.starts_with("/nix/store")
                                && source.path.is_dir()
                                && !source
                                    .path
                                    .components()
                                    .any(|c| matches!(c, std::path::Component::ParentDir)),
                            "Generated IDE source must be a realized store directory: {}",
                            source.path.display()
                        );
                        Ok((source.path.clone(), true))
                    } else {
                        Ok((local(root, source_root, &source.path)?, false))
                    }
                })
                .collect(),
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Resource {
    from: PathBuf,
    into: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Paths {
    classpath: Vec<Reference>,
    modulepath: Vec<Reference>,
}
impl Paths {
    fn all(&self) -> impl Iterator<Item = &Reference> {
        self.classpath.iter().chain(&self.modulepath)
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Reference {
    module: Option<String>,
    path: Option<String>,
    id: Option<String>,
    jar_name: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct App {
    name: String,
    main_class: Option<String>,
    module: Option<String>,
    jdk: String,
    runtime: Paths,
}
#[derive(Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct State {
    version: u32,
    files: BTreeMap<String, String>,
}
#[derive(Clone, PartialEq, Eq)]
enum Output {
    Text(String),
    Link(String),
}
impl Output {
    fn hash(&self) -> String {
        match self {
            Self::Text(s) => sha256(format!("file:{s}").as_bytes()),
            Self::Link(s) => sha256(format!("link:{s}").as_bytes()),
        }
    }
}
fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn name(s: &str) -> Result<()> {
    ensure!(
        !matches!(s, "" | "." | "..")
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
        "Invalid IDE name: {s}"
    );
    Ok(())
}
fn url(root: &Path, path: &Path) -> Result<String> {
    if path.starts_with("/nix/store") {
        let value = path.to_str().context("Non-UTF8 IDE path")?;
        ensure!(
            !value.contains('$') && !value.chars().any(char::is_control),
            "Unsafe IDE store path"
        );
        return Ok(format!("file://{value}"));
    }
    let relative = path
        .strip_prefix(root)
        .context("IDE source is outside the working tree")?;
    // IntelliJ VFS URLs are literal paths, not percent-encoded RFC URLs.
    let value = relative.to_str().context("Non-UTF8 IDE path")?;
    ensure!(
        !value.contains('$') && !value.chars().any(char::is_control),
        "IDE source paths cannot contain macros or control characters"
    );
    Ok(format!("file://$PROJECT_DIR$/{value}"))
}
fn sdk(home: &str) -> Result<String> {
    ensure!(
        home.starts_with("/nix/store/") && Path::new(home).join("bin/javac").is_file(),
        "Invalid pinned JDK: {home}"
    );
    Ok(format!("jix-{}", &sha256(home.as_bytes())[..12]))
}
fn local(root: &Path, source: &Path, path: &Path) -> Result<PathBuf> {
    let relative = path
        .strip_prefix(source)
        .context("IDE roots must belong to the local project")?;
    ensure!(
        !relative
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir)),
        "Unsafe IDE source path"
    );
    let result = root.join(relative);
    ensure!(
        result.is_dir(),
        "IDE source/resource directory missing: {}",
        result.display()
    );
    ensure!(
        result.canonicalize()?.starts_with(root),
        "IDE source escapes project: {}",
        result.display()
    );
    Ok(result)
}
fn alias(reference: &Reference, outputs: &mut BTreeMap<String, Output>) -> Result<String> {
    let path = reference.path.as_ref().context("Missing IDE binary path")?;
    ensure!(
        path.starts_with("/nix/store/") && Path::new(path).is_file(),
        "IDE dependency is not a store file: {path}"
    );
    let filename = reference
        .jar_name
        .as_ref()
        .context("Missing stable IDE JAR filename")?;
    name(filename)?;
    ensure!(filename.ends_with(".jar"), "IDE dependency is not a JAR");
    let key = format!(
        ".jix/idea-libs/{}/{}",
        &sha256(path.as_bytes())[..20],
        filename
    );
    outputs.insert(key.clone(), Output::Link(path.clone()));
    Ok(key)
}
fn reference_key(r: &Reference) -> Result<String> {
    match (&r.module, &r.path) {
        (Some(m), None) => Ok(format!("module:{m}")),
        (None, Some(p)) => Ok(format!("jar:{p}")),
        _ => bail!("Invalid IDE dependency reference"),
    }
}
type Rendered = (BTreeMap<String, Output>, BTreeMap<String, String>);

fn render(root: &Path, model: Model) -> Result<Rendered> {
    ensure!(
        matches!(model.version, 1 | 2),
        "Unsupported IDE model version"
    );
    ensure!(
        !model.modules.is_empty(),
        "Project has no local Java libraries to export"
    );
    let mut outputs = BTreeMap::new();
    let mut jdks = BTreeMap::new();
    let mut modules =
        String::from("<project version=\"4\"><component name=\"ProjectModuleManager\"><modules>");
    let mut compiler = String::from(
        "<project version=\"4\"><component name=\"CompilerConfiguration\"><option name=\"USE_RELEASE_OPTION\" value=\"true\"/><annotationProcessing><profile default=\"true\" name=\"Default\" enabled=\"false\"/></annotationProcessing><bytecodeTargetLevel>",
    );
    let mut options = String::from(
        "<component name=\"JavacSettings\"><option name=\"ADDITIONAL_OPTIONS_OVERRIDE\">",
    );
    let ids: BTreeSet<_> = model.modules.iter().map(|m| m.name.as_str()).collect();
    ensure!(
        ids.len() == model.modules.len(),
        "Duplicate IDE module identities"
    );
    let mut source_roots = BTreeMap::new();
    for m in &model.modules {
        name(&m.name)?;
        ensure!(
            m.javac_options
                .iter()
                .all(|option| crate::build::allowed_option(option)),
            "Unsupported javac option in IDE module {}",
            m.name
        );
        ensure!(
            m.release >= 8 && m.release <= 25,
            "Unsupported IntelliJ language level: {}",
            m.release
        );
        let sources = m.sources.resolve(root, &model.source_root)?;
        let java_sources = crate::build::java_sources(
            &sources
                .iter()
                .map(|(path, _)| path.clone())
                .collect::<Vec<_>>(),
        )?;
        ensure!(
            m.compile.modulepath.is_empty()
                || java_sources.iter().any(|(_, path)| path
                    .file_name()
                    .is_some_and(|name| name == "module-info.java")),
            "IntelliJ cannot preserve explicit module-path placement for unnamed source module {}",
            m.name
        );
        let mut roots: Vec<_> = sources
            .into_iter()
            .map(|(path, generated)| (path, false, generated))
            .collect();
        for r in &m.resources {
            ensure!(
                r.into.is_empty(),
                "IntelliJ cannot reproduce remapped resource roots in {}",
                m.name
            );
            roots.push((local(root, &model.source_root, &r.from)?, true, false));
        }
        for (path, _, _) in &roots {
            ensure!(
                !source_roots
                    .keys()
                    .any(|old: &PathBuf| old.starts_with(path) || path.starts_with(old)),
                "Overlapping IDE content root: {}",
                path.display()
            );
            source_roots.insert(path.clone(), m.name.clone());
        }
        let sdk_name = sdk(&m.jdk)?;
        jdks.insert(sdk_name.clone(), m.jdk.clone());
        let preview = m.javac_options.iter().any(|o| o == "--enable-preview");
        let level = if m.release == 8 {
            "JDK_1_8".into()
        } else {
            format!("JDK_{}{}", m.release, if preview { "_PREVIEW" } else { "" })
        };
        let mut text = format!(
            "<module type=\"JAVA_MODULE\" version=\"4\"><component name=\"NewModuleRootManager\" LANGUAGE_LEVEL=\"{level}\" inherit-compiler-output=\"false\"><output url=\"file://$PROJECT_DIR$/.jix/idea-out/{}/production\"/><exclude-output/>",
            m.name
        );
        for (path, resource, generated) in roots {
            let u = xml(&url(root, &path)?);
            text.push_str(&format!(
                "<content url=\"{u}\"><sourceFolder url=\"{u}\" {}/></content>",
                if resource {
                    "type=\"java-resource\""
                } else if generated {
                    "isTestSource=\"false\" generated=\"true\""
                } else {
                    "isTestSource=\"false\""
                }
            ));
        }
        text.push_str(&format!("<orderEntry type=\"jdk\" jdkName=\"{sdk_name}\" jdkType=\"JavaSDK\"/><orderEntry type=\"sourceFolder\" forTests=\"false\"/>"));
        let compile: BTreeSet<_> = m.compile.all().map(reference_key).collect::<Result<_>>()?;
        let mut seen = BTreeSet::new();
        for r in m.compile.all().chain(m.runtime.all()) {
            let key = reference_key(r)?;
            if !seen.insert(key.clone()) {
                continue;
            }
            let scope = if compile.contains(&key) {
                "COMPILE"
            } else {
                "RUNTIME"
            };
            if let Some(module) = &r.module {
                ensure!(
                    ids.contains(module.as_str()),
                    "Missing IDE module: {module}"
                );
                text.push_str(&format!("<orderEntry type=\"module\" module-name=\"{}\" exported=\"\" scope=\"{scope}\"/>",xml(module)));
            } else {
                let path = alias(r, &mut outputs)?;
                text.push_str(&format!("<orderEntry type=\"module-library\" exported=\"\" scope=\"{scope}\"><library name=\"{}\"><CLASSES><root url=\"jar://$PROJECT_DIR$/{path}!/\"/></CLASSES><JAVADOC/><SOURCES/></library></orderEntry>",xml(r.id.as_deref().unwrap_or("dependency"))));
            }
        }
        text.push_str("</component></module>\n");
        let file = format!(".idea/jix-modules/{}.iml", m.name);
        outputs.insert(
            file.clone(),
            Output::Text(text.replace("$PROJECT_DIR$", "$MODULE_DIR$/../..")),
        );
        modules.push_str(&format!(
            "<module fileurl=\"file://$PROJECT_DIR$/{file}\" filepath=\"$PROJECT_DIR$/{file}\"/>"
        ));
        compiler.push_str(&format!(
            "<module name=\"{}\" target=\"{}\"/>",
            m.name, m.release
        ));
        let opts = format!("-proc:none -implicit:none {}", m.javac_options.join(" "));
        options.push_str(&format!(
            "<module name=\"{}\" options=\"{}\"/>",
            m.name,
            xml(&opts)
        ));
    }
    modules.push_str("</modules></component></project>\n");
    compiler.push_str("</bytecodeTargetLevel></component>");
    options.push_str("</option></component>");
    compiler.push_str(&options);
    compiler.push_str("</project>\n");
    outputs.insert(".idea/modules.xml".into(), Output::Text(modules));
    outputs.insert(".idea/compiler.xml".into(), Output::Text(compiler));
    let first = &model.modules[0];
    outputs.insert(".idea/misc.xml".into(), Output::Text(format!("<project version=\"4\"><component name=\"ProjectRootManager\" version=\"2\" languageLevel=\"JDK_{}\" project-jdk-name=\"{}\" project-jdk-type=\"JavaSDK\"><output url=\"file://$PROJECT_DIR$/.jix/idea-out\"/></component></project>\n",if first.release==8 {"1_8".into()} else {first.release.to_string()},sdk(&first.jdk)?)));
    for app in &model.applications {
        name(&app.name)?;
        let sdk_name = sdk(&app.jdk)?;
        jdks.insert(sdk_name, app.jdk.clone());
        let main = app
            .main_class
            .as_deref()
            .or_else(|| {
                app.module
                    .as_deref()
                    .and_then(|s| s.split_once('/').map(|(_, m)| m))
            })
            .context("Application has no Java main class")?;
        let relative = format!("{}.java", main.split('$').next().unwrap().replace('.', "/"));
        let owners = model
            .modules
            .iter()
            .filter(|m| {
                m.sources
                    .resolve(root, &model.source_root)
                    .is_ok_and(|sources| {
                        sources
                            .iter()
                            .any(|(path, _)| path.join(&relative).is_file())
                    })
            })
            .collect::<Vec<_>>();
        ensure!(
            owners.len() == 1,
            "Cannot identify a unique source module for IDE main class: {main}"
        );
        let owner = owners[0];
        ensure!(
            app.runtime.modulepath.is_empty() || app.module.is_some(),
            "IntelliJ cannot preserve explicit module-path placement for classpath application {}",
            app.name
        );
        // Native Application configurations use the module's graph. Do not silently omit app-only inputs.
        let keys = |refs: &[Reference]| refs.iter().map(reference_key).collect::<Result<Vec<_>>>();
        let mut cp = keys(&owner.runtime.classpath)?;
        let mut mp = keys(&owner.runtime.modulepath)?;
        if app.module.is_some() {
            mp.insert(0, format!("module:{}", owner.name));
        } else {
            cp.insert(0, format!("module:{}", owner.name));
        }
        ensure!(
            keys(&app.runtime.classpath)? == cp && keys(&app.runtime.modulepath)? == mp,
            "IDE app {} changes its library's runtime order, placement or dependencies; declare its runtime on the library",
            app.name
        );
        let vm = if owner.javac_options.iter().any(|s| s == "--enable-preview") {
            "--enable-preview"
        } else {
            ""
        };
        let text = format!(
            "<component name=\"ProjectRunConfigurationManager\"><configuration default=\"false\" name=\"Jix {}\" type=\"Application\" factoryName=\"Application\"><option name=\"MAIN_CLASS_NAME\" value=\"{}\"/><module name=\"{}\"/><option name=\"ALTERNATIVE_JRE_PATH_ENABLED\" value=\"true\"/><option name=\"ALTERNATIVE_JRE_PATH\" value=\"{}\"/><option name=\"WORKING_DIRECTORY\" value=\"$PROJECT_DIR$\"/><option name=\"VM_PARAMETERS\" value=\"{vm}\"/><method v=\"2\"><option name=\"Make\" enabled=\"true\"/></method></configuration></component>\n",
            xml(&app.name),
            xml(main),
            xml(&owner.name),
            xml(&app.jdk)
        );
        outputs.insert(
            format!(".idea/runConfigurations/jix-{}.xml", app.name),
            Output::Text(text),
        );
    }
    Ok((outputs, jdks))
}

fn owned_path(relative: &str) -> bool {
    matches!(
        relative,
        ".idea/modules.xml" | ".idea/compiler.xml" | ".idea/misc.xml" | ".jix/idea-state.json"
    ) || relative
        .strip_prefix(".idea/jix-modules/")
        .is_some_and(|p| p.ends_with(".iml") && !p.contains('/'))
        || relative
            .strip_prefix(".idea/runConfigurations/jix-")
            .is_some_and(|p| p.ends_with(".xml") && !p.contains('/'))
        || relative.strip_prefix(".jix/idea-libs/").is_some_and(|p| {
            p.split_once('/').is_some_and(|(hash, file)| {
                hash.len() == 20
                    && hash.bytes().all(|b| b.is_ascii_hexdigit())
                    && !file.contains('/')
                    && file.ends_with(".jar")
            })
        })
}
fn check_path(root: &Path, relative: &str) -> Result<PathBuf> {
    crate::build::safe_path(relative)?;
    ensure!(owned_path(relative), "Unowned IDE path: {relative}");
    let path = root.join(relative);
    let mut parent = path.parent();
    while let Some(p) = parent {
        if p == root {
            break;
        }
        if let Ok(m) = fs::symlink_metadata(p) {
            ensure!(
                m.is_dir() && !m.file_type().is_symlink(),
                "Unsafe IDE parent: {}",
                p.display()
            );
        }
        parent = p.parent();
    }
    Ok(path)
}
fn current(path: &Path) -> Result<Option<Output>> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
        Ok(m) if m.file_type().is_symlink() => Ok(Some(Output::Link(
            fs::read_link(path)?
                .to_str()
                .context("Non-UTF8 symlink")?
                .into(),
        ))),
        Ok(m) if m.is_file() => Ok(Some(Output::Text(fs::read_to_string(path)?))),
        _ => bail!("IDE destination is not a file: {}", path.display()),
    }
}
fn write(path: &Path, value: &Output) -> Result<()> {
    fs::create_dir_all(path.parent().unwrap())?;
    let temp = tempfile::tempdir_in(path.parent().unwrap())?;
    let staged = temp.path().join("file");
    match value {
        Output::Text(text) => {
            let mut f = fs::File::create(&staged)?;
            f.write_all(text.as_bytes())?;
            f.sync_all()?;
        }
        Output::Link(target) => std::os::unix::fs::symlink(target, &staged)?,
    }
    fs::rename(staged, path)?;
    Ok(())
}
pub fn generate(project: &Path, json: &str) -> Result<BTreeMap<String, String>> {
    let root = project.canonicalize()?;
    let model: Model = serde_json::from_str(json).context("Invalid IDE model")?;
    let (outputs, jdks) = render(&root, model)?;
    let lock_path = check_path(&root, ".jix/idea-state.json")?.with_file_name("idea-write.lock");
    fs::create_dir_all(lock_path.parent().unwrap())?;
    let lock = fs::OpenOptions::new().write(true).create_new(true).open(&lock_path)
        .context("Another jix idea writer is active; remove .jix/idea-write.lock only if that writer has stopped")?;
    struct Guard(PathBuf, fs::File);
    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = self.1.sync_all();
            let _ = fs::remove_file(&self.0);
        }
    }
    let _guard = Guard(lock_path, lock);
    let state_path = check_path(&root, ".jix/idea-state.json")?;
    if let Ok(m) = fs::symlink_metadata(&state_path) {
        ensure!(!m.file_type().is_symlink(), "IDE state cannot be a symlink");
    }
    let old: State = if state_path.exists() {
        serde_json::from_slice(&fs::read(&state_path)?)?
    } else {
        State {
            version: 1,
            ..Default::default()
        }
    };
    ensure!(old.version == 1, "Unsupported IDE ownership state");
    let mut before = BTreeMap::new();
    for relative in old
        .files
        .keys()
        .chain(outputs.keys())
        .collect::<BTreeSet<_>>()
    {
        ensure!(
            relative != ".idea/workspace.xml" && relative != ".jix/idea-state.json",
            "Reserved IDE state path"
        );
        let path = check_path(&root, relative)?;
        let previous = current(&path)?;
        match (old.files.get(relative), &previous) {
            (None, Some(_)) => bail!(
                "Unmanaged IntelliJ file: {relative}; migrate existing Maven/Gradle IDE configuration explicitly"
            ),
            (Some(hash), Some(value)) => ensure!(
                hash == &value.hash(),
                "Modified IntelliJ file: {relative}; preserve your edits before regeneration"
            ),
            (Some(_), None) => {
                bail!("Owned IntelliJ file was removed: {relative}; restore it before regeneration")
            }
            _ => {}
        }
        before.insert(relative.clone(), previous);
    }
    let state = State {
        version: 1,
        files: outputs.iter().map(|(k, v)| (k.clone(), v.hash())).collect(),
    };
    // Prepare every replacement before touching any project files.
    let staging = tempfile::tempdir_in(root.join(".jix"))?;
    for (index, value) in outputs.values().enumerate() {
        write(&staging.path().join(index.to_string()), value)?;
    }
    let staged: BTreeMap<_, _> = outputs
        .keys()
        .enumerate()
        .map(|(i, name)| (name.clone(), staging.path().join(i.to_string())))
        .collect();
    let mut changed = Vec::new();
    let update = (|| -> Result<()> {
        for (relative, previous) in &before {
            let path = check_path(&root, relative)?;
            ensure!(
                current(&path)? == *previous,
                "Concurrent IDE edit: {relative}"
            );
            if outputs.get(relative) == previous.as_ref() {
                continue;
            }
            if outputs.contains_key(relative) {
                fs::create_dir_all(path.parent().unwrap())?;
                if previous.is_none() {
                    // No-clobber publication even if an unmanaged file appears after preflight.
                    fs::hard_link(&staged[relative], &path)?;
                } else {
                    fs::rename(&staged[relative], &path)?;
                }
            } else {
                fs::remove_file(path)?;
            }
            changed.push(relative.clone());
        }
        let data = serde_json::to_string_pretty(&state)? + "\n";
        if current(&state_path)? != Some(Output::Text(data.clone())) {
            write(&state_path, &Output::Text(data))?;
        }
        Ok(())
    })();
    if let Err(error) = update {
        for relative in changed.into_iter().rev() {
            let path = root.join(&relative);
            if current(&path).ok().flatten().as_ref() == outputs.get(&relative) {
                if let Some(previous) = &before[&relative] {
                    let _ = write(&path, previous);
                } else {
                    let _ = fs::remove_file(path);
                }
            }
        }
        return Err(error);
    }
    Ok(jdks)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paths_and_symlinks_cannot_escape_ownership() {
        let root = tempfile::tempdir().unwrap();
        for p in [
            "../escape",
            ".idea/workspace.xml",
            ".idea/user.xml",
            ".jix/idea-libs/hash/x.jar",
            ".idea/jix-modules/../../escape",
        ] {
            assert!(check_path(root.path(), p).is_err(), "{p}");
        }
        std::os::unix::fs::symlink("/tmp", root.path().join(".idea")).unwrap();
        assert!(check_path(root.path(), ".idea/compiler.xml").is_err());
        assert_eq!(xml("<&\"'>"), "&lt;&amp;&quot;&apos;&gt;");
        let relative = root.path().join("src space & #%");
        assert_eq!(
            url(root.path(), &relative).unwrap(),
            "file://$PROJECT_DIR$/src space & #%"
        );
        assert!(url(root.path(), &root.path().join("$macro")).is_err());
    }
}
