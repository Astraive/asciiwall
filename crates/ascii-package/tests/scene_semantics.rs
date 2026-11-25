use ascii_package::{SCENE_NODE_DESCRIPTORS, parameter_schema, validate_graph, validate_scene};
use serde_json::json;

#[test]
fn capability_declarations_drive_optional_pass_through_and_required_failure() {
    let manifest: ascii_package::Manifest =
        serde_json::from_str(include_str!("fixtures/manifest.json")).unwrap();
    let click = validate_scene(
        br##"{"scene_version":"1.0","seed":1,"nodes":[{"id":"click","kind":"click-ripple","params":{"radius_cells":1,"duration_ms":50,"strength":0}}],"edges":[],"output":"click","controls":[]}"##,
    )
    .unwrap();
    let feeds = ascii_package::SceneFeeds {
        available: [ascii_package::Capability::PointerButtons]
            .into_iter()
            .collect(),
        click_position_usable: false,
    };
    let policy = ascii_package::ScenePolicy::default();
    let optional =
        ascii_package::validate_scene_capabilities(&click, &manifest, &feeds, &policy).unwrap();
    assert_eq!(optional.pass_through_nodes, ["click"]);

    let mut required = manifest.clone();
    required.capabilities.required = vec![ascii_package::Capability::PointerButtons];
    required.capabilities.optional.clear();
    assert_eq!(
        ascii_package::validate_scene_capabilities(&click, &required, &feeds, &policy),
        Err(
            ascii_package::SceneSemanticError::MissingRequiredCapability(
                ascii_package::Capability::PointerButtons
            )
        )
    );
    let usable = ascii_package::SceneFeeds {
        available: [ascii_package::Capability::PointerButtons]
            .into_iter()
            .collect(),
        click_position_usable: true,
    };
    assert!(
        ascii_package::validate_scene_capabilities(&click, &required, &usable, &policy).is_ok()
    );
    let disabled = ascii_package::ScenePolicy {
        observation_disabled: true,
        ..Default::default()
    };
    assert_eq!(
        ascii_package::validate_scene_capabilities(&click, &required, &usable, &disabled),
        Err(
            ascii_package::SceneSemanticError::MissingRequiredCapability(
                ascii_package::Capability::PointerButtons
            )
        )
    );

    let mut undeclared = manifest;
    undeclared.capabilities.optional.clear();
    assert!(matches!(
        ascii_package::validate_scene_capabilities(&click, &undeclared, &feeds, &policy),
        Err(ascii_package::SceneSemanticError::InvalidCapability { .. })
    ));
}

#[test]
fn window_feed_and_media_asset_refs_are_checked_without_host_queries() {
    let manifest: ascii_package::Manifest =
        serde_json::from_str(include_str!("fixtures/manifest.json")).unwrap();
    let window = validate_scene(
        br##"{"scene_version":"1.0","seed":1,"nodes":[{"id":"avoid","kind":"window-avoid","params":{"margin_cells":0,"strength":0}}],"edges":[],"output":"avoid","controls":[]}"##,
    )
    .unwrap();
    let mut optional = manifest.clone();
    optional.capabilities.optional = vec![ascii_package::Capability::WindowRectangles];
    let feeds = ascii_package::SceneFeeds::default();
    let plan = ascii_package::validate_scene_capabilities(
        &window,
        &optional,
        &feeds,
        &ascii_package::ScenePolicy::default(),
    )
    .unwrap();
    assert_eq!(plan.pass_through_nodes, ["avoid"]);

    let source = validate_scene(
        br##"{"scene_version":"1.0","seed":1,"nodes":[{"id":"image","kind":"image-source","params":{"path":"not-declared.png"}}],"edges":[],"output":"image","controls":[]}"##,
    )
    .unwrap();
    assert!(ascii_package::validate_scene_source_refs(&source, &manifest).is_err());
}

