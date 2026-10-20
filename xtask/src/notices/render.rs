//! Deterministic notice declarations and per-package material rendering.
use super::*;
type PackageKey = (String, String);
type ReviewedGraph<'a> = BTreeMap<PackageKey, &'a ReviewedPackage>;
type ActualGraph<'a> = BTreeMap<PackageKey, &'a Package>;

#[derive(Default)]
struct RenderedNotices {
    files: Files,
    observed: BTreeSet<(PackageKey, String, String)>,
    inventory: Vec<Value>,
}

pub(super) fn render(
    root: &Path,
    scope: &str,
    report: &About,
    review: &Review,
    lock: &Lock,
) -> Result<(Files, Vec<Value>)> {
    let (expected, actual) = reviewed_graph(root, scope, report, review, lock)?;
    let mut rendered = RenderedNotices::default();
    for license in &report.licenses {
        render_license(
            root,
            scope,
            license,
            &expected,
            &actual,
            review,
            &mut rendered,
        )?;
    }
    render_supplements(root, scope, &expected, &actual, &mut rendered)?;
    let expected_count: usize = expected
        .values()
        .map(|package| {
            package
                .bindings
                .iter()
                .filter(|binding| binding.scopes.iter().any(|value| value == scope))
                .count()
        })
        .sum();
    if rendered.observed.len() != expected_count {
        return Err("Target notices omit reviewed per-package license bindings".into());
    }
    rendered
        .inventory
        .sort_by_key(|value| serde_json::to_string(value).expect("JSON value serialization"));
    Ok((rendered.files, rendered.inventory))
}

fn reviewed_graph<'a>(
    root: &Path,
    scope: &str,
    report: &'a About,
    review: &'a Review,
    lock: &Lock,
) -> Result<(ReviewedGraph<'a>, ActualGraph<'a>)> {
    let expected: ReviewedGraph<'_> = review
        .packages
        .iter()
        .filter(|package| package.scopes.iter().any(|value| value == scope))
        .map(|package| ((package.name.clone(), package.version.clone()), package))
        .collect();
    if expected.is_empty() {
        return Err("Notice scope has no reviewed package closure".into());
    }
    if expected.len()
        != review
            .packages
            .iter()
            .filter(|package| package.scopes.iter().any(|value| value == scope))
            .count()
    {
        return Err("Duplicate reviewed package identity in one component/target scope".into());
    }
    let mut actual = BTreeMap::new();
    for entry in &report.crates {
        let key = (entry.package.name.clone(), entry.package.version.clone());
        let reviewed = expected
            .get(&key)
            .ok_or("Unreviewed package in scope notice closure")?;
        if entry.license != reviewed.license_expression
            || actual.insert(key, &entry.package).is_some()
        {
            return Err("Duplicate package or changed notice license expression".into());
        }
        verify_package(root, &entry.package, reviewed, lock)?;
        if !reviewed
            .bindings
            .iter()
            .any(|binding| binding.scopes.iter().any(|value| value == scope))
        {
            return Err(
                "Reviewed package has no legal material binding for this component and target"
                    .into(),
            );
        }
    }
    if actual.keys().ne(expected.keys()) {
        return Err("Target notice package closure differs from review".into());
    }
    Ok((expected, actual))
}

