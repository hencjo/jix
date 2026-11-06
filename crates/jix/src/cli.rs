use std::path::PathBuf;

pub const HELP: &str = "jix — Nix-backed Java dependencies\n\nProject commands (default project: current directory):\n  jix init [--project DIR] [--jix-source FLAKE] [--template dependencies|thin|fat|shadow|multi-module|java]\n  jix lock [--project DIR] [--output jix.lock]\n  jix idea [--project DIR] [--open]\n  jix run [APP] [--project DIR] -- [ARGS...]\n  jix build [TARGET] [--project DIR]\n  jix classpath NAME [--project DIR]\n  jix export [REALIZED_DIR] --output DIR [--project DIR]\n  jix info [LOCK] [--project DIR] [--classpath NAME] [--json]\n  jix verify [LOCK] [--project DIR] [--dependencies FILE]\n             [--artifacts FILE] [--classpath NAME]\n\nMatrix build/classpath/export/run/idea: --environment NAME > JIX_ENVIRONMENT > project defaultEnvironment.\nExport of REALIZED_DIR defaults to its validated manifest selection.\ninfo/verify accept --environment to filter; lock always locks the complete matrix.\n\nTargets: dependencies, classpaths.NAME, or a declared project target.\nThe default is dependencies, or application in the Java template.\n\nGuidance (no project required):\n  jix migrate --guide\n\nExplicit integrations:\n  jix lock DEPENDENCIES.json [--output jix.lock]\n  jix resolve DEPENDENCIES.json\n  jix internal classpath|modulepath|compile|jar|fat-jar CONFIG.json --output PATH\n\nlock reconciles Maven declarations, preserving unchanged classpaths.\nBuild/classpath/export use frozen locks; they never update either lockfile.\ninfo reads lock metadata only. verify/internal stay offline.\nUpdate Nix tooling separately with nix flake update.\n  jix --help | --version";

