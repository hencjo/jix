// Initialization only: ordinary resolution must never probe Java or the host.
use anyhow::{Context, Result, ensure};
use std::{collections::BTreeMap, path::Path, process::Command};

const KEYS: [&str; 6] = [
    "java.version",
    "java.home",
    "os.name",
    "os.arch",
    "os.version",
    "path.separator",
];
const OPTION_ENV: [&str; 5] = [
    "JAVA_TOOL_OPTIONS",
    "JDK_JAVA_OPTIONS",
    "_JAVA_OPTIONS",
    "JAVA_OPTS",
    "CLASSPATH",
];

fn probe(java: &Path) -> Command {
    let mut command = Command::new(java);
    command.args(["-XshowSettings:properties", "-version"]);
    for key in OPTION_ENV {
        command.env_remove(key);
    }
    command
}

pub fn capture(java: &Path) -> Result<String> {
    let output = probe(java)
        .output()
        .context("Cannot execute Java; install Java on PATH for dependency-only init")?;
    ensure!(
        output.status.success(),
        "Java property probe failed: {}",
        output.status
    );
    render(std::str::from_utf8(&output.stderr).context("Java properties are not UTF-8")?)
}

fn quote(value: &str) -> Result<String> {
    ensure!(
        !value.chars().any(char::is_control),
        "Control character in Java property"
    );
    Ok(serde_json::to_string(value)?.replace("${", "\\${"))
}

fn render(output: &str) -> Result<String> {
    let mut properties = BTreeMap::new();
    for line in output.lines() {
        let Some((key, value)) = line.trim().split_once(" = ") else {
            continue;
        };
        if KEYS.contains(&key) {
            ensure!(!value.is_empty(), "Empty Java property: {key}");
            ensure!(
                properties.insert(key, value).is_none(),
                "Duplicate Java property: {key}"
            );
        }
    }
    let mut text = String::from(
        "# Captured by jix init. Review and commit; never refreshed automatically.\n{\n  systemProperties = {\n",
    );
    for key in KEYS {
        let value = properties
            .get(key)
            .with_context(|| format!("Missing Java property: {key}"))?;
        text.push_str(&format!("    {} = {};\n", quote(key)?, quote(value)?));
    }
    text.push_str("  };\n  userProperties = { };\n}\n");
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn properties() -> String {
        KEYS.iter()
            .map(|key| format!("    {key} = value\n"))
            .collect()
    }

    #[test]
    fn parses_only_required_properties() {
        let text = render(&(properties() + "    user.name = private\n")).unwrap();
        assert!(!text.contains("private"));
        assert!(text.contains("userProperties = { };"));
        assert!(render("java.version = 21").is_err());
        assert!(render(&(properties() + "java.version = duplicate\n")).is_err());
    }

    #[test]
    fn escapes_nix_strings_and_sanitizes_environment() {
        assert_eq!(quote("${value}").unwrap(), "\"\\${value}\"");
        assert_eq!(quote("a\\b\"").unwrap(), "\"a\\\\b\\\"\"");
        assert!(quote("bad\nvalue").is_err());
        let command = probe(Path::new("java"));
        for key in OPTION_ENV {
            assert!(
                command
                    .get_envs()
                    .any(|(name, value)| name == key && value.is_none())
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn failed_probe_is_not_a_context() {
        assert!(capture(Path::new("/bin/false")).is_err());
        assert!(capture(Path::new("/no/such/java")).is_err());
    }
}