fn render_license(
    root: &Path,
    scope: &str,
    license: &License,
    expected: &ReviewedGraph<'_>,
    actual: &ActualGraph<'_>,
    review: &Review,
    rendered: &mut RenderedNotices,
) -> Result<()> {
    if license.used_by.is_empty() || license.text.is_empty() {
        return Err("Empty license group cannot prove complete notices".into());
    }
    let hash = digest(license.text.as_bytes());
    for used in &license.used_by {
        let key = (used.package.name.clone(), used.package.version.clone());
        let package = actual
            .get(&key)
            .ok_or("License group refers to a package outside its scope closure")?;
        if used.package.source != package.source
            || used.package.manifest_path != package.manifest_path
        {
            return Err("License-group package provenance differs from graph package".into());
        }
        let reviewed = expected[&key];
        let candidates: Vec<_> = reviewed
            .bindings
            .iter()
            .filter(|binding| {
                binding.scopes.iter().any(|value| value == scope)
                    && binding.license_id == license.id
                    && binding.generator_text_sha256 == hash
                    && binding.generator_text_bytes == license.text.len()
            })
            .collect();
        if candidates.len() != 1
            || !rendered
                .observed
                .insert((key.clone(), license.id.clone(), hash.clone()))
        {
            return Err("Unreviewed, ambiguous or duplicate per-package license binding".into());
        }
        let binding = candidates[0];
        let (file, additional) =
            select_material(root, review, license, binding, package, &mut rendered.files)?;
        rendered.inventory.push(json!({"name":package.name,"version":package.version,"source":package.source,
            "checksum":reviewed.checksum,"license_expression":reviewed.license_expression,
            "license_id":license.id,"generator_text_sha256":hash,"file":file,
            "review_mapping":binding.mapping,"package_archive_identity_checked":package.source.is_some(),
            "unpacked_materials_checked_separately":true,
            "materials":binding.materials,"additional_material_files":additional,
            "source_provenance":reviewed.provenance }));
    }
    Ok(())
}

fn select_material(
    root: &Path,
    review: &Review,
    license: &License,
    binding: &Binding,
    package: &Package,
    files: &mut Files,
) -> Result<(String, Vec<String>)> {
    let hash = digest(license.text.as_bytes());
    let materials = binding
        .materials
        .iter()
        .map(|material| material_bytes(root, package, material))
        .collect::<Result<Vec<_>>>()?;
    let mut additional = Vec::new();
    let selected = match binding.mapping.as_str() {
        "exact_per_package_legal_file" => {
            if materials.len() != 1 || materials[0] != license.text.as_bytes() {
                return Err("Per-package legal bytes differ from selected generator text".into());
            }
            materials[0].clone()
        }
        "complete_source_license_materials" => {
            if materials.is_empty() || materials.iter().any(Vec::is_empty) {
                return Err("Missing complete source license material".into());
            }
            for material in materials.iter().skip(1) {
                additional.push(text_file(files, material.clone())?);
            }
            materials[0].clone()
        }
        "identified_standard_text_with_source_declarations" => {
            let standard = binding
                .standard_text
                .as_ref()
                .ok_or("Missing identified standard text")?;
            if standard["sha256"] != hash
                || standard["bytes"] != license.text.len()
                || standard["license_id"] != license.id
                || materials.is_empty()
                || materials.iter().any(Vec::is_empty)
            {
                return Err("Standard template or source declarations differ from review".into());
            }
            for material in &materials {
                additional.push(text_file(files, material.clone())?);
            }
            license.text.as_bytes().to_vec()
        }
        "workspace_full_license_plus_identified_standard_mpl_text"
        | "version_and_revision_bound_upstream_explanation_plus_identified_standard_mit_text" => {
            let standard = binding
                .standard_text
                .as_ref()
                .ok_or("Missing identified standard license text")?;
            if standard["sha256"] != hash
                || standard["bytes"] != license.text.len()
                || standard["license_id"] != license.id
            {
                return Err("Identified standard legal text differs from review".into());
            }
            if binding.mapping.starts_with("workspace_") {
                if materials.len() != 1 {
                    return Err("Missing full workspace license material".into());
                }
                insert(files, "LICENSE".into(), materials[0].clone())?;
            } else {
                if materials.len() != 1 {
                    return Err("Missing upstream licensing explanation".into());
                }
                insert(
                    files,
                    "licenses/objc2/LICENSE.md".into(),
                    materials[0].clone(),
                )?;
                let source = review
                    .source_inputs
                    .iter()
                    .find(|fact| fact.path == "legal/objc2/source.json")
                    .ok_or("Missing reviewed supplement source record")?;
                insert(
                    files,
                    "licenses/objc2/source.json".into(),
                    checked_file(root, source)?,
                )?;
            }
            license.text.as_bytes().to_vec()
        }
        _ => return Err("Unknown legal-material mapping requires renewed review".into()),
    };
    let file = text_file(files, selected)?;
    additional.retain(|path| path != &file);
    Ok((file, additional))
}