#[test]
fn aggregate_resource_estimate_checks_exact_budget_boundary_without_buffers() {
    use ascii_core::Budget;
    use ascii_package::{SceneResourceInputs, SceneResourceLimits, SourceResource};

    let bytes = br##"{"scene_version":"1.0","seed":1,"nodes":[{"id":"image","kind":"image-source","params":{"path":"assets/a.png"}},{"id":"video","kind":"video-source","params":{"path":"assets/b.webm"}},{"id":"mix","kind":"composite","params":{"mode":"over","opacity":1}}],"edges":[{"from":"image","to":"mix","slot":0},{"from":"video","to":"mix","slot":1}],"output":"mix","controls":[]}"##;
    let scene = validate_scene(bytes).unwrap();
    let sources = [
        (
            "image".to_owned(),
            SourceResource {
                decoded_pixels: 100,
                decoded_bytes: 100,
            },
        ),
        (
            "video".to_owned(),
            SourceResource {
                decoded_pixels: 200,
                decoded_bytes: 200,
            },
        ),
    ]
    .into_iter()
    .collect();
    let inputs = SceneResourceInputs {
        columns: 2,
        rows: 2,
        sources,
        limits: SceneResourceLimits {
            core: Budget::new(1_000_000, 1_000_000, 1_000_000),
            max_particles: 100,
            max_decoders: 2,
            max_aggregate_memory_bytes: 1_000_000,
        },
    };
    let settings = [("scene.simulation-hz".to_owned(), json!(60))]
        .into_iter()
        .collect();
    let estimate = ascii_package::estimate_scene_resources(&scene, &inputs, &settings, 50).unwrap();
    assert_eq!(estimate.cells, 4);
    assert_eq!(estimate.decoders, 2);
    assert_eq!(estimate.work_units, 36);

    let mut at_limit = inputs.clone();
    at_limit.limits.max_aggregate_memory_bytes = estimate.aggregate_memory_bytes;
    assert_eq!(
        ascii_package::estimate_scene_resources(&scene, &at_limit, &settings, 50),
        Ok(estimate)
    );
    at_limit.limits.max_aggregate_memory_bytes -= 1;
    assert_eq!(
        ascii_package::estimate_scene_resources(&scene, &at_limit, &settings, 50),
        Err(ascii_package::SceneSemanticError::ResourceLimitExceeded(
            "aggregate memory budget"
        ))
    );
}

fn raw_scene(nodes: &str, edges: &str, output: &str) -> String {
    format!(
        r#"{{"scene_version":"1.0","seed":1,"nodes":{nodes},"edges":{edges},"output":"{output}","controls":[]}}"#
    )
}

#[test]
fn registry_is_closed_and_uses_schema_parameter_authority() {
    let expected = [
        "solid",
        "matrix-rain",
        "starfield",
        "waves",
        "noise-field",
        "cube-wireframe",
        "image-source",
        "video-source",
        "glyph-color",
        "pointer-force",
        "click-ripple",
        "window-avoid",
        "trails",
        "composite",
    ];
    assert_eq!(
        SCENE_NODE_DESCRIPTORS.map(|descriptor| descriptor.kind),
        expected
    );
    for kind in expected {
        let params = parameter_schema(kind).expect("built-in schema branch");
        assert_eq!(params["type"], "object", "{kind}");
        assert_eq!(params["additionalProperties"], false, "{kind}");
    }
}

