//! Maven 3.9.11 published-POM activation. No environment or filesystem I/O.
use crate::{Error, ResolutionContext, Result};
use roxmltree::Node;
use std::collections::BTreeMap;

fn child<'a, 'i>(n: Node<'a, 'i>, tag: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|n| n.has_tag_name(tag))
}
fn text(n: Node<'_, '_>, tag: &str) -> String {
    child(n, tag)
        .and_then(|n| n.text())
        .unwrap_or("")
        .trim()
        .into()
}
fn expand(
    value: &str,
    props: &BTreeMap<String, String>,
    project: &BTreeMap<String, String>,
    depth: usize,
) -> Result<String> {
    if depth >= 64 {
        return Err(Error::unsupported(
            "Recursive profile activation interpolation",
        ));
    }
    let mut output = String::new();
    let mut tail = value;
    while let Some(start) = tail.find("${") {
        output.push_str(&tail[..start]);
        let Some(end) = tail[start + 2..].find('}') else {
            output.push_str(&tail[start..]);
            return Ok(output);
        };
        let end = start + 2 + end;
        let key = &tail[start + 2..end];
        if key == "packaging" {
            return Err(Error::unsupported(
                "Maven synthetic packaging property is not supported",
            ));
        }
        if key.contains("${") {
            return Err(Error::unsupported("Nested activation property names"));
        }
        if let Some(replacement) = project.get(key).or_else(|| props.get(key)) {
            output.push_str(&expand(replacement, props, project, depth + 1)?);
        } else {
            output.push_str(&tail[start..=end]);
        }
        if output.len() > 4 * 1024 * 1024 {
            return Err(Error::unsupported(
                "Profile activation expansion exceeds 4 MiB",
            ));
        }
        tail = &tail[end + 1..];
    }
    output.push_str(tail);
    if output.len() > 4 * 1024 * 1024 {
        return Err(Error::unsupported(
            "Profile activation expansion exceeds 4 MiB",
        ));
    }
    Ok(output)
}
fn required<'a>(properties: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str> {
    properties
        .get(key)
        .filter(|v| !v.is_empty())
        .map(String::as_str)
        .ok_or_else(|| {
            Error::unsupported(format!(
                "Declare resolutionContext.systemProperties.\"{key}\""
            ))
        })
}
fn negated(value: &str, test: impl FnOnce(&str) -> Result<bool>) -> Result<bool> {
    match value.strip_prefix('!') {
        Some(v) => Ok(!test(v)?),
        None => test(value),
    }
}
fn family(family: &str, os: &str, props: &BTreeMap<String, String>) -> Result<bool> {
    let windows = os.contains("windows");
    let win9x = ["95", "98", "me", "ce"].iter().any(|s| os.contains(s));
    let mac = os.contains("mac") || os.contains("darwin");
    Ok(match family {
        "windows" => windows,
        "win9x" => windows && win9x,
        "winnt" => windows && !win9x,
        "mac" => mac,
        "tandem" => os.contains("nonstop_kernel"),
        "z/os" => os.contains("z/os") || os.contains("os/390"),
        "dos" => required(props, "path.separator")? == ";" && !os.contains("netware") && !windows,
        "unix" => {
            required(props, "path.separator")? == ":"
                && !os.contains("openvms")
                && (!mac || os.ends_with('x'))
        }
        other => os.contains(other),
    })
}
// Maven's JDK activator compares only three integer tokens, not ComparableVersion.
fn relation(version: &str, bound: &str, inclusive: bool, lower: bool) -> Option<i32> {
    if bound.is_empty() {
        return Some(if lower { 1 } else { -1 });
    }
    let cleaned: String = version
        .chars()
        .filter(|c| c.is_ascii_digit() || "._-".contains(*c))
        .collect();
    let mut version: Vec<_> = cleaned.split(['.', '_', '-']).collect();
    let mut bound: Vec<_> = bound.split('.').collect();
    while version.last() == Some(&"") {
        version.pop();
    }
    while bound.last() == Some(&"") {
        bound.pop();
    }
    for i in 0..3 {
        let a: i32 = version.get(i).copied().unwrap_or("0").parse().ok()?;
        let b: i32 = bound.get(i).copied().unwrap_or("0").parse().ok()?;
        if a != b {
            return Some(if a < b { -1 } else { 1 });
        }
    }
    Some(if inclusive {
        0
    } else if lower {
        -1
    } else {
        1
    })
}
fn jdk(rule: &str, version: &str) -> Result<bool> {
    if let Some(prefix) = rule.strip_prefix('!') {
        return Ok(!version.starts_with(prefix));
    }
    if !rule.starts_with(['[', '(']) {
        return Ok(version.starts_with(rule));
    }
    // Match Maven's range tokenization, including its single-bound behavior.
    let mut bounds = Vec::new();
    for token in rule.split(',') {
        if token.starts_with('[') {
            bounds.push((token.replace('[', ""), true));
        } else if token.starts_with('(') {
            bounds.push((token.replace('(', ""), false));
        } else if token.ends_with(']') {
            bounds.push((token.replace(']', ""), true));
        } else if token.ends_with(')') {
            bounds.push((token.replace(')', ""), false));
        } else if token.is_empty() {
            bounds.push((String::new(), false));
        }
    }
    if bounds.len() < 2 {
        bounds.push(("99999999".into(), false));
    }
    let Some((lo, li)) = bounds.first() else {
        return Err(Error::unsupported("Malformed JDK activation range"));
    };
    let Some(lower) = relation(version, lo, *li, true) else {
        return Ok(false);
    };
    if lower == 0 {
        return Ok(true);
    }
    if lower < 0 {
        return Ok(false);
    }
    let (hi, ui) = &bounds[1];
    Ok(relation(version, hi, *ui, false).is_some_and(|n| n <= 0))
}
fn file_active(
    file: Node<'_, '_>,
    props: &BTreeMap<String, String>,
    project: &BTreeMap<String, String>,
) -> Result<bool> {
    let exists = text(file, "exists");
    let mut path = if exists.is_empty() {
        text(file, "missing")
    } else {
        exists
    };
    if path.is_empty() || path.contains("${basedir}") {
        return Ok(false);
    }
    // Published descriptors have no project directory. Project properties precede session properties here.
    for _ in 0..64 {
        let Some(start) = path.find("${") else {
            break;
        };
        let Some(end) = path[start + 2..].find('}') else {
            return Err(Error::invalid("Unclosed file activation expression"));
        };
        let end = start + 2 + end;
        let key = &path[start + 2..end];
        if key == "packaging" {
            return Err(Error::unsupported(
                "Maven synthetic packaging property is not supported",
            ));
        }
        let Some(value) = project.get(key).or_else(|| props.get(key)) else {
            // Unresolved relative paths remain relative and Maven considers them inactive.
            if !path.starts_with('/') && !path.contains(":\\") {
                return Ok(false);
            }
            return Err(Error::unsupported(format!(
                "File activation requires filesystem lookup: {path}"
            )));
        };
        let next = path.replacen(&path[start..=end], value, 1);
        if next == path {
            return Err(Error::invalid("Recursive file activation property"));
        }
        path = next;
    }
    if path.contains("${") {
        return Err(Error::unsupported(
            "Unresolved file activation interpolation",
        ));
    }
    // Reject both Unix and Windows absolute paths rather than consulting the locking host.
    if path.starts_with(['/', '\\']) || path.as_bytes().get(1) == Some(&b':') {
        return Err(Error::unsupported(format!(
            "File activation requires filesystem lookup: {path}"
        )));
    }
    Ok(false)
}
/// Returns profiles in Maven's injection order: conditionally active, or defaults if none active.
pub(crate) fn select<'a, 'i>(
    root: Node<'a, 'i>,
    context: Option<&ResolutionContext>,
) -> Result<Vec<Node<'a, 'i>>> {
    let profiles: Vec<_> = child(root, "profiles")
        .into_iter()
        .flat_map(|n| n.children())
        .filter(|n| n.has_tag_name("profile"))
        .collect();
    let props = context
        .map(ResolutionContext::properties)
        .unwrap_or_default();
    let project = child(root, "properties")
        .into_iter()
        .flat_map(|n| n.children())
        .filter(Node::is_element)
        .map(|n| {
            (
                n.tag_name().name().into(),
                n.text().unwrap_or("").trim().into(),
            )
        })
        .collect();
    let mut active = Vec::new();
    let mut defaults = Vec::new();
    for profile in profiles {
        let Some(a) = child(profile, "activation") else {
            continue;
        };
        let id = text(profile, "id");
        let evaluate = || -> Result<bool> {
            let mut present = false;
            let mut matches = true;
            for criterion in a.children().filter(Node::is_element) {
                let tag = criterion.tag_name().name();
                if tag == "activeByDefault" {
                    continue;
                }
                if context.is_none() {
                    return Err(Error::unsupported(format!(
                        "Environment-dependent profile: {id}; declare resolutionContext"
                    )));
                }
                present = true;
                // Maven interpolates all activation fields before selecting profiles.
                if tag != "file" {
                    for leaf in criterion
                        .descendants()
                        .filter(|n| n.is_element() && !n.children().any(|c| c.is_element()))
                    {
                        expand(leaf.text().unwrap_or(""), &props, &project, 0)?;
                    }
                }
                // Deliberately evaluate every configured activator, even after a false result (Maven selector uses &).
                let value = match tag {
                    "jdk" => jdk(
                        &expand(criterion.text().unwrap_or("").trim(), &props, &project, 0)?,
                        required(&props, "java.version")?,
                    )?,
                    "property" => {
                        let name = expand(&text(criterion, "name"), &props, &project, 0)?;
                        let (key, not) = name
                            .strip_prefix('!')
                            .map(|s| (s, true))
                            .unwrap_or((&name, false));
                        if key.is_empty() {
                            return Err(Error::invalid(
                                "Profile activation property name is required",
                            ));
                        }
                        if key == "packaging" {
                            return Err(Error::unsupported(
                                "Maven synthetic packaging activation is not supported",
                            ));
                        }
                        let actual = props.get(key);
                        let value = expand(&text(criterion, "value"), &props, &project, 0)?;
                        if value.is_empty() {
                            actual.is_some_and(|v| !v.is_empty()) != not
                        } else {
                            negated(&value, |v| Ok(actual.is_some_and(|a| a == v)))?
                        }
                    }
                    "os" => {
                        let mut result = false;
                        for field in ["family", "name", "arch", "version"] {
                            let Some(node) = child(criterion, field) else {
                                continue;
                            };
                            if !result
                                && ["family", "name", "arch", "version"]
                                    .iter()
                                    .take_while(|s| **s != field)
                                    .any(|s| child(criterion, s).is_some())
                            {
                                continue;
                            }
                            let rule =
                                expand(node.text().unwrap_or("").trim(), &props, &project, 0)?
                                    .to_ascii_lowercase();
                            result = negated(&rule, |rule| {
                                if field == "family" {
                                    family(
                                        rule,
                                        &required(&props, "os.name")?.to_ascii_lowercase(),
                                        &props,
                                    )
                                } else {
                                    let actual = required(&props, &format!("os.{field}"))?
                                        .to_ascii_lowercase();
                                    if field == "version" && rule.starts_with("regex:") {
                                        return Err(Error::unsupported(
                                            "OS-version Java regex activation is not supported",
                                        ));
                                    }
                                    Ok(actual == rule)
                                }
                            })?;
                        }
                        result
                    }
                    "file" => file_active(criterion, &props, &project)?,
                    other => {
                        return Err(Error::unsupported(format!(
                            "Unknown profile activation: {other}"
                        )));
                    }
                };
                matches &= value;
            }
            Ok(present && matches)
        };
        let enabled = evaluate().map_err(|mut e| {
            e.message = format!("profile {id}: {}", e.message);
            e
        })?;
        if enabled {
            active.push(profile);
        } else if text(a, "activeByDefault").eq_ignore_ascii_case("true") {
            defaults.push(profile);
        }
    }
    Ok(if active.is_empty() { defaults } else { active })
}
