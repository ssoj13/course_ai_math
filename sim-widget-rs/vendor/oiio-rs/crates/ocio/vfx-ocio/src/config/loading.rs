//! YAML → `Config` parsing methods.
//!
//! Extracted from `config.rs` during STRUCT.2.next Step 4. The `from_yaml`
//! method (and its `parse_*` private helpers) is the OCIO YAML loader —
//! a contiguous ~1000-LOC block that walks the parsed document and
//! populates the `Config` struct. All other config.rs methods (accessors,
//! mutators, processor construction) live elsewhere.
//!
//! Includes:
//! - `from_yaml` — top-level config walker
//! - `parse_colorspace` — color-space block parser
//! - `parse_named_transform` — named-transform block parser
//! - `parse_transform` / `parse_tagged_transform` — transform AST walker
//!   (the largest single block; one match arm per OCIO transform tag)

use super::*;

impl super::Config {
    /// Constructs config from parsed YAML.
    pub(super) fn from_yaml<'a>(
        root: &'a Yaml<'a>,
        working_dir: PathBuf,
        source: ConfigSource,
    ) -> OcioResult<Self> {
        let version_str = yaml_get(root, "ocio_profile_version")
            .and_then(yaml_to_string)
            .ok_or_else(|| OcioError::Yaml("missing ocio_profile_version".into()))?;

        // `ocio_profile_version` is `major` or `major.minor`, both numeric. The previous
        // parse took the leading character for the major (so "2 (draft)" and "20" both
        // read as v2) and fell back to minor 0 whenever the minor failed to parse — a
        // config declaring 2.x it could not satisfy would load as plain 2.0 and then
        // silently misinterpret the features gated on the minor.
        let unsupported = || OcioError::UnsupportedVersion {
            version: version_str.to_string(),
        };
        let (major_str, minor_str) = match version_str.split_once('.') {
            Some((major, minor)) => (major, Some(minor)),
            None => (version_str.as_ref(), None),
        };
        let version = match major_str.trim().parse::<u32>() {
            Ok(1) => ConfigVersion::V1,
            Ok(2) => ConfigVersion::V2,
            _ => return Err(unsupported()),
        };
        let version_minor = match minor_str {
            Some(minor) => minor.trim().parse::<u32>().map_err(|_| unsupported())?,
            None => 0,
        };

        let strict_parsing = yaml_bool(root, "strictparsing").unwrap_or(true);

        let (environment_vars, env_mode_override) = parse_environment(root);
        let mut context = Context::new();
        for (name, value) in &environment_vars {
            match resolve_env_value(value) {
                Ok(resolved) => context.set_var(name.clone(), resolved),
                Err(_) => context.set_var(name.clone(), value.clone()),
            }
        }

        let search_paths = parse_search_paths(root, &working_dir, &context);
        let default_luma_coefs =
            parse_luma(root).unwrap_or([REC709_LUMA_R, REC709_LUMA_G, REC709_LUMA_B]);
        let family_separator = yaml_str(root, "family_separator")
            .unwrap_or("/")
            .to_string();
        let default_view_transform =
            yaml_str(root, "default_view_transform").map(|s| s.to_string());

        // Sync context with resolved search_paths and working_dir (OCIO parity for resolve_file_location)
        context.set_working_dir(working_dir.to_string_lossy().to_string());
        context.clear_search_paths();
        for p in &search_paths {
            context.add_search_path(p.to_string_lossy().to_string());
        }

        let mut config = Self {
            name: yaml_str(root, "name").unwrap_or("").to_string(),
            description: sanitize_description(yaml_str(root, "description").unwrap_or("")),
            version,
            version_minor,
            working_dir_is_default: true,
            source,
            archive_dir: None,
            working_dir: working_dir.clone(),
            search_paths,
            colorspaces: Vec::new(),
            roles: Roles::new(),
            displays: DisplayManager::new(),
            looks: LookManager::new(),
            active_displays: yaml_str_list(root, "active_displays"),
            active_views: yaml_str_list(root, "active_views"),
            shared_views: parse_shared_views(root),
            virtual_display: parse_virtual_display(root),
            viewing_rules: parse_viewing_rules(root),
            named_transforms: Vec::new(), // Parsed below after config is created
            // OCIO's YAML has no key for it (inactive named transforms are listed in
            // `inactive_colorspaces`).
            inactive_named_transforms: Vec::new(),
            inactive_colorspaces: yaml_str_list(root, "inactive_colorspaces"),
            file_rules: Vec::new(),
            environment_vars,
            context,
            strict_parsing,
            family_separator,
            environment_mode: env_mode_override.unwrap_or(EnvironmentMode::LoadPredefined),
            default_luma_coefs,
            default_view_transform,
            cache_id: None,
            io_proxy: None,
            processor_cache_flags: 0,
        };

        // Parse roles
        if let Some(roles) = yaml_get(root, "roles")
            && let Yaml::Mapping(map) = unwrap_tagged(roles)
        {
            for (k, v) in map.iter() {
                if let (Some(role), Some(cs)) = (yaml_as_str(k), yaml_as_str(v)) {
                    config.roles.define(role, cs);
                }
            }
        }

        // Colour spaces: `colorspaces` (scene-referred) and `display_colorspaces` (OCIO v2) go
        // into the one list OCIO keeps (`m_colorspaces`), in the order the two sections appear
        // in the file (`OCIOYaml.cpp` load walks the document's keys and calls `addColorSpace`
        // as it meets them). OCIO's own configs write `display_colorspaces` first, so display
        // spaces come first in `getColorSpaceNames` there, and here.
        let mut sections: Vec<(&Yaml, bool)> = Vec::new();
        if let Yaml::Mapping(map) = unwrap_tagged(root) {
            for (k, v) in map.iter() {
                match yaml_as_str(k) {
                    Some("colorspaces") => sections.push((v, false)),
                    Some("display_colorspaces") => sections.push((v, true)),
                    _ => {}
                }
            }
        }
        for (section, display) in sections {
            let Yaml::Sequence(seq) = unwrap_tagged(section) else {
                continue;
            };
            for cs_yaml in seq {
                match config.parse_colorspace(cs_yaml) {
                    Ok(mut cs) => {
                        if display {
                            cs.set_reference_space_type(
                                crate::display::ReferenceSpaceType::Display,
                            );
                        }
                        config.colorspaces.push(cs);
                    }
                    Err(e) => {
                        if strict_parsing {
                            return Err(e);
                        }
                    }
                }
            }
        }

        // Parse named transforms (OCIO v2.0+)
        if let Some(named) = yaml_get(root, "named_transforms")
            && let Yaml::Sequence(seq) = unwrap_tagged(named)
        {
            for nt_yaml in seq {
                if let Ok(nt) = config.parse_named_transform(nt_yaml) {
                    config.named_transforms.push(nt);
                }
            }
        }

        // Parse displays
        if let Some(displays) = yaml_get(root, "displays")
            && let Yaml::Mapping(map) = unwrap_tagged(displays)
        {
            for (name_yaml, views_yaml) in map.iter() {
                if let Some(name) = yaml_as_str(name_yaml) {
                    let mut display = Display::new(name);
                    if let Yaml::Sequence(views) = unwrap_tagged(views_yaml) {
                        for view_yaml in views {
                            if let Yaml::Tagged(tag, inner) = view_yaml
                                && tag.suffix == "Views"
                            {
                                if let Yaml::Sequence(refs) = inner.as_ref() {
                                    for r in refs {
                                        if let Some(name) = yaml_to_string(r) {
                                            display.add_shared_view_ref(name);
                                        }
                                    }
                                }
                                continue;
                            }

                            let view_yaml = unwrap_tagged(view_yaml);
                            if let Some(vname) = yaml_str(view_yaml, "name") {
                                let colorspace = yaml_str(view_yaml, "colorspace")
                                    .or_else(|| yaml_str(view_yaml, "display_colorspace"))
                                    .unwrap_or("");
                                if !colorspace.is_empty() {
                                    let mut view = View::new(vname, colorspace);
                                    if let Some(looks) = yaml_str(view_yaml, "looks")
                                        .or_else(|| yaml_str(view_yaml, "look"))
                                        && !looks.is_empty()
                                    {
                                        view = view.with_look(looks.to_string());
                                    }
                                    if let Some(vt) = yaml_str(view_yaml, "view_transform")
                                        && !vt.is_empty()
                                    {
                                        view = view.with_view_transform(vt.to_string());
                                    }
                                    if let Some(rule) = yaml_str(view_yaml, "rule")
                                        && !rule.is_empty()
                                    {
                                        view = view.with_rule(rule.to_string());
                                    }
                                    if let Some(desc) = yaml_str(view_yaml, "description")
                                        && !desc.is_empty()
                                    {
                                        view = view.with_description(desc.to_string());
                                    }
                                    display.add_view(view);
                                }
                            }
                        }
                    }
                    config.displays.add_display(display);
                }
            }
        }

        // Parse looks
        if let Some(looks) = yaml_get(root, "looks")
            && let Yaml::Sequence(seq) = unwrap_tagged(looks)
        {
            for look_yaml in seq {
                let look_yaml = unwrap_tagged(look_yaml);
                if let Some(name) = yaml_str(look_yaml, "name") {
                    let mut look = Look::new(name)
                        .process_space(
                            yaml_str(look_yaml, "process_space")
                                .unwrap_or("")
                                .to_string(),
                        )
                        .description(sanitize_description(
                            yaml_str(look_yaml, "description").unwrap_or(""),
                        ));

                    if let Some(t) = yaml_get(look_yaml, "transform")
                        && let Some(parsed) = config.parse_optional_transform(t)?
                    {
                        look = look.transform(parsed);
                    }

                    if let Some(t) = yaml_get(look_yaml, "inverse_transform")
                        && let Some(parsed) = config.parse_optional_transform(t)?
                    {
                        look = look.inverse_transform(parsed);
                    }

                    // OCIO `interchange:` block — `Look::m_interchangeAttributes`.
                    // Each key inside becomes one named attribute (e.g.
                    // `amf_transform_ids` for AMF round-tripping).
                    if let Some(Yaml::Mapping(map)) = yaml_get(look_yaml, "interchange") {
                        for (k, v) in map {
                            let Some(key) = k.as_str() else { continue };
                            let Some(val) = v.as_str() else { continue };
                            look.set_interchange_attribute(key, sanitize_description(val));
                        }
                    }

                    config.looks.add(look);
                }
            }
        }

        // Parse view transforms (v2)
        if let Some(view_transforms) = yaml_get(root, "view_transforms") {
            use crate::display::ReferenceSpaceType;

            if let Yaml::Sequence(seq) = unwrap_tagged(view_transforms) {
                for vt_yaml in seq {
                    let vt_yaml = unwrap_tagged(vt_yaml);
                    if let Some(name) = yaml_str(vt_yaml, "name") {
                        // OCIO `peekViewTransformReferenceSpace`: display when it has a
                        // to/from_display_reference, scene when it has a to/from_scene_reference,
                        // and exactly one of the two.
                        let has = |k: &str| yaml_get(vt_yaml, k).is_some();
                        let is_display =
                            has("from_display_reference") || has("to_display_reference");
                        let is_scene = has("from_scene_reference") || has("to_scene_reference");
                        let ref_space = match (is_scene, is_display) {
                            (false, false) => {
                                return Err(OcioError::Validation(
                                    "The '!<ViewTransform>' needs to refer to a transform.".into(),
                                ));
                            }
                            (true, true) => {
                                return Err(OcioError::Validation(
                                    "The '!<ViewTransform>' cannot have both to/from_reference and to/from_display_reference transforms.".into(),
                                ));
                            }
                            (_, true) => ReferenceSpaceType::Display,
                            _ => ReferenceSpaceType::Scene,
                        };

                        let mut vt = ViewTransform::with_reference_space(name, ref_space)
                            .with_description(sanitize_description(
                                yaml_str(vt_yaml, "description").unwrap_or(""),
                            ));

                        if let Some(family) = yaml_str(vt_yaml, "family") {
                            vt = vt.with_family(family.to_string());
                        }
                        for category in yaml_str_list(vt_yaml, "categories") {
                            vt.add_category(category);
                        }

                        if let Some(t) = yaml_get(vt_yaml, "from_scene_reference")
                            && let Some(parsed) = config.parse_optional_transform(t)?
                        {
                            vt = vt.with_from_scene_reference(parsed);
                        }
                        if let Some(t) = yaml_get(vt_yaml, "to_scene_reference")
                            && let Some(parsed) = config.parse_optional_transform(t)?
                        {
                            vt = vt.with_to_scene_reference(parsed);
                        }
                        if let Some(t) = yaml_get(vt_yaml, "from_display_reference")
                            && let Some(parsed) = config.parse_optional_transform(t)?
                        {
                            vt = vt.with_from_display_reference(parsed);
                        }
                        if let Some(t) = yaml_get(vt_yaml, "to_display_reference")
                            && let Some(parsed) = config.parse_optional_transform(t)?
                        {
                            vt = vt.with_to_display_reference(parsed);
                        }

                        // OCIO `interchange:` block —
                        // `ViewTransform::m_interchangeAttributes`.
                        if let Some(Yaml::Mapping(map)) = yaml_get(vt_yaml, "interchange") {
                            for (k, v) in map {
                                let Some(key) = k.as_str() else { continue };
                                let Some(val) = v.as_str() else { continue };
                                vt.set_interchange_attribute(key, sanitize_description(val));
                            }
                        }

                        config.displays.add_view_transform(vt);
                    }
                }
            }
        }

        // Parse file rules (OCIO v2)
        if let Some(file_rules) = yaml_get(root, "file_rules") {
            if let Yaml::Sequence(seq) = unwrap_tagged(file_rules) {
                for rule_yaml in seq {
                    let rule_yaml = unwrap_tagged(rule_yaml);
                    let Some(name) = yaml_str(rule_yaml, "name") else {
                        continue;
                    };

                    // The special `ColorSpaceNamePathSearch` rule
                    // (FileRules.cpp:25 `FilePathSearchRuleName`) carries no
                    // `colorspace` field of its own — the colorspace comes
                    // from scanning the matched path. Requiring `colorspace`
                    // to be present (as the generic branch below does) would
                    // silently drop this rule from the config entirely.
                    if name.eq_ignore_ascii_case("ColorSpaceNamePathSearch") {
                        config.file_rules.push(FileRule {
                            name: "ColorSpaceNamePathSearch".to_string(),
                            colorspace: String::new(),
                            kind: FileRuleKind::PathSearch,
                            custom_keys: parse_custom_keys(rule_yaml),
                        });
                        continue;
                    }

                    if let Some(colorspace) = yaml_str(rule_yaml, "colorspace") {
                        let kind = if name.eq_ignore_ascii_case("Default") {
                            FileRuleKind::Default
                        } else if let Some(regex_str) = yaml_str(rule_yaml, "regex") {
                            // OCIO matches file-rule regexes with
                            // std::regex_match, i.e. the ENTIRE path must
                            // match, not `regex_search`'s substring semantics
                            // (FileRules.cpp:492-493: `regex_match(path,
                            // reg)`). The `regex` crate's `is_match` is a
                            // substring search, so anchor at compile time to
                            // reproduce full-match behavior.
                            let regex = Regex::new(&format!("^(?:{regex_str})$")).map_err(|e| {
                                OcioError::Validation(format!(
                                    "invalid regex rule '{}': {}",
                                    name, e
                                ))
                            })?;
                            FileRuleKind::Regex { regex }
                        } else {
                            let pattern = yaml_str(rule_yaml, "pattern").unwrap_or("").to_string();
                            let extension = yaml_str(rule_yaml, "extension").map(|s| s.to_string());
                            // Validate the way the reference does: build the
                            // actual combined `^(pattern)(\.extension)$`
                            // regex (BuildRegularExpression) rather than
                            // validating `pattern`/`extension` as two
                            // independent globs — a glob that's fine alone
                            // but produces an invalid regex once combined
                            // (or vice versa) must be caught the same way
                            // matching will (dis)allow it later.
                            let ext_for_validation =
                                extension.as_deref().unwrap_or("").trim_start_matches('.');
                            build_file_rule_regex(&pattern, ext_for_validation)
                                .and_then(|expr| Regex::new(&expr).ok())
                                .ok_or_else(|| {
                                    OcioError::Validation(format!(
                                        "invalid file rule pattern/extension in rule '{}'",
                                        name
                                    ))
                                })?;
                            FileRuleKind::Basic { pattern, extension }
                        };

                        config.file_rules.push(FileRule {
                            name: name.to_string(),
                            colorspace: colorspace.to_string(),
                            kind,
                            custom_keys: parse_custom_keys(rule_yaml),
                        });
                    }
                }
            }

            // Validate file rules
            if strict_parsing {
                let default_idx = config
                    .file_rules
                    .iter()
                    .position(|r| matches!(r.kind, FileRuleKind::Default));
                if default_idx.is_none() {
                    return Err(OcioError::Validation(
                        "file_rules must include a Default rule".into(),
                    ));
                }
                if let Some(idx) = default_idx
                    && idx + 1 != config.file_rules.len()
                {
                    return Err(OcioError::Validation(
                        "Default rule must be the last file rule".into(),
                    ));
                }
            }
        }

        Ok(config)
    }

    /// Parses a colorspace from YAML.
    fn parse_colorspace(&self, yaml: &Yaml) -> OcioResult<ColorSpace> {
        let yaml = unwrap_tagged(yaml);
        let name = yaml_str(yaml, "name")
            .ok_or_else(|| OcioError::Yaml("colorspace missing name".into()))?;

        let mut builder = ColorSpace::builder(name);

        if let Some(desc) = yaml_str(yaml, "description") {
            builder = builder.description(sanitize_description(desc));
        }

        if let Some(family) = yaml_str(yaml, "family") {
            builder = builder.family_name(family);
        }

        if let Some(encoding) = yaml_str(yaml, "encoding") {
            let parsed = Encoding::parse(encoding);
            builder = builder.encoding(parsed);
            if parsed == Encoding::Unknown && !encoding.is_empty() {
                builder = builder.encoding_name(encoding);
            }
        }

        if let Some(bitdepth) = yaml_str(yaml, "bitdepth") {
            builder = builder.bit_depth(parse_bit_depth(bitdepth));
        }

        if yaml_bool(yaml, "isdata") == Some(true) {
            builder = builder.is_data(true);
        }

        if let Some(eq) = yaml_str(yaml, "equalitygroup") {
            builder = builder.equality_group(eq);
        }

        if let Some(interop_id) = yaml_str(yaml, "interop_id") {
            builder = builder.interop_id(interop_id);
        }

        if let Some(aliases) = yaml_get(yaml, "aliases")
            && let Yaml::Sequence(seq) = unwrap_tagged(aliases)
        {
            for alias in seq {
                if let Some(s) = yaml_as_str(alias) {
                    builder = builder.alias(s);
                }
            }
        }

        if let Some(categories) = yaml_get(yaml, "categories")
            && let Yaml::Sequence(seq) = unwrap_tagged(categories)
        {
            for cat in seq {
                if let Some(s) = yaml_as_str(cat) {
                    builder = builder.category(s);
                }
            }
        }

        let alloc_type =
            yaml_str(yaml, "allocation").map(crate::transform::AllocationType::from_name);
        let alloc_vars = yaml_f64_list(yaml, "allocationvars");
        if alloc_type.is_some() || !alloc_vars.is_empty() {
            let vars = alloc_vars.into_iter().map(|v| v as f32).collect::<Vec<_>>();
            let allocation = crate::colorspace::AllocationInfo {
                alloc_type: alloc_type.unwrap_or(crate::colorspace::AllocationType::Uniform),
                vars,
            };
            builder = builder.allocation(allocation);
        }

        // OCIO `interchange:` block (`ColorSpace::m_interchangeAttributes`).
        if let Some(Yaml::Mapping(map)) = yaml_get(yaml, "interchange") {
            for (k, v) in map {
                if let (Some(key), Some(val)) = (yaml_as_str(k), yaml_as_str(v)) {
                    builder = builder.interchange_attribute(key, sanitize_description(val));
                }
            }
        }

        // Parse transforms - OCIO v1: to_reference/from_reference
        // OCIO v2: to_scene_reference/from_scene_reference + display variants
        let transform_fields = [
            ("to_reference", "to_ref"),
            ("to_scene_reference", "to_ref"),
            ("from_reference", "from_ref"),
            ("from_scene_reference", "from_ref"),
            ("to_display_reference", "to_disp"),
            ("from_display_reference", "from_disp"),
        ];

        for (field, kind) in transform_fields {
            if let Some(t) = yaml_get(yaml, field)
                && let Some(parsed) = self.parse_optional_transform(t)?
            {
                builder = match kind {
                    "to_ref" => builder.to_reference(parsed),
                    "from_ref" => builder.from_reference(parsed),
                    "to_disp" => builder.to_display_reference(parsed),
                    "from_disp" => builder.from_display_reference(parsed),
                    _ => builder,
                };
            }
        }

        Ok(builder.build())
    }

    /// Parses a named transform from YAML (OCIO v2.0+).
    fn parse_named_transform(&self, yaml: &Yaml) -> OcioResult<NamedTransform> {
        let yaml = unwrap_tagged(yaml);
        let name = yaml_str(yaml, "name")
            .ok_or_else(|| OcioError::Yaml("named_transform missing name".into()))?;

        let forward = if let Some(t) = yaml_get(yaml, "transform") {
            self.parse_optional_transform(t)?
        } else {
            None
        };

        let inverse = if let Some(t) = yaml_get(yaml, "inverse_transform") {
            self.parse_optional_transform(t)?
        } else {
            None
        };

        Ok(NamedTransform {
            name: name.to_string(),
            aliases: yaml_str_list(yaml, "aliases"),
            family: yaml_str(yaml, "family").map(|s| s.to_string()),
            // OCIO loads a named transform's description as is (no newline sanitising).
            description: yaml_str(yaml, "description").map(|s| s.to_string()),
            categories: yaml_str_list(yaml, "categories"),
            encoding: yaml_str(yaml, "encoding").map(|s| s.to_string()),
            forward,
            inverse,
        })
    }

    /// Parses a transform slot of a color space, look, view or named transform. Any error fails
    /// the whole load, as in OCIO (`load(const YAML::Node&, TransformRcPtr&)` throws).
    fn parse_optional_transform(&self, yaml: &Yaml) -> OcioResult<Option<Transform>> {
        self.parse_transform(yaml).map(Some)
    }

    /// Parses a tagged transform (`!<MatrixTransform> {...}`). OCIO accepts only a tagged
    /// mapping here (`Only Mapping types supported`).
    fn parse_transform(&self, yaml: &Yaml) -> OcioResult<Transform> {
        if let Yaml::Tagged(tag, inner) = yaml
            && matches!(unwrap_tagged(inner.as_ref()), Yaml::Mapping(_))
        {
            return self.parse_tagged_transform(&tag.suffix, inner.as_ref());
        }
        if matches!(yaml, Yaml::Mapping(_)) {
            // An untagged mapping (not even `{children: [...]}` is a transform in OCIO).
            return Err(OcioError::Validation(
                "'?' parsing failed: Unsupported transform type !<?> in OCIO profile.".into(),
            ));
        }
        Err(OcioError::Validation(
            "Unsupported Transform type encountered in OCIO profile. Only Mapping types supported."
                .into(),
        ))
    }

    /// Parses a tagged transform: the keys and defaults of `OCIOYaml.cpp`'s
    /// `load(const YAML::Node&, <Type>TransformRcPtr&)`. Keys OCIO does not know are ignored (OCIO
    /// logs a warning); the transform `name` (FormatMetadata) is not kept by this port.
    fn parse_tagged_transform(&self, tag: &str, yaml: &Yaml) -> OcioResult<Transform> {
        let yaml = unwrap_tagged(yaml);
        let direction = parse_direction(yaml_str(yaml, "direction"))?;
        // FormatMetadata `name`, which OCIO reads for the transforms that carry one.
        let name = yaml_str(yaml, "name").unwrap_or("").to_string();

        match tag {
            "MatrixTransform" => Ok(Transform::Matrix(MatrixTransform {
                name: name.clone(),
                matrix: match ocio_f64_vec(yaml, "matrix", &[16], tag)? {
                    Some(v) => v.try_into().unwrap(),
                    None => MatrixTransform::IDENTITY,
                },
                offset: match ocio_f64_vec(yaml, "offset", &[4], tag)? {
                    Some(v) => v.try_into().unwrap(),
                    None => [0.0; 4],
                },
                direction,
            })),

            "FileTransform" => {
                // Kept as written (OCIO `FileTransform::getSrc`); the config resolves it against
                // its context and search paths when it builds a processor (`expand_transform`).
                let src = yaml_str(yaml, "src").unwrap_or("");
                // OCIO key `cdl_style`; the default is CDL_TRANSFORM_DEFAULT (no clamp).
                let cdl_style = yaml_str(yaml, "cdl_style")
                    .map(parse_cdl_style)
                    .transpose()?
                    .unwrap_or(CdlStyle::NoClamp);
                Ok(Transform::FileTransform(FileTransform {
                    src: std::path::PathBuf::from(src),
                    ccc_id: yaml_str(yaml, "cccid").map(|s| s.to_string()),
                    cdl_style,
                    interpolation: parse_interpolation(yaml_str(yaml, "interpolation"))?,
                    direction,
                }))
            }

            "ExponentTransform" => {
                let value = vec4_or_scalar(yaml, "value", 1.0, tag)?.unwrap_or([1.0; 4]);
                Ok(Transform::Exponent(ExponentTransform {
                    name: name.clone(),
                    value,
                    // OCIO's ExponentTransform default is CLAMP.
                    negative_style: yaml_str(yaml, "style")
                        .map(parse_negative_style)
                        .transpose()?
                        .unwrap_or(NegativeStyle::Clamp),
                    direction,
                }))
            }

            "LogTransform" => Ok(Transform::Log(LogTransform {
                name: name.clone(),
                base: scalar_f64(yaml, "base", tag)?.unwrap_or(2.0),
                direction,
            })),

            "CDLTransform" => {
                let v3 = |key: &str, default: f64| -> OcioResult<[f64; 3]> {
                    Ok(match ocio_f64_vec(yaml, key, &[3], tag)? {
                        Some(v) => [v[0], v[1], v[2]],
                        None => [default; 3],
                    })
                };
                Ok(Transform::Cdl(CdlTransform {
                    name: name.clone(),
                    slope: v3("slope", 1.0)?,
                    offset: v3("offset", 0.0)?,
                    power: v3("power", 1.0)?,
                    // OCIO reads `saturation` and `sat` (and writes `sat`).
                    saturation: match yaml_get(yaml, "saturation").or_else(|| yaml_get(yaml, "sat"))
                    {
                        Some(v) => yaml_as_f64(unwrap_tagged(v))
                            .ok_or_else(|| value_error(tag, "sat", "expected a number"))?,
                        None => 1.0,
                    },
                    // CDL_TRANSFORM_DEFAULT is CDL_NO_CLAMP.
                    style: yaml_str(yaml, "style")
                        .map(parse_cdl_style)
                        .transpose()?
                        .unwrap_or(CdlStyle::NoClamp),
                    direction,
                }))
            }

            "ColorSpaceTransform" => Ok(Transform::ColorSpace(ColorSpaceTransform {
                src: yaml_str(yaml, "src").unwrap_or("").to_string(),
                dst: yaml_str(yaml, "dst").unwrap_or("").to_string(),
                data_bypass: yaml_bool(yaml, "data_bypass").unwrap_or(true),
                direction,
            })),

            "BuiltinTransform" => Ok(Transform::Builtin(BuiltinTransform {
                style: yaml_str(yaml, "style").unwrap_or("").to_string(),
                direction,
            })),

            "RangeTransform" => {
                let style = yaml_str(yaml, "style")
                    .map(parse_range_style)
                    .transpose()?
                    .unwrap_or(RangeStyle::Clamp);
                Ok(Transform::Range(RangeTransform {
                    name: name.clone(),
                    min_in: scalar_f64(yaml, "min_in_value", tag)?,
                    max_in: scalar_f64(yaml, "max_in_value", tag)?,
                    min_out: scalar_f64(yaml, "min_out_value", tag)?,
                    max_out: scalar_f64(yaml, "max_out_value", tag)?,
                    style,
                    direction,
                }))
            }

            "FixedFunctionTransform" => {
                let style_str = yaml_str(yaml, "style").ok_or_else(|| {
                    OcioError::Validation(
                        "'FixedFunctionTransform' parsing failed: style value is missing.".into(),
                    )
                })?;
                let style = parse_fixed_function_style(style_str)?;
                Ok(Transform::FixedFunction(FixedFunctionTransform {
                    name: name.clone(),
                    style,
                    params: ocio_f64_vec(yaml, "params", &[], tag)?.unwrap_or_default(),
                    direction,
                }))
            }

            "ExposureContrastTransform" => {
                let style = yaml_str(yaml, "style")
                    .map(parse_exposure_contrast_style)
                    .transpose()?
                    .unwrap_or(ExposureContrastStyle::Linear);
                // A missing exposure/contrast/gamma is dynamic in OCIO, at its default value
                // (`OCIOYaml.cpp:1273-1285`, "Missing values are dynamic").
                let exposure = scalar_f64(yaml, "exposure", tag)?;
                let contrast = scalar_f64(yaml, "contrast", tag)?;
                let gamma = scalar_f64(yaml, "gamma", tag)?;
                Ok(Transform::ExposureContrast(ExposureContrastTransform {
                    name: name.clone(),
                    exposure: exposure.unwrap_or(0.0),
                    contrast: contrast.unwrap_or(1.0),
                    gamma: gamma.unwrap_or(1.0),
                    pivot: scalar_f64(yaml, "pivot", tag)?.unwrap_or(0.18),
                    log_exposure_step: scalar_f64(yaml, "log_exposure_step", tag)?.unwrap_or(0.088),
                    log_mid_gray: scalar_f64(yaml, "log_midway_gray", tag)?.unwrap_or(0.435),
                    style,
                    direction,
                    dynamic: crate::EcDynamic {
                        exposure: exposure.is_none(),
                        contrast: contrast.is_none(),
                        gamma: gamma.is_none(),
                    },
                }))
            }

            "LookTransform" => Ok(Transform::Look(LookTransform {
                src: yaml_str(yaml, "src").unwrap_or("").to_string(),
                dst: yaml_str(yaml, "dst").unwrap_or("").to_string(),
                looks: yaml_str(yaml, "looks").unwrap_or("").to_string(),
                // OCIO's YAML has no key for it (a runtime setting).
                skip_color_space_conversion: false,
                direction,
            })),

            "GroupTransform" => {
                let mut transforms = Vec::new();
                if let Some(children) = yaml_get(yaml, "children") {
                    let Yaml::Sequence(seq) = unwrap_tagged(children) else {
                        return Err(value_error(
                            tag,
                            "children",
                            "Child transform could not be parsed.",
                        ));
                    };
                    for item in seq {
                        transforms.push(self.parse_transform(item)?);
                    }
                }
                Ok(Transform::Group(GroupTransform {
                    name: name.clone(),
                    transforms,
                    direction,
                }))
            }

            "GradingPrimaryTransform" => {
                let style = match yaml_str(yaml, "style")
                    .map(parse_grading_style)
                    .transpose()?
                {
                    Some(0) => GradingPrimaryStyle::Linear,
                    Some(2) => GradingPrimaryStyle::Video,
                    _ => GradingPrimaryStyle::Log,
                };
                // `GradingPrimary{style}` defaults, then every key present (`OCIOYaml.cpp`
                // load(GradingPrimaryTransformRcPtr)): RGBM maps `{rgb, master}`, `pivot:
                // {contrast, black, white}`, `clamp: {black, white}`.
                let mut value = GradingPrimaryTransform::new(style).value;
                for (key, field) in [
                    ("brightness", &mut value.brightness),
                    ("contrast", &mut value.contrast),
                    ("gamma", &mut value.gamma),
                    ("offset", &mut value.offset),
                    ("exposure", &mut value.exposure),
                    ("lift", &mut value.lift),
                    ("gain", &mut value.gain),
                ] {
                    if let Some(v) = parse_grading_rgbm(yaml, key, tag)? {
                        *field = v;
                    }
                }
                if let Some(s) = scalar_f64(yaml, "saturation", tag)? {
                    value.saturation = s;
                }
                if let Some(pivot) = yaml_get(yaml, "pivot") {
                    let pivot = sub_map(pivot, tag, "pivot")?;
                    let (c, b, w) = (
                        scalar_f64(pivot, "contrast", tag)?,
                        scalar_f64(pivot, "black", tag)?,
                        scalar_f64(pivot, "white", tag)?,
                    );
                    if c.is_none() && b.is_none() && w.is_none() {
                        return Err(value_error(
                            tag,
                            "pivot",
                            "At least one of the pivot values must be provided.",
                        ));
                    }
                    value.pivot = c.unwrap_or(value.pivot);
                    value.pivot_black = b.unwrap_or(value.pivot_black);
                    value.pivot_white = w.unwrap_or(value.pivot_white);
                }
                if let Some(clamp) = yaml_get(yaml, "clamp") {
                    let clamp = sub_map(clamp, tag, "clamp")?;
                    let (b, w) = (
                        scalar_f64(clamp, "black", tag)?,
                        scalar_f64(clamp, "white", tag)?,
                    );
                    if b.is_none() && w.is_none() {
                        return Err(value_error(
                            tag,
                            "clamp",
                            "At least one of the clamp values must be provided.",
                        ));
                    }
                    value.clamp_black = b.unwrap_or(value.clamp_black);
                    value.clamp_white = w.unwrap_or(value.clamp_white);
                }
                Ok(Transform::GradingPrimary(GradingPrimaryTransform {
                    name: name.clone(),
                    style,
                    value,
                    direction,
                }))
            }

            "GradingRGBCurveTransform" => {
                let style = match yaml_str(yaml, "style")
                    .map(parse_grading_style)
                    .transpose()?
                {
                    Some(0) => GradingToneStyle::Linear,
                    Some(2) => GradingToneStyle::Video,
                    _ => GradingToneStyle::Log,
                };
                let mut t = GradingRgbCurveTransform::new(style);
                t.name = name.clone();
                t.direction = direction;
                t.bypass_lin_to_log = yaml_bool(yaml, "lintolog_bypass").unwrap_or(false);
                for (key, pts, slopes) in [
                    ("red", &mut t.red, &mut t.red_slopes),
                    ("green", &mut t.green, &mut t.green_slopes),
                    ("blue", &mut t.blue, &mut t.blue_slopes),
                    ("master", &mut t.master, &mut t.master_slopes),
                ] {
                    if let Some((p, s)) = parse_bspline_curve(yaml, key, tag)? {
                        *pts = p;
                        *slopes = s;
                    }
                }
                Ok(Transform::GradingRgbCurve(t))
            }

            "GradingHueCurveTransform" => {
                let style = match yaml_str(yaml, "style")
                    .map(parse_grading_style)
                    .transpose()?
                {
                    Some(0) => GradingHueCurveStyle::Linear,
                    Some(2) => GradingHueCurveStyle::Video,
                    _ => GradingHueCurveStyle::Log,
                };
                let mut t = GradingHueCurveTransform::new(style);
                t.name = name.clone();
                t.direction = direction;
                if let Some(v) = yaml_str(yaml, "hsy_transform") {
                    if v != "none" {
                        return Err(value_error(
                            tag,
                            "hsy_transform",
                            "Unknown hsy_transform value.",
                        ));
                    }
                    t.rgb_to_hsy = HsyTransformStyle::None;
                }
                for (key, pts, slopes) in [
                    ("hue_hue", &mut t.hue_hue, &mut t.hue_hue_slopes),
                    ("hue_sat", &mut t.hue_sat, &mut t.hue_sat_slopes),
                    ("hue_lum", &mut t.hue_lum, &mut t.hue_lum_slopes),
                    ("lum_sat", &mut t.lum_sat, &mut t.lum_sat_slopes),
                    ("sat_sat", &mut t.sat_sat, &mut t.sat_sat_slopes),
                    ("lum_lum", &mut t.lum_lum, &mut t.lum_lum_slopes),
                    ("sat_lum", &mut t.sat_lum, &mut t.sat_lum_slopes),
                    ("hue_fx", &mut t.hue_fx, &mut t.hue_fx_slopes),
                ] {
                    if let Some((p, s)) = parse_bspline_curve(yaml, key, tag)? {
                        *pts = p;
                        *slopes = s;
                    }
                }
                Ok(Transform::GradingHueCurve(t))
            }

            "GradingToneTransform" => {
                let style = match yaml_str(yaml, "style")
                    .map(parse_grading_style)
                    .transpose()?
                {
                    Some(0) => GradingToneStyle::Linear,
                    Some(2) => GradingToneStyle::Video,
                    _ => GradingToneStyle::Log,
                };
                // `GradingTone{style}` defaults; zones are `{rgb, master, start|center,
                // width|pivot}` maps (midtones use `center`, shadows/highlights `pivot`).
                let d = crate::grading_tone::GradingTone::from_style(style);
                let zone = |key: &str, default, center: bool, pivot: bool| {
                    parse_grading_rgbmsw(yaml, key, tag, center, pivot)
                        .map(|z| z.unwrap_or(default))
                };
                Ok(Transform::GradingTone(GradingToneTransform {
                    name: name.clone(),
                    style,
                    blacks: zone("blacks", d.blacks, false, false)?,
                    shadows: zone("shadows", d.shadows, false, true)?,
                    midtones: zone("midtones", d.midtones, true, false)?,
                    highlights: zone("highlights", d.highlights, false, true)?,
                    whites: zone("whites", d.whites, false, false)?,
                    scontrast: scalar_f64(yaml, "s_contrast", tag)?.unwrap_or(1.0),
                    direction,
                }))
            }

            "LogAffineTransform" => Ok(Transform::LogAffine(LogAffineTransform {
                name: name.clone(),
                base: scalar_f64(yaml, "base", tag)?.unwrap_or(2.0),
                log_side_slope: log_param(yaml, "log_side_slope", tag)?.unwrap_or([1.0; 3]),
                log_side_offset: log_param(yaml, "log_side_offset", tag)?.unwrap_or([0.0; 3]),
                lin_side_slope: log_param(yaml, "lin_side_slope", tag)?.unwrap_or([1.0; 3]),
                lin_side_offset: log_param(yaml, "lin_side_offset", tag)?.unwrap_or([0.0; 3]),
                direction,
            })),

            "LogCameraTransform" => Ok(Transform::LogCamera(LogCameraTransform {
                name: name.clone(),
                base: scalar_f64(yaml, "base", tag)?.unwrap_or(2.0),
                log_side_slope: log_param(yaml, "log_side_slope", tag)?.unwrap_or([1.0; 3]),
                log_side_offset: log_param(yaml, "log_side_offset", tag)?.unwrap_or([0.0; 3]),
                lin_side_slope: log_param(yaml, "lin_side_slope", tag)?.unwrap_or([1.0; 3]),
                lin_side_offset: log_param(yaml, "lin_side_offset", tag)?.unwrap_or([0.0; 3]),
                lin_side_break: log_param(yaml, "lin_side_break", tag)?.ok_or_else(|| {
                    OcioError::Validation(
                        "LogCameraTransform parse error: lin_side_break values are missing.".into(),
                    )
                })?,
                linear_slope: log_param(yaml, "linear_slope", tag)?,
                direction,
            })),

            "AllocationTransform" => {
                let allocation = yaml_str(yaml, "allocation")
                    .map_or(AllocationType::Uniform, AllocationType::from_name);
                Ok(Transform::Allocation(AllocationTransform {
                    allocation,
                    vars: ocio_f64_vec(yaml, "vars", &[], tag)?.unwrap_or_default(),
                    direction,
                }))
            }

            "ExponentWithLinearTransform" => {
                const ERR: &str = "ExponentWithLinear parse error, ";
                let gamma = vec4_or_scalar(yaml, "gamma", 1.0, tag)?;
                let offset = vec4_or_scalar(yaml, "offset", 0.0, tag)?;
                let (gamma, offset) = match (gamma, offset) {
                    (Some(g), Some(o)) => (g, o),
                    (None, None) => {
                        return Err(OcioError::Validation(format!(
                            "{ERR}gamma and offset fields are missing"
                        )));
                    }
                    (None, _) => {
                        return Err(OcioError::Validation(format!(
                            "{ERR}gamma field is missing"
                        )));
                    }
                    (_, None) => {
                        return Err(OcioError::Validation(format!(
                            "{ERR}offset field is missing"
                        )));
                    }
                };
                // OCIO's default for ExponentWithLinear is LINEAR.
                let negative_style = match yaml_str(yaml, "style") {
                    Some(s) => parse_negative_style(s)?,
                    None => NegativeStyle::Linear,
                };
                let ewl = ExponentWithLinearTransform {
                    name: name.clone(),
                    gamma,
                    offset,
                    negative_style,
                    direction,
                };
                // OCIO's reader goes through setNegativeStyle, which rejects CLAMP / PASS_THRU
                // for a moncurve, so the whole config fails to load (B24).
                ewl.validate()?;
                Ok(Transform::ExponentWithLinear(ewl))
            }

            "DisplayViewTransform" => Ok(Transform::DisplayView(DisplayViewTransform {
                src: yaml_str(yaml, "src").unwrap_or("").to_string(),
                display: yaml_str(yaml, "display").unwrap_or("").to_string(),
                view: yaml_str(yaml, "view").unwrap_or("").to_string(),
                looks_bypass: yaml_bool(yaml, "looks_bypass").unwrap_or(false),
                data_bypass: yaml_bool(yaml, "data_bypass").unwrap_or(true),
                // OCIO's YAML has no looks-override keys (a runtime setting).
                looks_override: String::new(),
                looks_override_enabled: false,
                direction,
            })),

            // No `Lut1DTransform` / `Lut3DTransform` arms: OCIO's YAML has no inline LUTs (a
            // config names a LUT file through FileTransform), so these tags fail the load below
            // as in OCIO.
            _ => Err(OcioError::Validation(format!(
                "'{tag}' parsing failed: Unsupported transform type !<{tag}> in OCIO profile."
            ))),
        }
    }
}
