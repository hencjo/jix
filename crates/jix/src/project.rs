use anyhow::{Context, Result, ensure};
use jix_resolver::DependencySpec;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub fn offline_spec(project: &Path) -> Result<DependencySpec> {
    serde_json::from_str(&backend(project, "verify", None)?)
        .context("Invalid dependency specification from Nix")
}
pub fn run_program(
    project: &Path,
    name: Option<String>,
    environment: Option<&str>,
) -> Result<PathBuf> {
    Ok(backend_environment(project, "run", name, environment)?.into())
}
pub fn idea(project: &Path, open: bool, environment: Option<&str>) -> Result<()> {
    let model = backend_environment(project, "idea", None, environment)?;
    let jdks = jix::idea::generate(project, &model)?;
    eprintln!("Generated IntelliJ project in .idea/; compiler output is in .jix/idea-out/");
    for (name, home) in jdks {
        eprintln!("SDK {name}: {home} (register with Add JDK from disk if missing)");
    }
    if open {
        let status = std::process::Command::new("idea")
            .arg(project.canonicalize()?)
            .status()
            .context(
                "Cannot launch IntelliJ; install an idea launcher or open the project manually",
            )?;
        ensure!(status.success(), "IntelliJ launcher failed: {status}");
    }
    Ok(())
}
pub fn build(project: &Path, name: Option<String>, environment: Option<&str>) -> Result<PathBuf> {
    Ok(backend_environment(project, "build", name, environment)?.into())
}
pub fn spec_value(project: &Path, offline: bool) -> Result<serde_json::Value> {
    serde_json::from_str(&backend(
        project,
        if offline { "verify" } else { "spec" },
        None,
    )?)
    .context("Invalid dependency specification from Nix")
}
fn backend(project: &Path, operation: &str, name: Option<String>) -> Result<String> {
    backend_environment(project, operation, name, None)
}
fn backend_environment(
    project: &Path,
    operation: &str,
    name: Option<String>,
    environment: Option<&str>,
) -> Result<String> {
    #[cfg(feature = "nix")]
    {
        let op = match operation {
            "spec" => jix_nix::Operation::Spec,
            "verify" => jix_nix::Operation::VerifySpec,
            "init" => jix_nix::Operation::Init,
            "init-jdk" => jix_nix::Operation::InitJdk,
            "run" => jix_nix::Operation::Run(name),
            "idea" => jix_nix::Operation::Idea,
            _ => jix_nix::Operation::Build(name),
        };
        jix_nix::project_environment(project, op, environment.map(str::to_owned))
    }
    #[cfg(not(feature = "nix"))]
    {
        let _ = (project, operation, name, environment);
        anyhow::bail!("This offline-only binary has no embedded Nix backend");
    }
}
pub fn init(project: &Path, source: Option<String>, template: &str) -> Result<()> {
    ensure!(
        cfg!(feature = "nix"),
        "This offline-only binary has no project frontend"
    );
    fs::create_dir_all(project)?;
    let project = project.canonicalize()?;
    let source = match source.or_else(|| {
        option_env!("JIX_FLAKE_SOURCE")
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    }) {
        Some(source) => source,
        None => {
            let checkout = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .canonicalize()
                .context("Cannot locate Jix source; pass --jix-source FLAKE")?;
            format!("path:{}", checkout.display())
        }
    };
    ensure!(
        !source.contains(['\n', '\r', '\0']),
        "Invalid Jix flake source"
    );
    let flake_template = match template {
        "dependencies" => include_str!("../../../templates/dependencies/flake.nix"),
        "multi-module" => include_str!("../../../templates/multi-module/flake.nix"),
        _ => include_str!("../../../templates/java/flake.nix"),
    };
    let snippet = flake_template.replace(
        "@JIX_SOURCE@",
        &source
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace("${", "\\${"),
    );
    let flake = project.join("flake.nix");
    let pending = project.join(".jix/init-context-pending");
    if flake.exists() {
        if pending.is_file() && fs::read_to_string(&flake)? == snippet {
            ensure!(
                fs::read_to_string(&pending)? == template,
                "Retry init with the original template"
            );
            backend(&project, "init", None)?;
            finish_context(&project, template)?;
            return Ok(());
        }
        if !project.join("flake.lock").exists() && fs::read_to_string(&flake)? == snippet {
            backend(&project, "init", None)?;
            eprintln!("Pinned Nix inputs for the existing Jix template.");
            return Ok(());
        }
        eprintln!(
            "Existing flake.nix left untouched. Integrate this dependency module:\n{snippet}"
        );
        return Ok(());
    }
    let mut files = vec![
        (
            "maven.nix",
            include_str!("../../../templates/dependencies/maven.nix"),
        ),
        ("flake.nix", snippet.as_str()),
    ];
    if matches!(template, "java" | "thin" | "fat" | "shadow") {
        let build = match template {
            "fat" => include_str!("../../../templates/fat/build.nix"),
            "shadow" => include_str!("../../../templates/shadow/build.nix"),
            _ => include_str!("../../../templates/java/build.nix"),
        };
        files.extend([
            ("build.nix", build),
            (
                "src/main/java/example/Main.java",
                include_str!("../../../templates/java/src/main/java/example/Main.java"),
            ),
            (
                "src/main/resources/message.txt",
                include_str!("../../../templates/java/src/main/resources/message.txt"),
            ),
        ]);
    }
    if template == "multi-module" {
        files.extend([
            (
                "build.nix",
                include_str!("../../../templates/multi-module/build.nix"),
            ),
            (
                "core/src/main/java/example/core/Greeting.java",
                include_str!(
                    "../../../templates/multi-module/core/src/main/java/example/core/Greeting.java"
                ),
            ),
            (
                "app/src/main/java/example/Main.java",
                include_str!("../../../templates/multi-module/app/src/main/java/example/Main.java"),
            ),
        ]);
    }
    ensure!(
        fs::symlink_metadata(project.join("resolution-context.nix")).is_err(),
        "resolution-context.nix already exists; refusing to overwrite"
    );
    for (name, _) in &files {
        ensure!(
            fs::symlink_metadata(project.join(name)).is_err(),
            "{name} already exists; refusing to overwrite"
        );
    }
    for (name, content) in files {
        let path = project.join(name);
        fs::create_dir_all(path.parent().unwrap())?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        file.write_all(content.as_bytes())?;
    }
    let ignore = project.join(".gitignore");
    let old = fs::read_to_string(&ignore).unwrap_or_default();
    let missing = ["/.jix/", "/.idea/"]
        .into_iter()
        .filter(|pattern| !old.lines().any(|line| line == *pattern))
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        let mut file = fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(ignore)?;
        if !old.is_empty() && !old.ends_with('\n') {
            writeln!(file)?;
        }
        for pattern in missing {
            writeln!(file, "{pattern}")?;
        }
    }
    fs::create_dir_all(project.join(".jix"))?;
    fs::write(&pending, template)?;
    backend(&project,"init",None).context("Project files created, but pinning Nix inputs failed; fix the error and retry init with the same template")?;
    finish_context(&project, template)?;
    eprintln!("Initialized maven.nix and pinned Nix inputs. Edit dependencies, then run jix lock.");
    Ok(())
}

fn finish_context(project: &Path, template: &str) -> Result<()> {
    let context = project.join("resolution-context.nix");
    if fs::symlink_metadata(&context).is_err() {
        let java = if template == "dependencies" {
            PathBuf::from("java")
        } else {
            PathBuf::from(backend(project, "init-jdk", None)?).join("bin/java")
        };
        let content = crate::resolution_context::capture(&java).context(
            "Cannot capture Maven context; fix Java and retry init with the same template",
        )?;
        let mut file = tempfile::NamedTempFile::new_in(project)?;
        file.write_all(content.as_bytes())?;
        file.persist_noclobber(&context)?;
        eprintln!(
            "Captured resolution-context.nix; review and commit it. It is never refreshed automatically."
        );
    }
    fs::remove_file(project.join(".jix/init-context-pending"))?;
    Ok(())
}
