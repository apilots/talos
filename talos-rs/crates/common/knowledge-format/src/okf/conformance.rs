use std::collections::HashSet;

use okf_core::{
    computation::ATTESTED_COMPUTATION_TYPE, trust::STATUS_VALUES, yaml::Value, ComputationSource,
    Document, Frontmatter, ResourceKind, Source, Verification,
};

use super::{OkfDiagnosticSeverity, OkfValidationReport};

pub(crate) fn validate_document_families(document: &Document, report: &mut OkfValidationReport) {
    check_recommended(document, report);
    check_tags(&document.frontmatter, report);
    check_trust(&document.frontmatter, report);
    check_lifecycle(&document.frontmatter, report);
    check_provenance(&document.frontmatter, report);
    check_attribution(document, report);
    check_legacy(document, report);
    check_computation(document, report);
}

fn check_recommended(document: &Document, report: &mut OkfValidationReport) {
    for field in document.missing_recommended() {
        if field == "runtime" {
            continue;
        }
        report.push_fixable(
            OkfDiagnosticSeverity::Warning,
            None,
            None,
            format!("missing recommended frontmatter field `{field}`"),
            field == "title" || field == "generated",
        );
    }
}

fn check_tags(frontmatter: &Frontmatter, report: &mut OkfValidationReport) {
    let Some(value) = frontmatter
        .get("tags")
        .filter(|value| !value.is_empty_value())
    else {
        return;
    };
    if !matches!(value, Value::Sequence(_)) {
        warn(
            report,
            format!(
                "`tags` should be a list of short strings, found {}; no tags are read from it",
                type_name(value)
            ),
        );
    }
}

fn check_trust(frontmatter: &Frontmatter, report: &mut OkfValidationReport) {
    if let Some(value) = frontmatter
        .get("generated")
        .filter(|value| !value.is_empty_value())
    {
        match frontmatter.generated() {
            None => warn(
                report,
                format!(
                    "`generated` should be a `{{ by, at }}` mapping, found {}",
                    type_name(value)
                ),
            ),
            Some(generated) => {
                if generated.by.is_none() {
                    warn(report, "`generated.by` is required within `generated`");
                }
                if let Some(at) = generated.at.filter(|value| !value.is_valid()) {
                    warn(
                        report,
                        format!(
                            "`generated.at` is not an ISO-8601 datetime with an explicit offset: {:?}",
                            at.raw
                        ),
                    );
                }
            }
        }
    }

    let Some(value) = frontmatter
        .get("verified")
        .filter(|value| !value.is_empty_value())
    else {
        return;
    };
    if !matches!(value, Value::Sequence(_) | Value::Mapping(_)) {
        warn(
            report,
            format!(
                "`verified` should be a list of `{{ by, at }}` events (a bare mapping is read as a one-element list), found {}",
                type_name(value)
            ),
        );
        return;
    }
    if frontmatter.verified().is_empty() {
        warn(report, "`verified` contains no `{ by, at }` events");
    }
    match value {
        Value::Sequence(items) => {
            for (index, item) in items.iter().enumerate() {
                let Some(event) = Verification::from_value(item) else {
                    warn(
                        report,
                        format!(
                            "`verified[{index}]` should be a mapping with `by` and `at`, found {}",
                            type_name(item)
                        ),
                    );
                    continue;
                };
                check_verification_event(index, &event, report);
            }
        }
        Value::Mapping(_) => {
            if let Some(event) = Verification::from_value(value) {
                check_verification_event(0, &event, report);
            }
        }
        _ => unreachable!("verified shape checked above"),
    }
}

fn check_verification_event(index: usize, event: &Verification, report: &mut OkfValidationReport) {
    if event
        .by
        .as_ref()
        .is_none_or(|actor| actor.as_str().trim().is_empty())
    {
        warn(report, format!("`verified[{index}].by` is missing"));
    }
    match &event.at {
        None => warn(report, format!("`verified[{index}].at` is missing")),
        Some(at) if !at.is_valid() => warn(
            report,
            format!(
                "`verified[{index}].at` is not an ISO-8601 datetime with an explicit offset: {:?}",
                at.raw
            ),
        ),
        Some(_) => {}
    }
}