#[test]
fn all_registered_node_parameter_minimum_and_maximum_values_are_schema_valid() {
    for descriptor in SCENE_NODE_DESCRIPTORS {
        let schema = parameter_schema(descriptor.kind).expect("schema descriptor");
        for boundary in ["minimum", "maximum"] {
            let mut params = serde_json::Map::new();
            let properties = schema["properties"].as_object().unwrap();
            for (name, property) in properties {
                let value = match property["type"].as_str().unwrap() {
                    "integer" | "number" => property[boundary].clone(),
                    "string" if property.get("enum").is_some() => property["enum"][0].clone(),
                    "string"
                        if property["pattern"]
                            .as_str()
                            .is_some_and(|p| p.contains("#")) =>
                    {
                        json!("#00000000")
                    }
                    "string" => json!("asset.png"),
                    kind => panic!("unexpected schema parameter type {kind}"),
                };
                params.insert(name.clone(), value);
            }
            let value = json!({
                "scene_version":"1.0", "seed":0,
                "nodes":[{"id":"node","kind":descriptor.kind,"params":params}],
                "edges":[], "output":"node", "controls":[]
            });
            assert!(
                validate_scene(value.to_string().as_bytes()).is_ok(),
                "{} at {boundary}",
                descriptor.kind
            );
        }
        let properties = schema["properties"].as_object().unwrap();
        let mut params = serde_json::Map::new();
        for (name, property) in properties {
            let value = match property["type"].as_str().unwrap() {
                "integer" | "number" => property["minimum"].clone(),
                "string" if property.get("enum").is_some() => property["enum"][0].clone(),
                "string"
                    if property["pattern"]
                        .as_str()
                        .is_some_and(|p| p.contains("#")) =>
                {
                    json!("#00000000")
                }
                "string" => json!("asset.png"),
                kind => panic!("unexpected schema parameter type {kind}"),
            };
            params.insert(name.clone(), value);
        }
        for (name, property) in properties {
            for boundary in ["minimum", "maximum"] {
                let outside = match (property["type"].as_str().unwrap(), boundary) {
                    ("integer", "minimum") => json!(property[boundary].as_i64().unwrap() - 1),
                    ("integer", _) => json!(property[boundary].as_i64().unwrap() + 1),
                    ("number", "minimum") => json!(property[boundary].as_f64().unwrap() - 1.0),
                    ("number", _) => json!(property[boundary].as_f64().unwrap() + 1.0),
                    _ => continue,
                };
                let mut invalid = params.clone();
                invalid.insert(name.clone(), outside);
                let value = json!({
                    "scene_version":"1.0", "seed":0,
                    "nodes":[{"id":"node","kind":descriptor.kind,"params":invalid}],
                    "edges":[], "output":"node", "controls":[]
                });
                assert!(
                    validate_scene(value.to_string().as_bytes()).is_err(),
                    "{}.{name} outside {boundary}",
                    descriptor.kind
                );
            }
        }
    }
}

#[test]
fn rejects_missing_output_cycle_duplicate_slot_and_bad_arity() {
    let node = r##"[{"id":"source","kind":"solid","params":{"color":"#00000000"}}]"##;
    assert!(
        validate_graph(&validate_scene(raw_scene(node, "[]", "missing").as_bytes()).unwrap())
            .is_err()
    );

    let unary_nodes = r#"[{"id":"a","kind":"trails","params":{"decay":0.5}},{"id":"b","kind":"trails","params":{"decay":0.5}}]"#;
    let cycle = r#"[{"from":"a","to":"b","slot":0},{"from":"b","to":"a","slot":0}]"#;
    assert!(
        validate_graph(&validate_scene(raw_scene(unary_nodes, cycle, "a").as_bytes()).unwrap())
            .is_err()
    );

    let duplicate =
        r#"[{"from":"source","to":"effect","slot":0},{"from":"source","to":"effect","slot":0}]"#;
    let unary = r##"[{"id":"source","kind":"solid","params":{"color":"#00000000"}},{"id":"effect","kind":"trails","params":{"decay":0.5}}]"##;
    assert!(
        validate_graph(&validate_scene(raw_scene(unary, duplicate, "effect").as_bytes()).unwrap())
            .is_err()
    );

    let generator_input = r#"[{"from":"source","to":"second","slot":0}]"#;
    let generators = r##"[{"id":"source","kind":"solid","params":{"color":"#00000000"}},{"id":"second","kind":"waves","params":{"amplitude":1,"frequency":1,"speed":1,"color":"#00000000"}}]"##;
    assert!(
        validate_graph(
            &validate_scene(raw_scene(generators, generator_input, "second").as_bytes()).unwrap()
        )
        .is_err()
    );

    let extra =
        r#"[{"from":"source","to":"effect","slot":0},{"from":"other","to":"effect","slot":1}]"#;
    let extra_nodes = r##"[{"id":"source","kind":"solid","params":{"color":"#00000000"}},{"id":"other","kind":"solid","params":{"color":"#00000000"}},{"id":"effect","kind":"trails","params":{"decay":0.5}}]"##;
    assert!(
        validate_graph(
            &validate_scene(raw_scene(extra_nodes, extra, "effect").as_bytes()).unwrap()
        )
        .is_err()
    );

    let composite = r##"[{"id":"base","kind":"solid","params":{"color":"#00000000"}},{"id":"overlay","kind":"solid","params":{"color":"#ffffffff"}},{"id":"mix","kind":"composite","params":{"mode":"over","opacity":1}}]"##;
    let missing_overlay = r#"[{"from":"base","to":"mix","slot":0}]"#;
    assert!(
        validate_graph(
            &validate_scene(raw_scene(composite, missing_overlay, "mix").as_bytes()).unwrap()
        )
        .is_err()
    );

    let unreachable_nodes = r##"[{"id":"selected","kind":"solid","params":{"color":"#00000000"}},{"id":"hidden","kind":"solid","params":{"color":"#ffffffff"}}]"##;
    assert!(
        validate_graph(
            &validate_scene(raw_scene(unreachable_nodes, "[]", "selected").as_bytes()).unwrap()
        )
        .is_err()
    );
}

