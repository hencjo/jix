//! Document routing: legacy commands keep their wire format; matrix commands never infer an environment.
use crate::{cli::OperationMode, project};
use anyhow::{Context, Result, ensure};
use jix::{
    artifacts, build, lock,
    matrix::{self, MatrixLock},
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn read(path: &Path) -> Result<Value> {
    build::read_json(path)
}

pub fn run(mode: &OperationMode, environment: Option<&str>) -> Result<bool> {
    match mode {
        OperationMode::Resolve { dependencies } => {
            let value = read(dependencies)?;
            let result = if value.get("environments").is_some() {
                serde_json::to_value(matrix::resolve(&matrix::normalize(value)?)?)?
            } else {
                let spec = lock::read_spec(dependencies)?;
                serde_json::to_value(jix_resolver::resolve(
                    &spec,
                    &mut jix::repository::Repository::new(&spec.repositories)?,
                )?)?
            };
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(true)
        }
        OperationMode::Lock {
            dependencies,
            project,
            output,
        } => {
            let value = if let Some(input) = dependencies {
                read(input)?
            } else {
                project::spec_value(project, false)?
            };
            let output = output.clone().unwrap_or_else(|| {
                if dependencies.is_some() {
                    PathBuf::from("jix.lock")
                } else {
                    project.join("jix.lock")
                }
            });
            if let Some(input) = dependencies {
                ensure!(
                    !input
                        .canonicalize()
                        .ok()
                        .zip(output.canonicalize().ok())
                        .is_some_and(|(a, b)| a == b),
                    "Refusing to overwrite dependencies with the lockfile"
                );
            }
            let old = if output.exists() {
                Some(jix::lock_codec::read(&output)?)
            } else {
                None
            };
            let result = if value.get("environments").is_some() {
                let spec = matrix::normalize(value)?;
                let previous: Option<MatrixLock> = match &old {
                    Some(v) if v["request"].get("environments").is_some() => {
                        Some(serde_json::from_value(v.clone())?)
                    }
                    Some(v) => {
                        let old: lock::Lockfile = serde_json::from_value(v.clone())?;
                        lock::validate(&old, &old.spec)?;
                        eprintln!("Switching to matrix lock: resolving all environments");
                        None
                    }
                    None => None,
                };
                serde_json::to_value(matrix::reconcile(&spec, previous.as_ref())?)?
            } else {
                let spec = if let Some(input) = dependencies {
                    lock::read_spec(input)?
                } else {
                    serde_json::from_value(value)?
                };
                let previous = old
                    .clone()
                    .map(serde_json::from_value::<lock::Lockfile>)
                    .transpose()?;
                serde_json::to_value(lock::reconcile(&spec, previous.as_ref())?)?
            };
            if old.as_ref() == Some(&result) {
                eprintln!("lockfile is current; no repository access or lockfile changes");
            } else {
                report_changes(old.as_ref(), &result);
                lock::write(&result, &output)?;
                eprintln!(
                    "locked {} artifacts → {}",
                    result["artifacts"]
                        .as_object()
                        .context("Missing artifact map")?
                        .len(),
                    output.display()
                );
            }
            Ok(true)
        }
        OperationMode::Info {
            project,
            lock: input,
            classpath,
            json: as_json,
        } => {
            let input = input.clone().unwrap_or_else(|| project.join("jix.lock"));
            let value = jix::lock_codec::read(&input)?;
            if value["request"].get("environments").is_none() {
                ensure!(
                    environment.is_none(),
                    "--environment requires a matrix lock"
                );
                return Ok(false);
            }
            let locked: MatrixLock = serde_json::from_value(value)?;
            locked.validate(&locked.spec)?;
            if let Some(name) = environment {
                locked.select(name)?;
            }
            let mut environments = serde_json::Map::new();
            for (name, result) in &locked.resolution.environments {
                if environment.is_some_and(|selected| selected != name) {
                    continue;
                }
                let cps: std::collections::BTreeMap<_, _> = result
                    .classpaths
                    .iter()
                    .filter(|(name, _)| classpath.as_ref().is_none_or(|cp| cp == *name))
                    .collect();
                if classpath.is_some() && cps.is_empty() {
                    continue;
                }
                environments.insert(
                    name.clone(),
                    json!({"contextHash":locked.selection(name)?.context_hash,"classpaths":cps}),
                );
            }
            ensure!(
                !environments.is_empty(),
                "Unknown classpath: {}",
                classpath.as_deref().unwrap_or("")
            );
            if *as_json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &json!({"lockVersion":locked.version,"resolver":locked.resolver,"repositories":locked.spec.repositories,"environments":environments})
                    )?
                );
            } else {
                println!("lock v{} · {}", locked.version, locked.resolver);
                for (name, result) in environments {
                    println!("environment: {name}");
                    for (cp, result) in result["classpaths"].as_object().unwrap() {
                        println!("  {cp}:");
                        for a in result["artifacts"].as_array().unwrap() {
                            println!(
                                "    {} ({})",
                                a["id"].as_str().unwrap(),
                                a["scope"].as_str().unwrap()
                            );
                        }
                    }
                }
                println!("Lock metadata only; use jix verify to check declaration freshness.");
            }
            Ok(true)
        }
        OperationMode::Verify {
            project,
            lock: input,
            dependencies,
            artifacts: map,
            classpath,
        } => {
            let is_project = input.is_none();
            let input = input.clone().unwrap_or_else(|| project.join("jix.lock"));
            let value = jix::lock_codec::read(&input)?;
            if value["request"].get("environments").is_none() {
                ensure!(
                    environment.is_none(),
                    "--environment requires a matrix lock"
                );
                return Ok(false);
            }
            let locked: MatrixLock = serde_json::from_value(value)?;
            let spec = if let Some(input) = dependencies {
                matrix::normalize(read(input)?)?
            } else if is_project {
                matrix::normalize(project::spec_value(project, true)?)?
            } else {
                locked.spec.clone()
            };
            locked.validate(&spec)?;
            if let Some(name) = environment {
                locked.select(name)?;
            }
            if let Some(map) = map {
                let name = environment
                    .context("Matrix artifact verification requires --environment NAME")?;
                artifacts::verify_environment(&locked, map, name, classpath.as_deref())?;
            }
            eprintln!(
                "verified matrix lock{}",
                if is_project {
                    " and dependency declarations"
                } else {
                    ""
                }
            );
            Ok(true)
        }
        _ => Ok(false),
    }
}
fn report_changes(old: Option<&Value>, new: &Value) {
    fn selections(
        value: &Value,
    ) -> std::collections::BTreeMap<String, std::collections::BTreeSet<String>> {
        let mut result = std::collections::BTreeMap::new();
        let mut add = |prefix: &str, cps: &Value| {
            if let Some(cps) = cps.as_object() {
                for (name, cp) in cps {
                    result.insert(
                        format!("{prefix}{name}"),
                        cp["artifacts"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|a| a["id"].as_str().map(str::to_owned))
                            .collect(),
                    );
                }
            }
        };
        if let Some(envs) = value["resolution"]["environments"].as_object() {
            for (name, env) in envs {
                add(&format!("{name}/"), &env["classpaths"]);
            }
        } else {
            add("", &value["resolution"]["classpaths"]);
        }
        result
    }
    let old = old.map(selections).unwrap_or_default();
    let new = selections(new);
    for (name, after) in &new {
        let before = old.get(name).cloned().unwrap_or_default();
        for id in before.difference(after) {
            eprintln!("{name}: - {id}");
        }
        for id in after.difference(&before) {
            eprintln!("{name}: + {id}");
        }
    }
    for name in old.keys().filter(|name| !new.contains_key(*name)) {
        eprintln!("{name}: removed classpath");
    }
}