fn check_lifecycle(frontmatter: &Frontmatter, report: &mut OkfValidationReport) {
    let status = frontmatter.status();
    if !status.is_known() {
        warn(
            report,
            format!(
                "unknown `status` value {:?}; the spec defines {} (consumers must still accept it)",
                status.to_string(),
                STATUS_VALUES.join(", ")
            ),
        );
    }
    if let Some(stale_after) = frontmatter.stale_after().filter(|value| !value.is_valid()) {
        warn(
            report,
            format!(
                "`stale_after` is not an ISO-8601 datetime with an explicit offset: {:?}",
                stale_after.raw
            ),
        );
    }
}

fn check_provenance(frontmatter: &Frontmatter, report: &mut OkfValidationReport) {
    let Some(value) = frontmatter
        .get("sources")
        .filter(|value| !value.is_empty_value())
    else {
        if frontmatter.get("usage_window").is_some() {
            warn(
                report,
                "`usage_window` is present without `sources` to frame",
            );
        }
        return;
    };
    if !matches!(value, Value::Sequence(_) | Value::Mapping(_)) {
        warn(
            report,
            format!(
                "`sources` should be a list of entries, found {}",
                type_name(value)
            ),
        );
        return;
    }

    let shared_window = frontmatter.usage_window();
    if let Some(window) = &shared_window {
        for (field, date) in [("from", &window.from), ("to", &window.to)] {
            if let Some(date) = date.as_ref().filter(|date| !date.is_valid()) {
                warn(
                    report,
                    format!(
                        "`usage_window.{field}` is not an ISO-8601 datetime with an explicit offset: {:?}",
                        date.raw
                    ),
                );
            }
        }
    }

    let mut seen_ids = HashSet::new();
    let entries: Vec<(usize, Source)> = match value {
        Value::Sequence(items) => items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                if item.as_mapping().is_none() {
                    warn(
                        report,
                        format!(
                            "`sources[{index}]` should be a mapping entry, found {}",
                            type_name(item)
                        ),
                    );
                    None
                } else {
                    Source::from_value(item).map(|source| (index, source))
                }
            })
            .collect(),
        Value::Mapping(_) => Source::from_value(value)
            .into_iter()
            .map(|source| (0, source))
            .collect(),
        _ => unreachable!("sources shape checked above"),
    };
    for (index, source) in &entries {
        if source.resource_kind() == ResourceKind::Missing {
            warn(
                report,
                format!("`sources[{index}].resource` is required within an entry"),
            );
        }
        if let Some(id) = &source.id {
            if !seen_ids.insert(id.clone()) {
                warn(
                    report,
                    format!(
                        "duplicate `sources[].id` {id:?}; ids are the join key for attribution"
                    ),
                );
            }
        }
        if let Some(last_modified) = source
            .last_modified
            .as_ref()
            .filter(|value| !value.is_valid())
        {
            warn(
                report,
                format!(
                    "`sources[{index}].last_modified` is not an ISO-8601 datetime with an explicit offset: {:?}",
                    last_modified.raw
                ),
            );
        }
        if source.usage_count.is_some()
            && source
                .effective_usage_window(shared_window.as_ref())
                .is_none()
        {
            warn(
                report,
                format!("`sources[{index}].usage_count` has no `usage_window` to frame it"),
            );
        }
    }
    if let Value::Sequence(items) = value {
        for (index, item) in items.iter().enumerate() {
            let raw = item
                .as_mapping()
                .and_then(|mapping| mapping.get("usage_count"));
            if let Some(raw) = raw.filter(|value| value.as_int().is_none()) {
                warn(
                    report,
                    format!(
                        "`sources[{index}].usage_count` should be an integer, found {}",
                        type_name(raw)
                    ),
                );
            }
        }
    }
}

