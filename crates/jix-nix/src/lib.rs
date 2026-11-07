//! Upstream Nix on one registered evaluator thread. No subprocess backend.
use anyhow::{Context, Result, bail};
use nix_bindings_expr::eval_state::{self, EvalStateBuilder};
use nix_bindings_fetchers::FetchersSettings;
use nix_bindings_flake::{
    EvalStateBuilderExt, FlakeLockFlags, FlakeReference, FlakeReferenceParseFlags, FlakeSettings,
    LockedFlake,
};
use nix_bindings_store::store::Store;
use std::{
    ffi::{CStr, CString},
    path::{Path, PathBuf},
    sync::Mutex,
};

static SERIAL: Mutex<bool> = Mutex::new(false);
unsafe extern "C" {
    fn jix_configure_eval(builder: *mut std::ffi::c_void);
    fn jix_offline() -> *mut std::ffi::c_char;
    fn jix_root(
        target: *const std::ffi::c_char,
        root: *const std::ffi::c_char,
    ) -> *mut std::ffi::c_char;
    fn jix_root_error_free(error: *mut std::ffi::c_char);
}

#[derive(Clone)]
pub enum Operation {
    Init,
    InitJdk,
    Spec,
    VerifySpec,
    Build(Option<String>),
    Run(Option<String>),
    Idea,
}

pub fn project(path: &Path, operation: Operation) -> Result<String> {
    project_environment(path, operation, None)
}

pub fn project_environment(
    path: &Path,
    operation: Operation,
    environment: Option<String>,
) -> Result<String> {
    let path = path.canonicalize()?;
    // Nixpkgs evaluation (notably launchers) outgrows Rust's default 2 MiB.
    // Set this explicitly rather than requiring a host RUST_MIN_STACK override.
    std::thread::Builder::new()
        .name("jix-nix-evaluator".into())
        .stack_size(64 * 1024 * 1024)
        .spawn(move || execute(path, operation, environment))
        .context("Starting Nix evaluator thread")?
        .join()
        .map_err(|_| anyhow::anyhow!("Nix evaluator thread failed"))?
}