#[test]
fn rejects_duplicate_json_keys_before_schema_validation() {
    let bytes = br##"{"scene_version":"1.0","seed":0,"nodes":[{"id":"x","kind":"solid","params":{"color":"#00000000","color":"#ffffffff"}}],"edges":[],"output":"x","controls":[]}"##;
    let error = validate_scene(bytes).unwrap_err();
    assert!(error.message.contains("unique-key"));
}

#[test]
fn schema_rejects_arbitrary_executable_node_kinds() {
    let custom = r#"{"scene_version":"1.0","seed":0,"nodes":[{"id":"custom","kind":"wgsl","params":{}}],"edges":[],"output":"custom","controls":[]}"#;
    assert!(validate_scene(custom.as_bytes()).is_err());
    let custom = r#"{"scene_version":"1.0","seed":0,"nodes":[{"id":"custom","kind":"wasm","params":{}}],"edges":[],"output":"custom","controls":[]}"#;
    assert!(validate_scene(custom.as_bytes()).is_err());
    let custom = r#"{"scene_version":"1.0","seed":0,"nodes":[{"id":"custom","kind":"script","params":{}}],"edges":[],"output":"custom","controls":[]}"#;
    assert!(validate_scene(custom.as_bytes()).is_err());
}

#[test]
fn data_only_v1_registry_does_not_require_an_executor_or_host_access() {
    assert!(SCENE_NODE_DESCRIPTORS[6].embeds_conversion);
    assert!(SCENE_NODE_DESCRIPTORS[7].embeds_conversion);
    assert!(
        SCENE_NODE_DESCRIPTORS
            .iter()
            .all(|descriptor| descriptor.output == ascii_package::ScenePortType::CharacterFrame)
    );
}

#[test]
fn media_source_path_length_boundaries_are_inclusive() {
    let maximum = "a".repeat(240);
    let over_limit = "a".repeat(241);
    for kind in ["image-source", "video-source"] {
        for (path, valid) in [
            ("a", true),
            (maximum.as_str(), true),
            ("", false),
            (over_limit.as_str(), false),
        ] {
            let value = json!({
                "scene_version":"1.0", "seed":0,
                "nodes":[{"id":"source","kind":kind,"params":{"path":path}}],
                "edges":[], "output":"source", "controls":[]
            });
            assert_eq!(
                validate_scene(value.to_string().as_bytes()).is_ok(),
                valid,
                "{kind} path length {}",
                path.len()
            );
        }
    }
}