#[derive(Debug, PartialEq, Eq)]
pub enum OperationMode {
    Help,
    Version,
    MigrationGuide,
    Init {
        project: PathBuf,
        source: Option<String>,
        template: String,
    },
    Lock {
        dependencies: Option<PathBuf>,
        project: PathBuf,
        output: Option<PathBuf>,
    },
    Resolve {
        dependencies: PathBuf,
    },
    Idea {
        project: PathBuf,
        open: bool,
    },
    Run {
        project: PathBuf,
        app: Option<String>,
    },
    ProjectBuild {
        project: PathBuf,
        target: Option<String>,
    },
    Classpath {
        project: PathBuf,
        name: String,
    },
    Info {
        project: PathBuf,
        lock: Option<PathBuf>,
        classpath: Option<String>,
        json: bool,
    },
    Export {
        project: PathBuf,
        input: Option<PathBuf>,
        output: PathBuf,
    },
    Verify {
        project: PathBuf,
        lock: Option<PathBuf>,
        dependencies: Option<PathBuf>,
        artifacts: Option<PathBuf>,
        classpath: Option<String>,
    },
    Internal {
        operation: String,
        config: PathBuf,
        output: PathBuf,
    },
}
#[derive(Debug, PartialEq, Eq)]
pub struct CliOptions {
    pub environment: Option<String>,
    pub mode: OperationMode,
}
pub fn parse(args: &[String]) -> Result<CliOptions, String> {
    if args.iter().any(|a| matches!(a.as_str(), "--help" | "-h")) {
        return Ok(CliOptions {
            environment: None,
            mode: OperationMode::Help,
        });
    }
    if args
        .iter()
        .any(|a| matches!(a.as_str(), "--version" | "-V"))
    {
        return Ok(CliOptions {
            environment: None,
            mode: OperationMode::Version,
        });
    }
    let error = || "Unknown argument or invalid invocation. Use --help for usage.".to_owned();
    let (mode, tail) = args.split_first().ok_or_else(error)?;
    if mode == "migrate" {
        return if tail == ["--guide"] {
            Ok(CliOptions {
                environment: None,
                mode: OperationMode::MigrationGuide,
            })
        } else {
            Err("Use 'jix migrate --guide' to print migration instructions; automatic migration is not implemented.".into())
        };
    }

    match mode.as_str() {
        "realize" => return Err("'realize' was replaced by 'jix build dependencies' (or 'jix build classpaths.NAME').".into()),
        "materialize" => return Err("'materialize' was renamed to 'jix export'.".into()),
        "build" if tail.iter().any(|a| a == "--output") || tail.len() > 1 && matches!(tail[0].as_str(), "classpath" | "modulepath" | "compile" | "jar" | "fat-jar") && !tail[1].starts_with('-') =>
            return Err("JSON construction moved to 'jix internal classpath|modulepath|compile|jar|fat-jar CONFIG --output PATH'.".into()),
        _ => {}
    }
    let allowed: &[&str] = match mode.as_str() {
        "init" => &["--project", "--jix-source", "--template"],
        "lock" => &["--project", "--output"],
        "resolve" => &[],
        "build" | "run" => &["--project", "--environment"],
        "idea" => &["--project", "--open", "--environment"],
        "info" => &["--project", "--classpath", "--json", "--environment"],
        "classpath" => &["--project", "--environment"],
        "export" => &["--project", "--output", "--environment"],
        "verify" => &[
            "--project",
            "--dependencies",
            "--artifacts",
            "--classpath",
            "--environment",
        ],
        "internal" => &["--output"],
        _ => return Err(error()),
    };
    let mut flags = std::collections::BTreeMap::new();
    let mut positional = Vec::new();
    let mut iter = tail.iter();
    while let Some(arg) = iter.next() {
        if arg.starts_with('-') {
            if !allowed.contains(&arg.as_str()) {
                return Err(error());
            }
            if matches!(arg.as_str(), "--json" | "--open") {
                if flags.insert(arg.as_str(), String::new()).is_some() {
                    return Err(error());
                }
                continue;
            }
            let value = iter.next().ok_or_else(error)?;
            if value.trim().is_empty()
                || value.starts_with('-')
                || flags.insert(arg.as_str(), value.clone()).is_some()
            {
                return Err(error());
            }
        } else {
            if arg.trim().is_empty() {
                return Err(error());
            }
            positional.push(arg.clone());
        }
    }
    let project = PathBuf::from(flags.get("--project").map(String::as_str).unwrap_or("."));
    let output = flags.get("--output").map(PathBuf::from);
    let mode = match (mode.as_str(), positional.as_slice()) {
        ("init", []) => OperationMode::Init {
            project,
            source: flags.get("--jix-source").cloned(),
            template: {
                let t = flags
                    .get("--template")
                    .map(String::as_str)
                    .unwrap_or("dependencies");
                if !matches!(
                    t,
                    "dependencies" | "java" | "thin" | "fat" | "shadow" | "multi-module"
                ) {
                    return Err(error());
                }
                t.into()
            },
        },
        ("lock", []) => OperationMode::Lock {
            dependencies: None,
            project,
            output,
        },
        ("lock", [input]) if !flags.contains_key("--project") => OperationMode::Lock {
            dependencies: Some(input.into()),
            project,
            output,
        },
        ("resolve", [input]) => OperationMode::Resolve {
            dependencies: input.into(),
        },
        ("idea", []) => OperationMode::Idea {
            project,
            open: flags.contains_key("--open"),
        },
        ("run", inputs) if inputs.len() <= 1 => OperationMode::Run {
            project,
            app: inputs.first().cloned(),
        },
        ("build", inputs) if inputs.len() <= 1 => OperationMode::ProjectBuild {
            project,
            target: inputs.first().cloned(),
        },
        ("classpath", [name]) => OperationMode::Classpath {
            project,
            name: name.clone(),
        },
        ("info", inputs) if inputs.len() <= 1 => OperationMode::Info {
            project,
            lock: inputs.first().map(PathBuf::from),
            classpath: flags.get("--classpath").cloned(),
            json: flags.contains_key("--json"),
        },
        ("export", inputs)
            if inputs.len() <= 1 && !(inputs.len() == 1 && flags.contains_key("--project")) =>
        {
            OperationMode::Export {
                project,
                input: inputs.first().map(PathBuf::from),
                output: output.ok_or_else(error)?,
            }
        }
        ("verify", inputs) if inputs.len() <= 1 => {
            if flags.contains_key("--classpath") && !flags.contains_key("--artifacts") {
                return Err(error());
            }
            OperationMode::Verify {
                project,
                lock: inputs.first().map(PathBuf::from),
                dependencies: flags.get("--dependencies").map(PathBuf::from),
                artifacts: flags.get("--artifacts").map(PathBuf::from),
                classpath: flags.get("--classpath").cloned(),
            }
        }
        ("internal", [operation, config])
            if matches!(
                operation.as_str(),
                "classpath" | "modulepath" | "compile" | "jar" | "fat-jar"
            ) =>
        {
            OperationMode::Internal {
                operation: operation.clone(),
                config: config.into(),
                output: output.ok_or_else(error)?,
            }
        }
        _ => return Err(error()),
    };
    let environment = flags.get("--environment").cloned();
    if environment
        .as_ref()
        .is_some_and(|name| !jix::matrix::valid_name(name))
    {
        return Err("Invalid environment name: use letters, digits, '_' or '-' and no '--'".into());
    }
    Ok(CliOptions { mode, environment })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn p(args: &[&str]) -> Result<CliOptions, String> {
        parse(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }
    #[test]
    fn migration_guide() {
        assert_eq!(
            p(&["migrate", "--guide"]).unwrap().mode,
            OperationMode::MigrationGuide
        );
        for args in [
            vec!["migrate"],
            vec!["migrate", "--guide", "--guide"],
            vec!["migrate", "--guide", "extra"],
            vec!["migrate", "--guide", "--project", "."],
        ] {
            assert!(p(&args).unwrap_err().contains("jix migrate --guide"));
        }
    }
    #[test]
    fn globals() {
        assert_eq!(p(&["bad", "--help"]).unwrap().mode, OperationMode::Help);
        assert_eq!(p(&["--version"]).unwrap().mode, OperationMode::Version);
    }
    #[test]
    fn invalid() {
        for args in [
            vec![],
            vec!["idea", "extra"],
            vec!["idea", "--open", "--open"],
            vec!["idea", "--json"],
            vec!["init", "--template", "unknown"],
            vec!["lock", ""],
            vec!["lock", "a", "--project", "b"],
            vec!["realize", "--classpath"],
            vec!["verify", "--classpath", "main"],
            vec!["run", "one", "two"],
            vec!["run", "--output", "out"],
            vec!["build", "jar", "a"],
            vec!["init", "--project", "a", "--project", "b"],
        ] {
            assert!(p(&args).is_err(), "{args:?}");
        }
    }
    #[test]
    fn modes() {
        for args in [
            vec!["idea"],
            vec!["idea", "--project", "demo", "--open"],
            vec!["init", "--template", "thin"],
            vec!["init", "--template", "fat"],
            vec!["init", "--template", "shadow"],
            vec!["init", "--template", "multi-module"],
            vec!["internal", "fat-jar", "a.json", "--output", "a.jar"],
            vec!["lock"],
            vec!["lock", "deps.json"],
            vec!["build"],
            vec!["run"],
            vec!["run", "application", "--project", "example"],
            vec!["build", "dependencies"],
            vec!["build", "classpaths.main"],
            vec!["info", "--json"],
            vec!["classpath", "main"],
            vec!["verify", "jix.lock", "--artifacts", "artifacts.json"],
            vec!["export", "--output", "deps"],
            vec!["internal", "jar", "a.json", "--output", "a.jar"],
        ] {
            assert!(p(&args).is_ok(), "{args:?}");
        }
    }
}

#[cfg(test)]
mod migration_tests {
    use super::*;
    #[test]
    fn migration_errors() {
        for (args, expected) in [
            (vec!["realize"], "jix build dependencies"),
            (vec!["materialize"], "jix export"),
            (
                vec!["build", "jar", "config.json", "--output", "a.jar"],
                "jix internal",
            ),
        ] {
            assert!(
                parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
                    .unwrap_err()
                    .contains(expected)
            );
        }
        assert!(parse(&["info".into(), "--json".into(), "--json".into()]).is_err());
    }
}