fn render_supplements(
    root: &Path,
    scope: &str,
    expected: &ReviewedGraph<'_>,
    actual: &ActualGraph<'_>,
    rendered: &mut RenderedNotices,
) -> Result<()> {
    // cargo-about 0.9.2 omits selected LicenseRef text groups, including Slint's
    // bundled royalty-free license. Preserve this omission as a generator fact;
    // supplement only an explicitly reviewed exact per-package source file.
    for (key, reviewed) in expected {
        for binding in reviewed.bindings.iter().filter(|binding| {
            binding.scopes.iter().any(|value| value == scope)
                && binding.mapping == "explicit_package_license_reference_supplement"
        }) {
            if !binding.license_id.starts_with("LicenseRef-")
                || !reviewed
                    .license_expression
                    .split(|value: char| value.is_whitespace() || matches!(value, '(' | ')'))
                    .any(|value| value == binding.license_id)
                || !binding.generator_text_sha256.is_empty()
                || binding.generator_text_bytes != 0
                || binding.materials.len() != 1
                || binding.materials[0].kind != "package_file"
                || rendered
                    .observed
                    .iter()
                    .any(|(package, _, _)| package == key)
            {
                return Err("Invalid or redundant reviewed license-reference supplement".into());
            }
            let package = actual[key];
            let bytes = material_bytes(root, package, &binding.materials[0])?;
            if bytes.is_empty() {
                return Err("Empty license-reference supplement".into());
            }
            let file = text_file(&mut rendered.files, bytes)?;
            rendered
                .observed
                .insert((key.clone(), binding.license_id.clone(), String::new()));
            rendered.inventory.push(
                json!({"name":package.name,"version":package.version,"source":package.source,
                "checksum":reviewed.checksum,"license_expression":reviewed.license_expression,
                "license_id":binding.license_id,"generator_text_sha256":null,
                "generator_observation":"selected package present; license text group omitted",
                "file":file,"review_mapping":binding.mapping,
                "package_archive_identity_checked":package.source.is_some(),
                "unpacked_materials_checked_separately":true,"materials":binding.materials}),
            );
        }
    }
    Ok(())
}

pub(super) fn bundle(
    root: &Path,
    directory: &str,
    output: &str,
    required: &[&str],
    files: &mut Files,
) -> Result<Value> {
    let base = root.join(directory);
    let source = read(&base.join("source.json"), MAX_LEGAL_BYTES)?;
    let record: Value = serde_json::from_slice(&source)?;
    let materials = record["materials"]
        .as_array()
        .ok_or("Missing retained legal-material inventory")?;
    let mut names = BTreeSet::new();
    for material in materials {
        let name = material["file"]
            .as_str()
            .ok_or("Missing retained legal filename")?;
        if relative(name)?.components().count() != 1 || !names.insert(name) {
            return Err("Invalid retained legal-material filename".into());
        }
        let bytes = read(&base.join(name), MAX_LEGAL_BYTES)?;
        if material["sha256"] != digest(&bytes) {
            return Err("Retained upstream legal bytes differ from their source record".into());
        }
        insert(files, format!("{output}/{name}"), bytes)?;
    }
    if names != required.iter().copied().collect() {
        return Err(
            "Retained upstream legal-material inventory is incomplete or unexpected".into(),
        );
    }
    insert(files, format!("{output}/source.json"), source)?;
    Ok(record)
}
