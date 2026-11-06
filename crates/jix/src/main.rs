mod cli;
mod matrix_cli;
mod project;
mod resolution_context;
use anyhow::{Result, ensure};
use cli::{OperationMode, parse};
use jix::{artifacts, build, lock};

fn run() -> Result<()> {
    let mut raw: Vec<_> = std::env::args_os().skip(1).collect();
    let forwarded = if raw.first().is_some_and(|arg| arg == "run") {
        if let Some(index) = raw.iter().position(|arg| arg == "--") {
            let args = raw.split_off(index + 1);
            raw.pop();
            args
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };
    let args: Vec<String> = raw
        .into_iter()
        .map(|arg| {
            arg.into_string()
                .map_err(|_| anyhow::anyhow!("Jix options must be valid UTF-8"))
        })
        .collect::<Result<_>>()?;
    let options = parse(&args).map_err(anyhow::Error::msg)?;
    let environment = options.environment.as_deref();
    if matrix_cli::run(&options.mode, environment)? {
        return Ok(());
    }
    match options.mode {
        OperationMode::Help => println!("{}", cli::HELP),
        OperationMode::MigrationGuide => {
            use std::io::Write;
            std::io::stdout()
                .lock()
                .write_all(jix::migration::guide().as_bytes())?;
        }

        OperationMode::Version => println!("jix {}", env!("CARGO_PKG_VERSION")),
        OperationMode::Init {
            project,
            source,
            template,
        } => project::init(&project, source, &template)?,
        OperationMode::Resolve { .. } | OperationMode::Lock { .. } => {
            unreachable!("document operations routed above")
        }
        OperationMode::Idea { project, open } => project::idea(&project, open, environment)?,
        OperationMode::Run { project, app } => {
            use std::os::unix::process::CommandExt;
            let program = project::run_program(&project, app, environment)?;
            return Err(std::process::Command::new(program)
                .args(forwarded)
                .exec()
                .into());
        }
        OperationMode::ProjectBuild { project, target } => {
            println!(
                "{}",
                project::build(&project, target, environment)?.display()
            );
        }
        OperationMode::Classpath { project, name } => {
            let output = project::build(&project, Some(format!("classpaths.{name}")), environment)?;
            println!("{}", std::fs::read_to_string(output.join("classpath"))?);
        }
        OperationMode::Info {
            project,
            lock: input,
            classpath,
            json,
        } => {
            let input = input.unwrap_or_else(|| project.join("jix.lock"));
            let locked = lock::read(&input)?;
            lock::validate(&locked, &locked.spec)?;
            if let Some(name) = &classpath {
                ensure!(
                    locked.resolution.classpaths.contains_key(name),
                    "Unknown classpath: {name}"
                );
            }
            let classpaths: std::collections::BTreeMap<_, _> = locked
                .resolution
                .classpaths
                .iter()
                .filter(|(name, _)| classpath.as_ref().is_none_or(|selected| selected == *name))
                .collect();
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "lockVersion": locked.version, "resolver": locked.resolver,
                        "repositories": locked.spec.repositories, "classpaths": classpaths,
                    }))?
                );
            } else {
                println!("lock v{} · {}", locked.version, locked.resolver);
                for repo in &locked.spec.repositories {
                    println!("repository: {repo}");
                }
                for (name, cp) in classpaths {
                    println!("{name}: {} artifacts", cp.artifacts.len());
                    for artifact in &cp.artifacts {
                        println!("  {} ({})", artifact.id, artifact.scope);
                    }
                }
                println!("Lock metadata only; use jix verify to check declaration freshness.");
            }
        }
        OperationMode::Export {
            project,
            input,
            output,
        } => {
            let mut export_environment = environment.map(str::to_owned);
            let input = match input {
                Some(input) => {
                    let manifest: artifacts::Manifest =
                        build::read_json(&input.join("artifacts.json"))?;
                    if export_environment.is_none() && manifest.selection.is_some() {
                        export_environment = match std::env::var("JIX_ENVIRONMENT") {
                            Ok(name) => Some(name),
                            Err(std::env::VarError::NotPresent) => None,
                            Err(error) => return Err(error.into()),
                        };
                    }
                    input
                }
                None => project::build(&project, Some("dependencies".into()), environment)?,
            };
            artifacts::materialize_environment(&input, &output, export_environment.as_deref())?;
            eprintln!("exported dependencies → {}", output.display());
        }
        OperationMode::Verify {
            project,
            lock: input,
            dependencies,
            artifacts: map,
            classpath,
        } => {
            let is_project = input.is_none();
            let input = input.unwrap_or_else(|| project.join("jix.lock"));
            let locked = lock::read(&input)?;
            let spec = if let Some(dependencies) = dependencies {
                lock::read_spec(&dependencies)?
            } else if is_project {
                project::offline_spec(&project)?
            } else {
                locked.spec.clone()
            };
            lock::validate(&locked, &spec)?;
            if let Some(map) = map {
                artifacts::verify(&locked, &map, classpath.as_deref())?;
            }
            eprintln!(
                "verified lock{}",
                if is_project {
                    " and dependency declarations"
                } else {
                    ""
                }
            );
        }
        OperationMode::Internal {
            operation,
            config,
            output,
        } => build::run(&operation, &config, &output)?,
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}