fn execute(path: PathBuf, operation: Operation, environment: Option<String>) -> Result<String> {
    let mut serial = SERIAL
        .lock()
        .map_err(|_| anyhow::anyhow!("Nix evaluator poisoned"))?;
    let offline = matches!(operation, Operation::VerifySpec);
    if offline {
        if *serial {
            bail!("Offline evaluation requires a fresh Nix backend process");
        }
        // SAFETY: adapter has no inputs and returns an owned error or null.
        unsafe {
            let error = jix_offline();
            if !error.is_null() {
                let message = CStr::from_ptr(error).to_string_lossy().into_owned();
                jix_root_error_free(error);
                bail!("Cannot enforce offline evaluation: {message}");
            }
        }
    }
    *serial = true;
    eval_state::init()?;
    let _registration = eval_state::gc_register_my_thread()?;
    nix_bindings_util::settings::set("experimental-features", "nix-command flakes")?;
    if offline {
        nix_bindings_util::settings::set("substitute", "false").context("Setting substitute")?;
        nix_bindings_util::settings::set("download-attempts", "1")
            .context("Setting download-attempts")?;
    }
    let mut store = Store::open(None, [])?;
    let fetchers = FetchersSettings::new()?;
    let settings = FlakeSettings::new()?;
    let builder = EvalStateBuilder::new(store.clone())?
        .flakes(&settings)?
        .load_ambient_settings(false);
    // SAFETY: builder owns the pinned native evaluator builder until build().
    unsafe {
        jix_configure_eval(builder.raw_ptr().cast());
    }
    let mut eval = builder.build()?;
    let mut flags = FlakeLockFlags::new(&settings)?;
    if matches!(operation, Operation::Init) {
        flags.set_mode_write_as_needed()?;
    } else {
        if !path.join("flake.lock").is_file() {
            bail!("Missing flake.lock; initialize and pin the project first");
        }
        flags.set_mode_check()?;
    }
    let source = if matches!(operation, Operation::Init) {
        path.to_string_lossy().into_owned()
    } else {
        let filter = eval.eval_from_string(r#"root: builtins.toString (builtins.path {
          name = "source";
          path = builtins.toPath root;
          filter = p: type: !(builtins.elem (toString p)
            (map (name: root + "/" + name) [ ".git" ".idea" ".jix" ".devenv" ".direnv" "target" "result" ]));
        })"#, "<jix-project-source>")?;
        let root = eval.new_value_str(path.to_str().context("Non-UTF8 project path")?)?;
        let source = eval.call(filter, root)?;
        eval.require_string(&source)?
    };
    let (reference, _) = FlakeReference::parse_with_fragment(
        &fetchers,
        &settings,
        &FlakeReferenceParseFlags::new(&settings)?,
        &format!("path:{source}"),
    )?;
    let locked = LockedFlake::lock(&fetchers, &settings, &eval, &flags, &reference)?;
    if matches!(operation, Operation::Init) {
        // Local immutable bootstrap inputs cannot be re-downloaded from a URL.
        // Retain them even before the first dependency realization.
        let lock: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path.join("flake.lock"))?)?;
        let nodes = lock["nodes"]
            .as_object()
            .context("Invalid flake lock nodes")?;
        for (index, node) in nodes.values().enumerate() {
            if let Some(input) = node["locked"]["path"].as_str()
                && store.parse_store_path(input).is_ok()
            {
                retain(input, &path.join(format!(".jix/roots/input-{index:05}")))?;
            }
        }
        return Ok(String::new());
    }
    let outputs = locked.outputs(&settings, &mut eval)?;
    let system = eval.eval_from_string("builtins.currentSystem", "<jix-system>")?;
    let system = eval.require_string(&system)?;
    if matches!(operation, Operation::InitJdk) {
        let mut value = outputs;
        for key in ["jix", &system, "initJdk"] {
            value = eval
                .require_attrs_select(&value, key)
                .context("Missing initialization JDK output")?;
        }
        let drv = eval.require_attrs_select(&value, "drvPath")?;
        let drv = eval.require_string(&drv)?;
        let drv = store.parse_store_path(&drv)?;
        let realized = store.realise(&drv)?;
        let output = store.real_path(realized.get("out").context("JDK has no out output")?)?;
        retain(&output, &path.join(".jix/roots/init-jdk"))?;
        return Ok(output);
    }
    let mut descriptor = outputs;
    for key in ["jix", &system, "dependencies"] {
        descriptor = eval
            .require_attrs_select(&descriptor, key)
            .with_context(|| {
                format!("Missing project output jix.{system}.dependencies; see jix init")
            })?;
    }
    let version = eval.require_attrs_select(&descriptor, "version")?;
    let mut version = eval.require_int(&version)?;
    if !matches!(version, 1..=3) {
        bail!("Unsupported Jix project descriptor version");
    }
    let mut selected_environment = None;
    if version == 3 && !matches!(operation, Operation::Spec | Operation::VerifySpec) {
        let names_fn = eval.eval_from_string(
            "d: builtins.toJSON (builtins.attrNames d.environments)",
            "<jix-environments>",
        )?;
        let names = eval.call(names_fn, descriptor.clone())?;
        let names: Vec<String> = serde_json::from_str(&eval.require_string(&names)?)?;
        let override_name = match environment.clone() {
            Some(name) => Some(name),
            None => match std::env::var("JIX_ENVIRONMENT") {
                Ok(name) => Some(name),
                Err(std::env::VarError::NotPresent) => None,
                Err(error) => return Err(error).context("Invalid JIX_ENVIRONMENT"),
            },
        };
        let selected = match override_name {
            Some(name) => Some(name),
            None => {
                let default_fn = eval.eval_from_string(
                    "d: builtins.toJSON (d.defaultEnvironment or null)",
                    "<jix-default-environment>",
                )?;
                let value = eval.call(default_fn, descriptor.clone())?;
                serde_json::from_str::<Option<String>>(&eval.require_string(&value)?).with_context(
                    || {
                        format!(
                            "defaultEnvironment must be a name or null; available: {}",
                            names.join(", ")
                        )
                    },
                )?
            }
        };
        let name = selected.with_context(|| format!(
            "Select an environment with --environment NAME, JIX_ENVIRONMENT or project defaultEnvironment; available: {}",
            names.join(", ")
        ))?;
        if !names.contains(&name)
            || name.is_empty()
            || name.contains("--")
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            bail!(
                "Invalid or unknown environment: {name:?}; available: {}",
                names.join(", ")
            );
        }
        let environments = eval.require_attrs_select(&descriptor, "environments")?;
        descriptor = eval.require_attrs_select(&environments, &name)?;
        selected_environment = Some(name);
        let selected_version = eval.require_attrs_select(&descriptor, "version")?;
        version = eval.require_int(&selected_version)?;
        if version != 2 {
            bail!("Unsupported environment descriptor version");
        }
    } else if environment.is_some() {
        bail!("--environment requires a matrix project operation");
    }
    let root_prefix = selected_environment
        .as_ref()
        .map(|name| format!("{name}--"))
        .unwrap_or_default();
    match operation {
        Operation::Spec | Operation::VerifySpec => {
            let spec = eval.require_attrs_select(&descriptor, "spec")?;
            let to_json = eval.eval_from_string("builtins.toJSON", "<jix-json>")?;
            let json = eval.call(to_json, spec)?;
            eval.require_string(&json)
        }
        Operation::Idea => {
            let valid = eval.require_attrs_select(&descriptor, "lockValid")?;
            if !eval.require_bool(&valid)? {
                bail!("Invalid lock; run jix lock");
            }
            let model = eval
                .require_attrs_select(&descriptor, "ide")
                .context("Project has no IDE model; use the current jix.project helper")?;
            let root = eval.new_value_str(&source)?;
            let model = eval.call(model, root)?;
            let to_json = eval.eval_from_string("builtins.toJSON", "<jix-idea-json>")?;
            let json = eval.call(to_json, model)?;
            let realised = eval.realise_string(&json, false)?;
            for input in &realised.paths {
                let target = store.real_path(input)?;
                let name = Path::new(&target)
                    .file_name()
                    .context("Invalid IDE store path")?;
                // Append content-addressed roots: a failed IDE publication must not
                // unroot dependencies still referenced by the previous project files.
                retain(
                    &target,
                    &path
                        .join(".jix/roots")
                        .join(format!("idea-{}", name.to_string_lossy())),
                )?;
            }
            Ok(realised.s)
        }
        Operation::Run(app) => {
            let name = app.unwrap_or_else(|| "default".into());
            if matches!(name.as_str(), "" | "." | "..")
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            {
                bail!("Invalid app name: {name}");
            }
            let valid = eval
                .require_attrs_select(&descriptor, "lockValid")
                .context("Project does not support apps; adopt jix.project")?;
            if !eval.require_bool(&valid)? {
                bail!("Invalid lock; run jix lock");
            }
            let apps = eval
                .require_attrs_select(&descriptor, "apps")
                .context("Project declares no apps; adopt jix.project")?;
            let app = eval
                .require_attrs_select(&apps, &name)
                .with_context(|| format!("Unknown app: {name}"))?;
            let kind = eval.require_attrs_select(&app, "type")?;
            if eval.require_string(&kind)? != "app" {
                bail!("Invalid app type: {name}");
            }
            let program = eval.require_attrs_select(&app, "program")?;
            let realised = eval.realise_string(&program, false)?;
            if !realised.s.starts_with("/nix/store/") || realised.paths.is_empty() {
                bail!("App program must reference a Nix store executable");
            }
            for (index, input) in realised.paths.iter().enumerate() {
                retain(
                    &store.real_path(input)?,
                    &path
                        .join(".jix/roots")
                        .join(format!("{root_prefix}app-{name}-{index}")),
                )?;
            }
            Ok(realised.s)
        }
        Operation::Build(target) => {
            let name = match target {
                Some(target) => target,
                None if version == 1 => "dependencies".into(),
                None => {
                    let default = eval.require_attrs_select(&descriptor, "defaultTarget")?;
                    eval.require_string(&default)?
                }
            };
            if matches!(name.as_str(), "" | "." | "..")
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            {
                bail!("Invalid build target: {name}");
            }
            let value = if version == 2 {
                let valid = eval.require_attrs_select(&descriptor, "lockValid")?;
                if !eval.require_bool(&valid)? {
                    bail!("Invalid lock; run jix lock");
                }
                let targets = eval.require_attrs_select(&descriptor, "targets")?;
                eval.require_attrs_select(&targets, &name)
                    .with_context(|| format!("Unknown build target: {name}"))?
            } else if name == "dependencies" {
                eval.require_attrs_select(&descriptor, "all")?
            } else if let Some(cp) = name.strip_prefix("classpaths.") {
                let cps = eval.require_attrs_select(&descriptor, "classpaths")?;
                eval.require_attrs_select(&cps, cp)
                    .with_context(|| format!("Unknown classpath: {cp}"))?
            } else {
                bail!(
                    "Unknown build target: {name} (legacy project supports dependencies and classpaths.NAME)"
                );
            };
            let root_name = if name == "dependencies" {
                "all".into()
            } else if let Some(cp) = name.strip_prefix("classpaths.") {
                format!("classpath-{cp}")
            } else {
                format!("target-{name}")
            };
            let drv = eval.require_attrs_select(&value, "drvPath")?;
            let drv = eval.require_string(&drv)?;
            let drv = store.parse_store_path(&drv)?;
            let realized = store.realise(&drv)?;
            let output = realized
                .get("out")
                .context("Nix derivation has no out output")?;
            let output = store.real_path(output)?;
            retain(
                &output,
                &path
                    .join(".jix/roots")
                    .join(format!("{root_prefix}{root_name}")),
            )?;
            Ok(output)
        }
        Operation::Init | Operation::InitJdk => unreachable!(),
    }
}

fn retain(target: &str, root: &Path) -> Result<()> {
    std::fs::create_dir_all(root.parent().context("Missing root parent")?)?;
    let root = CString::new(root.to_str().context("Non-UTF8 project path")?)?;
    let target = CString::new(target)?;
    // SAFETY: live NUL-terminated strings; adapter returns an owned error or null.
    unsafe {
        let error = jix_root(target.as_ptr(), root.as_ptr());
        if !error.is_null() {
            let message = CStr::from_ptr(error).to_string_lossy().into_owned();
            jix_root_error_free(error);
            bail!("{message}");
        }
    }
    Ok(())
}