fn check_attribution(document: &Document, report: &mut OkfValidationReport) {
    let has_sources = !document.frontmatter.sources().is_empty();
    for attribution in document.attributions() {
        if !attribution.is_resolved() && has_sources {
            warn(
                report,
                format!(
                    "footnote [^{}] matches no `sources[].id`; the label is the join key for attribution",
                    attribution.label
                ),
            );
        }
        if attribution.references > 0 && attribution.definitions == 0 {
            warn(
                report,
                format!(
                    "footnote [^{}] is cited but never defined",
                    attribution.label
                ),
            );
        }
    }
}

fn check_legacy(document: &Document, report: &mut OkfValidationReport) {
    let frontmatter = &document.frontmatter;
    if !is_blank(frontmatter, "timestamp") {
        let message = if is_blank(frontmatter, "generated") {
            "`timestamp` is superseded by `generated: { by, at }`"
        } else {
            "`timestamp` is redundant alongside `generated` and should be removed"
        };
        warn_fixable(report, message);
    }
    if document.has_legacy_citations() {
        warn_fixable(
            report,
            "the body `# Citations` list is superseded by the `sources` frontmatter field",
        );
    }
}

fn check_computation(document: &Document, report: &mut OkfValidationReport) {
    let frontmatter = &document.frontmatter;
    let computation_keys = [
        "runtime",
        "parameters",
        "computation",
        "executor",
        "attester",
    ];
    if !frontmatter.is_attested_computation() {
        let present: Vec<_> = computation_keys
            .iter()
            .copied()
            .filter(|key| !is_blank(frontmatter, key))
            .collect();
        if !present.is_empty() {
            info(
                report,
                format!(
                    "carries computation field(s) `{}` but `type` is not `{ATTESTED_COMPUTATION_TYPE}`; a sanctioned computation is its own concept",
                    present.join("`, `")
                ),
            );
        }
        return;
    }

    let Some(contract) = document.attested_computation() else {
        return;
    };
    if contract.runtime.is_none() {
        warn(
            report,
            "`runtime` is required on an `Attested Computation`; it defines what `parameters` mean",
        );
    }
    match &contract.computation {
        ComputationSource::Missing => warn(
            report,
            "no computation: set `computation` to a path or add a `# Computation` block to the body",
        ),
        ComputationSource::File(_) if contract.has_redundant_inline => warn(
            report,
            "`computation` names a file and the body also has a `# Computation` block; the spec asks for one or the other",
        ),
        _ => {}
    }
    for (index, parameter) in contract.parameters.iter().enumerate() {
        if parameter.name.is_none() {
            warn(report, format!("`parameters[{index}].name` is missing"));
        }
        if parameter.type_.is_none() {
            warn(report, format!("`parameters[{index}].type` is missing"));
        }
    }
    match &contract.executor {
        None => warn(
            report,
            "missing `executor`: nothing says how to run the computation",
        ),
        Some(executor) => {
            if executor.resource.is_none() {
                warn(
                    report,
                    "`executor.resource` is missing; it names the run instructions or code",
                );
            }
            if executor.receipt.is_empty() {
                warn(
                    report,
                    "`executor.receipt` is empty; it declares the evidence the attester inspects",
                );
            }
        }
    }
    match &contract.attester {
        None => warn(
            report,
            "missing `attester`: nothing can check a run's receipt",
        ),
        Some(attester) if attester.resource.is_none() => warn(
            report,
            "`attester.resource` is missing; it names the deterministic check",
        ),
        Some(_) => {}
    }
}

fn warn(report: &mut OkfValidationReport, message: impl Into<String>) {
    report.push(OkfDiagnosticSeverity::Warning, None, None, message);
}

fn warn_fixable(report: &mut OkfValidationReport, message: impl Into<String>) {
    report.push_fixable(OkfDiagnosticSeverity::Warning, None, None, message, true);
}

fn info(report: &mut OkfValidationReport, message: impl Into<String>) {
    report.push(OkfDiagnosticSeverity::Info, None, None, message);
}

fn is_blank(frontmatter: &Frontmatter, key: &str) -> bool {
    frontmatter.get(key).is_none_or(Value::is_empty_value)
}

const fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Int(_) => "an integer",
        Value::Float(_) => "a float",
        Value::String(_) => "a string",
        Value::Sequence(_) => "a list",
        Value::Mapping(_) => "a mapping",
    }
}
