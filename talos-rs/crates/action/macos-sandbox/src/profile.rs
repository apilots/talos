use std::path::PathBuf;

use talos_sandbox_spi::{NetworkIsolationSpec, SandboxExecutionRequest};

const BASE_POLICY: &str = include_str!("seatbelt_base_policy.sbpl");
const RUNTIME_POLICY: &str = include_str!("seatbelt_runtime_policy.sbpl");
const NETWORK_POLICY: &str = include_str!("seatbelt_network_policy.sbpl");
pub(crate) fn build_seatbelt_profile(
    request: &SandboxExecutionRequest,
    host_tmp: Option<&std::path::Path>,
) -> (String, Vec<(String, PathBuf)>) {
    let mut definitions = Vec::new();
    let mut sections = vec![BASE_POLICY.to_string(), RUNTIME_POLICY.to_string()];

    sections.push(access_policy(
        "file-read* file-test-existence",
        "READABLE_ROOT",
        request.file_system().readable_roots(),
        &mut definitions,
    ));
    sections.push(access_policy(
        "file-write*",
        "WRITABLE_ROOT",
        request.file_system().writable_roots(),
        &mut definitions,
    ));
    sections.push(deny_policy(
        request.file_system().denied_roots(),
        &mut definitions,
    ));

    if let Some(host_tmp) = host_tmp {
        if let Ok(canonical) = std::fs::canonicalize(host_tmp) {
            if let Some(policy) = xcrun_cache_policy(host_tmp, &canonical) {
                sections.push(policy);
            }
        }
    }

    let program_name = "TARGET_PROGRAM".to_string();
    definitions.push((
        program_name.clone(),
        request.command().program().as_path().to_path_buf(),
    ));
    sections.push(format!(
        "(allow file-read* file-test-existence (literal (param \"{program_name}\")))"
    ));

    if request.network() == NetworkIsolationSpec::Enabled {
        sections.push(NETWORK_POLICY.to_string());
        sections.push("(allow network-outbound)\n(allow network-inbound)".to_string());
    }

    (sections.join("\n"), definitions)
}

fn xcrun_cache_policy(host_tmp: &std::path::Path, canonical: &std::path::Path) -> Option<String> {
    let mut rules = Vec::new();
    for root in [host_tmp, canonical] {
        let root = root.to_str().filter(|path| restricted_path(path))?;
        for ancestor in std::path::Path::new(root)
            .ancestors()
            .filter(|path| path != &std::path::Path::new("/"))
        {
            let ancestor = ancestor.to_str().filter(|path| restricted_path(path))?;
            rules.push(format!(
                r#"(allow file-read-metadata file-test-existence (literal "{ancestor}"))"#
            ));
        }
        let root = root.trim_end_matches('/').replace('.', r"\.");
        rules.push(format!(
            r#"(allow file-read* file-write* (regex #"^{root}/xcrun_db(-[^/]+)?$"))"#
        ));
    }
    Some(rules.join("\n"))
}

fn restricted_path(path: &str) -> bool {
    path.chars()
        .all(|character| character.is_ascii_alphanumeric() || "/_-.".contains(character))
}

fn access_policy(
    actions: &str,
    prefix: &str,
    roots: &[talos_sandbox_spi::AbsolutePathBuf],
    definitions: &mut Vec<(String, PathBuf)>,
) -> String {
    let filters = roots
        .iter()
        .enumerate()
        .map(|(index, root)| {
            let name = format!("{prefix}_{index}");
            definitions.push((name.clone(), root.as_path().to_path_buf()));
            format!("  (subpath (param \"{name}\"))")
        })
        .collect::<Vec<_>>();
    if filters.is_empty() {
        String::new()
    } else {
        format!("(allow {actions}\n{}\n)", filters.join("\n"))
    }
}

fn deny_policy(
    roots: &[talos_sandbox_spi::AbsolutePathBuf],
    definitions: &mut Vec<(String, PathBuf)>,
) -> String {
    roots
        .iter()
        .enumerate()
        .map(|(index, root)| {
            let name = format!("DENIED_ROOT_{index}");
            definitions.push((name.clone(), root.as_path().to_path_buf()));
            format!(
                "(deny file-read* file-write*\n  (literal (param \"{name}\"))\n  (subpath (param \"{name}\"))\n)"
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
