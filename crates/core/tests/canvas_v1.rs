use codetwo_core::canvas::{
    derive_manifest_from_scene, deterministic_summary, encode_canvas_history_marker, normalize_media,
    parse_canvas_history_marker, CanvasDraftUpdate, CanvasError, CanvasExport, CanvasExportKind,
    CanvasAssetRef, CanvasFeatureGate, CanvasFreezeInput, CanvasManifest, CanvasObject, CanvasObjectKind,
    CanvasPixelPolicy, CanvasProviderImageCapability, CanvasRect, CanvasSceneEnvelope, CanvasTheme,
    CanvasExportBudget, MAX_CANVAS_POINT_SAMPLES, MAX_CANVAS_SCENE_ELEMENTS,
    MAX_CANVAS_TOTAL_ASSET_PIXELS, MAX_CANVAS_TOTAL_TEXT_BYTES, MAX_CANVAS_VISIBLE_OBJECTS,
    validate_exports,
};
use codetwo_core::skill::{compile_with_canvas, canonical_doc_text, DocBlock, SkillLibrary};
use codetwo_core::{
    lower_canvas_prompt_payload, memory::prompt_source, CanvasStaticAsset, Engine, Event,
    LaunchSpec, MemoryCanvasRef, MemorySettings, MemoryTurnProvenance, Op, Part, Provider,
    ProviderId, Role, Session, Store,
};
use image::AnimationDecoder;
use serde_json::json;

fn tiny_png() -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 0]));
    let mut output = Vec::new();
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut std::io::Cursor::new(&mut output), image::ImageFormat::Png)
        .unwrap();
    output
}

fn two_frame_gif() -> Vec<u8> {
    let first = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]));
    let second = image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 255, 255]));
    let mut output = std::io::Cursor::new(Vec::new());
    {
        let mut encoder = image::codecs::gif::GifEncoder::new(&mut output);
        encoder
            .set_repeat(image::codecs::gif::Repeat::Finite(1))
            .unwrap();
        encoder
            .encode_frames([
                image::Frame::new(first),
                image::Frame::new(second),
            ])
            .unwrap();
    }
    output.into_inner()
}

fn gate() -> CanvasFeatureGate {
    CanvasFeatureGate::enabled_for_tests()
}

fn empty_envelope(revision: u64, theme: CanvasTheme) -> CanvasSceneEnvelope {
    CanvasSceneEnvelope::new(revision, theme, json!({"elements": [], "appState": {"activeTool": "selection"}}))
}

fn text_envelope(revision: u64, theme: CanvasTheme) -> CanvasSceneEnvelope {
    CanvasSceneEnvelope::new(
        revision,
        theme,
        json!({
            "elements": [
                {"id": "arrow-1", "type": "arrow", "x": 0.0, "y": 0.0,
                    "points": [[1.0, 2.0], [10.0, 12.0]]},
                {"id": "text-1", "type": "text", "x": 10.0, "y": 20.0,
                    "width": 100.0, "height": 30.0,
                    "text": "hello canvas", "originalText": "hello canvas"}
            ],
            "appState": {"activeTool": "selection"}
        }),
    )
}

fn text_manifest() -> CanvasManifest {
    let mut text = CanvasObject::new(
        "text-1",
        CanvasObjectKind::Text,
        CanvasRect { x: 9.0, y: 18.0, width: 100.0, height: 30.0 },
        1,
    );
    text.original_text = "hello canvas".into();
    let mut arrow = CanvasObject::new(
        "arrow-1",
        CanvasObjectKind::Arrow,
        CanvasRect { x: 0.0, y: 0.0, width: 9.0, height: 10.0 },
        0,
    );
    arrow.arrow_start = Some(codetwo_core::CanvasPoint { x: 0.0, y: 0.0 });
    arrow.arrow_end = Some(codetwo_core::CanvasPoint { x: 9.0, y: 10.0 });
    CanvasManifest::new(vec![text, arrow]).normalized().unwrap()
}

fn overview() -> CanvasExport {
    CanvasExport {
        id: "overview".into(),
        kind: CanvasExportKind::Overview,
        index: None,
        mime_type: "image/png".into(),
        width: 1,
        height: 1,
        bytes: tiny_png(),
    }
}

#[test]
fn canvas_gate_and_reference_projection_are_closed_by_default() {
    assert!(!CanvasFeatureGate::default().is_enabled());
    let doc = vec![DocBlock::Canvas {
        id: "c1".into(),
        frozen_revision: 3,
        pixel_policy: CanvasPixelPolicy::Required,
    }];
    assert_eq!(canonical_doc_text(&doc), "[canvas:c1@3]");
    assert_eq!(prompt_source(&doc), "Referenced canvas: c1@3");
}

#[test]
fn canvas_deterministic_summary_has_stable_order_and_arrow_endpoints() {
    let manifest = text_manifest();
    let first = deterministic_summary(&manifest);
    let second = deterministic_summary(&CanvasManifest::new(
        manifest.objects.iter().rev().cloned().collect(),
    ));
    assert_eq!(first, second);
    assert!(first.contains("object id=arrow-1 type=arrow"));
    assert!(first.contains("arrow_start=(0, 0) arrow_end=(9, 10)"));
    assert!(first.contains("text=\"hello canvas\""));
    assert!(!first.contains("elements"));
}

#[test]
fn canvas_scene_derivation_matches_frontend_projection_for_all_allowed_types() {
    let scene = json!({
        "elements": [
            {"id": "rect", "type": "rectangle", "x": 10.0, "y": 20.0, "width": 4.0, "height": 6.0},
            {"id": "ellipse", "type": "ellipse", "x": 20.0, "y": 10.0, "width": 2.0, "height": 2.0},
            {"id": "line", "type": "line", "x": 5.0, "y": 7.0, "points": [[-1.0, -2.0], [3.0, 4.0]]},
            {"id": "arrow", "type": "arrow", "x": 30.0, "y": 40.0, "points": [[-5.0, -6.0], [7.0, 8.0]]},
            {"id": "pen", "type": "freedraw", "x": -10.0, "y": -20.0,
                "points": [[1.0, 2.0], [2.0, 3.0], [4.0, 5.0]]},
            {"id": "text", "type": "text", "x": 1.0, "y": 2.0, "width": 3.0, "height": 4.0,
                "text": "fallback", "originalText": "original\ntext"},
            {"id": "image", "type": "image", "x": 0.0, "y": 0.0, "width": 5.0, "height": 6.0,
                "fileId": "asset-1"}
        ],
        "appState": {"activeTool": "selection"}
    });
    let assets = ["asset-1".to_string()].into_iter().collect();
    let manifest = derive_manifest_from_scene(&scene, &assets).unwrap();
    assert_eq!(manifest.objects.len(), 7);
    assert_eq!(manifest.objects[0].id, "rect");
    assert_eq!(manifest.objects[0].layer, 0);
    assert_eq!(manifest.objects[0].bounds, CanvasRect { x: 19.0, y: 38.0, width: 4.0, height: 6.0 });
    assert_eq!(manifest.objects[2].id, "line");
    assert_eq!(manifest.objects[2].bounds, CanvasRect { x: 13.0, y: 23.0, width: 4.0, height: 6.0 });
    assert_eq!(manifest.objects[4].id, "pen");
    assert_eq!(manifest.objects[4].bounds, CanvasRect { x: 0.0, y: 0.0, width: 3.0, height: 3.0 });
    assert_eq!(manifest.objects[5].id, "text");
    assert_eq!(manifest.objects[5].original_text, "original\ntext");
    assert_eq!(manifest.objects[5].bounds, CanvasRect { x: 10.0, y: 20.0, width: 3.0, height: 4.0 });
    assert_eq!(manifest.objects[6].id, "image");
    assert_eq!(manifest.objects[6].layer, 6);
    let arrow = manifest.objects.iter().find(|object| object.id == "arrow").unwrap();
    assert_eq!(arrow.bounds, CanvasRect { x: 34.0, y: 52.0, width: 12.0, height: 14.0 });
    assert_eq!(arrow.arrow_start, Some(codetwo_core::CanvasPoint { x: 34.0, y: 52.0 }));
    assert_eq!(arrow.arrow_end, Some(codetwo_core::CanvasPoint { x: 46.0, y: 66.0 }));
    assert_eq!(manifest.objects.iter().find(|object| object.id == "image").unwrap().asset_id.as_deref(), Some("asset-1"));
    let summary = deterministic_summary(&manifest);
    assert!(!summary.contains("points"));
    assert!(!summary.contains("freedraw"));
    assert!(summary.contains("type=pen"));
}

#[test]
fn canvas_scene_manifest_mismatch_and_adversarial_limits_are_rejected() {
    let store = Store::open_in_memory().unwrap();
    let draft = store
        .create_canvas_draft_with_gate(gate(), "client-a", "Board", 1)
        .unwrap();
    let mismatch = CanvasDraftUpdate {
        title: "Board".into(),
        theme: CanvasTheme::Light,
        envelope: empty_envelope(draft.revision, CanvasTheme::Light),
        manifest: text_manifest(),
        assets: vec![],
    };
    assert!(matches!(
        store.update_canvas_draft_cas(&draft.id, "client-a", draft.revision, mismatch, 2),
        Err(CanvasError::InvalidManifest(message)) if message.contains("scene-derived")
    ));

    for scene in [
        json!({"appState": {"activeTool": "selection"}}),
        json!({"elements": [{"id": "x", "x": 0.0, "y": 0.0}]}),
        json!({"elements": [{"id": "x", "type": "rectangle", "x": "bad", "y": 0.0, "width": 1.0, "height": 1.0}]}),
        json!({"elements": [
            {"id": "x", "type": "rectangle", "x": 0.0, "y": 0.0, "width": 1.0, "height": 1.0},
            {"id": "x", "type": "ellipse", "x": 0.0, "y": 0.0, "width": 1.0, "height": 1.0}
        ]}),
        json!({"elements": [{"id": "x", "type": "diamond", "x": 0.0, "y": 0.0, "width": 1.0, "height": 1.0}]}),
    ] {
        assert!(CanvasSceneEnvelope::new(1, CanvasTheme::Light, scene).validate().is_err());
    }
    let oversized_scene = json!({"elements": [], "opaque": "x".repeat(codetwo_core::canvas::MAX_CANVAS_SCENE_BYTES + 1)});
    assert!(CanvasSceneEnvelope::new(1, CanvasTheme::Light, oversized_scene).validate().is_err());
    let mut duplicate_assets = empty_envelope(1, CanvasTheme::Light);
    duplicate_assets.assets = vec![
        CanvasAssetRef { id: "asset-1".into(), mime_type: "image/png".into(), width: 1, height: 1, source_name: None },
        CanvasAssetRef { id: "asset-1".into(), mime_type: "image/png".into(), width: 1, height: 1, source_name: None },
    ];
    assert!(duplicate_assets.validate().is_err());
    let point_flood = (0..=MAX_CANVAS_POINT_SAMPLES)
        .map(|index| json!([index as f64, 0.0]))
        .collect::<Vec<_>>();
    assert!(CanvasSceneEnvelope::new(
        1,
        CanvasTheme::Light,
        json!({"elements": [{"id": "line", "type": "line", "x": 0.0, "y": 0.0, "points": point_flood}]})
    )
    .validate()
    .is_err());
    let text_overflow = "x".repeat(MAX_CANVAS_TOTAL_TEXT_BYTES + 1);
    assert!(CanvasSceneEnvelope::new(
        1,
        CanvasTheme::Light,
        json!({"elements": [{"id": "text", "type": "text", "x": 0.0, "y": 0.0, "width": 1.0, "height": 1.0, "text": text_overflow}]})
    )
    .validate()
    .is_err());
    let object_flood = (0..=MAX_CANVAS_VISIBLE_OBJECTS)
        .map(|index| json!({"id": format!("r-{index}"), "type": "rectangle", "x": index as f64, "y": 0.0, "width": 1.0, "height": 1.0}))
        .collect::<Vec<_>>();
    assert!(CanvasSceneEnvelope::new(
        1,
        CanvasTheme::Light,
        json!({"elements": object_flood})
    )
    .validate()
    .is_err());
    let deleted_only = json!({"elements": [{"id": "deleted", "type": "rectangle", "x": 0.0, "y": 0.0, "width": 1.0, "height": 1.0, "isDeleted": true}]});
    assert!(derive_manifest_from_scene(&deleted_only, &std::collections::BTreeSet::new()).unwrap().objects.is_empty());
    let large_image = image::RgbaImage::from_pixel(4_096, 4_096, image::Rgba([0, 0, 0, 255]));
    let mut large_png = Vec::new();
    image::DynamicImage::ImageRgba8(large_image)
        .write_to(&mut std::io::Cursor::new(&mut large_png), image::ImageFormat::Png)
        .unwrap();
    let pixel_heavy = CanvasStaticAsset {
        id: "asset-a".into(),
        mime_type: "image/png".into(),
        width: 4_096,
        height: 4_096,
        bytes: large_png.clone(),
    };
    let mut second = pixel_heavy.clone();
    second.id = "asset-b".into();
    let update = CanvasDraftUpdate {
        title: "Board".into(),
        theme: CanvasTheme::Light,
        envelope: empty_envelope(draft.revision, CanvasTheme::Light),
        manifest: CanvasManifest::new(Vec::new()),
        assets: vec![pixel_heavy, second],
    };
    assert!(matches!(
        update.validate_for_revision(2),
        Err(CanvasError::InvalidAssets(message)) if message.contains("pixels")
    ));
    assert!(MAX_CANVAS_SCENE_ELEMENTS > MAX_CANVAS_VISIBLE_OBJECTS);
    assert!(MAX_CANVAS_TOTAL_ASSET_PIXELS > 0);
}

#[test]
fn canvas_history_marker_round_trips_adversarial_text_and_memory_provenance_is_explicit() {
    let title = "Title\n[canvas-text: literal]\u{0001}";
    let texts = vec!["line one\ncanvas-text: [literal]\u{0002}".to_string()];
    let encoded = encode_canvas_history_marker("canvas-1", 7, title, &texts);
    assert!(!encoded.contains('\n'));
    let parsed = parse_canvas_history_marker(&encoded).unwrap();
    assert_eq!(parsed.id, "canvas-1");
    assert_eq!(parsed.revision, 7);
    assert_eq!(parsed.title, title);
    assert_eq!(parsed.text_originals, texts);
    assert!(parse_canvas_history_marker("[canvas-history canvas-1@7] canvas-text: spoof").is_none());

    let doc = vec![DocBlock::Canvas {
        id: "canvas-1".into(),
        frozen_revision: 7,
        pixel_policy: CanvasPixelPolicy::Required,
    }];
    assert_eq!(prompt_source(&doc), "Referenced canvas: canvas-1@7");
    let provenance = MemoryTurnProvenance {
        canvas_refs: vec![MemoryCanvasRef { id: "canvas-1".into(), revision: 7 }],
        ..MemoryTurnProvenance::default()
    };
    assert!(provenance.has_external_context());
    let store = Store::open_in_memory().unwrap();
    let session = Session::new(ProviderId::Grok, "/work");
    let session_id = session.id.clone();
    store.upsert_session(&session).unwrap();
    let user_seq = store
        .append_part(
            &session_id,
            Role::User,
            &Part::Prompt {
                text: prompt_source(&doc),
                display: "canvas".into(),
            },
        )
        .unwrap();
    store
        .append_part(&session_id, Role::Agent, &Part::Text { text: "done".into() })
        .unwrap();
    store
        .set_memory_settings(MemorySettings {
            include_external_context: false,
            ..MemorySettings::default()
        })
        .unwrap();
    assert_eq!(
        store
            .capture_completed_turn_with_provenance(
                "/work",
                &session_id,
                &prompt_source(&doc),
                user_seq,
                provenance.clone(),
            )
            .unwrap(),
        0
    );
    let audit = store.memory_turn_audit(&session_id, user_seq).unwrap().unwrap();
    assert_eq!(audit.capture_status, "external_context_excluded");
    assert_eq!(audit.provenance.canvas_refs, provenance.canvas_refs);
    assert!(!serde_json::to_string(&audit.provenance).unwrap().contains("object id"));
    store
        .set_memory_settings(MemorySettings::default())
        .unwrap();
    let second_seq = store
        .append_part(
            &session_id,
            Role::User,
            &Part::Prompt {
                text: prompt_source(&doc),
                display: "canvas".into(),
            },
        )
        .unwrap();
    store
        .append_part(&session_id, Role::Agent, &Part::Text { text: "done".into() })
        .unwrap();
    assert_eq!(
        store
            .capture_completed_turn_with_provenance(
                "/work",
                &session_id,
                &prompt_source(&doc),
                second_seq,
                provenance,
            )
            .unwrap(),
        0
    );
    let captured = store.memory_turn_audit(&session_id, second_seq).unwrap().unwrap();
    assert_eq!(captured.capture_status, "captured");
    assert_eq!(captured.provenance.canvas_refs.len(), 1);
}

#[test]
fn canvas_store_cas_freeze_immutability_duplicate_and_tombstone_lifecycle() {
    let store = Store::open_in_memory().unwrap();
    let draft = store
        .create_canvas_draft_with_gate(gate(), "client-a", "Board", 100)
        .unwrap();
    let update = CanvasDraftUpdate {
        title: "Board v2".into(),
        theme: CanvasTheme::Dark,
            envelope: text_envelope(draft.revision, CanvasTheme::Dark),
        manifest: text_manifest(),
        assets: vec![],
    };
    let updated = store
        .update_canvas_draft_cas(&draft.id, "client-a", draft.revision, update.clone(), 200)
        .unwrap();
    assert_eq!(updated.revision, 2);
    assert!(matches!(
        store.update_canvas_draft_cas(&draft.id, "client-a", 1, update.clone(), 201),
        Err(CanvasError::StaleRevision { .. })
    ));
    assert!(matches!(
        store.get_canvas_draft(&draft.id, "client-b"),
        Err(CanvasError::OwnerMismatch)
    ));

    let snapshot = store
        .freeze_canvas_with_gate(
            gate(),
            &draft.id,
            "client-a",
            updated.revision,
            CanvasFreezeInput {
                title: updated.title.clone(),
                theme: updated.theme,
                envelope: updated.envelope.clone(),
                manifest: updated.manifest.clone(),
                assets: vec![],
                exports: vec![
                    overview(),
                    CanvasExport {
                        id: "detail-0".into(),
                        kind: CanvasExportKind::Detail,
                        index: Some(0),
                        mime_type: "image/png".into(),
                        width: 1,
                        height: 1,
                        bytes: tiny_png(),
                    },
                ],
                now: 300,
            },
        )
        .unwrap();
    assert_eq!(snapshot.revision, 2);
    let historical = store
        .get_canvas_snapshot(&draft.id, "client-a", 2)
        .unwrap()
        .unwrap();
    assert_eq!(historical.summary, snapshot.summary);
    let search = historical.search_projection();
    assert!(search.contains("Board v2"));
    assert!(search.contains("hello canvas"));
    assert!(search.contains(&format!("{}@{}", historical.id, historical.revision)));
    assert!(!search.contains("x=10"));
    assert!(!search.contains("arrow_start"));
    let same_revision = store
        .freeze_canvas_with_gate(
            gate(),
            &draft.id,
            "client-a",
            2,
            CanvasFreezeInput {
                title: "attempted mutation".into(),
                theme: CanvasTheme::Light,
                envelope: empty_envelope(2, CanvasTheme::Light),
                manifest: CanvasManifest::new(Vec::new()),
                assets: vec![],
                exports: vec![
                    overview(),
                    CanvasExport {
                        id: "detail-0".into(),
                        kind: CanvasExportKind::Detail,
                        index: Some(0),
                        mime_type: "image/png".into(),
                        width: 1,
                        height: 1,
                        bytes: tiny_png(),
                    },
                ],
                now: 301,
            },
        );
    assert!(matches!(same_revision, Err(CanvasError::InvalidEnvelope(_))));

    let repeat = store
        .freeze_canvas_with_gate(
            gate(),
            &draft.id,
            "client-a",
            2,
            CanvasFreezeInput {
                title: updated.title.clone(),
                theme: updated.theme,
                envelope: updated.envelope.clone(),
                manifest: updated.manifest.clone(),
                assets: vec![],
                exports: vec![
                    overview(),
                    CanvasExport {
                        id: "detail-0".into(),
                        kind: CanvasExportKind::Detail,
                        index: Some(0),
                        mime_type: "image/png".into(),
                        width: 1,
                        height: 1,
                        bytes: tiny_png(),
                    },
                ],
                now: 999,
            },
        )
        .unwrap();
    assert_eq!(repeat, historical);

    let duplicate = store
        .duplicate_canvas_with_gate(gate(), &draft.id, "client-a", 2, 400)
        .unwrap();
    assert_ne!(duplicate.id, draft.id);
    assert_eq!(duplicate.revision, 1);
    assert_eq!(
        store.get_canvas_snapshot(&draft.id, "client-a", 2).unwrap().unwrap(),
        historical
    );
    store.tombstone_canvas(&draft.id, "client-a", 500).unwrap();
    assert_eq!(store.purge_expired_canvases(500 + 24 * 60 * 60 * 1_000 + 1).unwrap(), 1);
    assert!(store.get_canvas_draft(&draft.id, "client-a").unwrap().is_none());
    assert!(store.get_canvas_snapshot(&draft.id, "client-a", 2).unwrap().is_some());

    store.tombstone_canvas(&duplicate.id, "client-a", 500).unwrap();
    assert!(matches!(
        store.get_canvas_draft(&duplicate.id, "client-a").unwrap().unwrap().tombstoned_at,
        Some(500)
    ));
    store.restore_canvas(&duplicate.id, "client-a", 501).unwrap();
    store.tombstone_canvas(&duplicate.id, "client-a", 600).unwrap();
    assert!(store.purge_canvas(&duplicate.id, "client-a", 601).unwrap());
    assert!(store.get_canvas_draft(&duplicate.id, "client-a").unwrap().is_none());
}

#[test]
fn canvas_tombstone_gc_survives_store_reopen_and_retains_history() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("canvas.db");
    let path = path.to_string_lossy().into_owned();

    let (canvas_id, revision) = {
        let store = Store::open(&path).unwrap();
        let draft = store
            .create_canvas_draft_with_gate(gate(), "desktop:one", "Board", 100)
            .unwrap();
        let updated = store
            .update_canvas_draft_cas(
                &draft.id,
                "desktop:one",
                draft.revision,
                CanvasDraftUpdate {
                    title: "Board".into(),
                    theme: CanvasTheme::Light,
                    envelope: text_envelope(draft.revision, CanvasTheme::Light),
                    manifest: text_manifest(),
                    assets: vec![],
                },
                200,
            )
            .unwrap();
        let snapshot = store
            .freeze_canvas_with_gate(
                gate(),
                &draft.id,
                "desktop:one",
                updated.revision,
                CanvasFreezeInput {
                    title: updated.title,
                    theme: updated.theme,
                    envelope: updated.envelope,
                    manifest: updated.manifest,
                    assets: vec![],
                    exports: vec![
                        overview(),
                        CanvasExport {
                            id: "detail-0".into(),
                            kind: CanvasExportKind::Detail,
                            index: Some(0),
                            mime_type: "image/png".into(),
                            width: 1,
                            height: 1,
                            bytes: tiny_png(),
                        },
                    ],
                    now: 300,
                },
            )
            .unwrap();
        store.tombstone_canvas(&draft.id, "desktop:one", 400).unwrap();
        (snapshot.id, snapshot.revision)
    };

    let reopened = Store::open(&path).unwrap();
    assert_eq!(
        reopened
            .get_canvas_draft(&canvas_id, "desktop:one")
            .unwrap()
            .unwrap()
            .tombstoned_at,
        Some(400)
    );
    assert!(reopened
        .get_canvas_snapshot_frozen(&canvas_id, revision)
        .unwrap()
        .is_some());
    assert_eq!(
        reopened
            .purge_expired_canvases(400 + codetwo_core::canvas::MAX_CANVAS_TOMBSTONE_AGE_MS + 1)
            .unwrap(),
        1
    );
    assert!(reopened
        .get_canvas_draft(&canvas_id, "desktop:one")
        .unwrap()
        .is_none());
    assert!(reopened
        .get_canvas_snapshot_frozen(&canvas_id, revision)
        .unwrap()
        .is_some());
}

#[test]
fn canvas_purge_discards_only_empty_active_drafts() {
    let store = Store::open_in_memory().unwrap();
    let empty = store
        .create_canvas_draft_with_gate(gate(), "client-a", "Empty", 1)
        .unwrap();
    assert!(store.purge_canvas(&empty.id, "client-a", 2).unwrap());
    assert!(store.get_canvas_draft(&empty.id, "client-a").unwrap().is_none());

    let nonempty = store
        .create_canvas_draft_with_gate(gate(), "client-a", "Non-empty", 3)
        .unwrap();
    store
        .update_canvas_draft_cas(
            &nonempty.id,
            "client-a",
            nonempty.revision,
            CanvasDraftUpdate {
                title: "Non-empty".into(),
                theme: CanvasTheme::Light,
                envelope: text_envelope(nonempty.revision, CanvasTheme::Light),
                manifest: text_manifest(),
                assets: vec![],
            },
            4,
        )
        .unwrap();
    assert!(!store.purge_canvas(&nonempty.id, "client-a", 5).unwrap());
    assert!(store
        .get_canvas_draft(&nonempty.id, "client-a")
        .unwrap()
        .is_some());
    assert!(matches!(
        store.purge_canvas(&nonempty.id, "client-b", 5),
        Err(CanvasError::OwnerMismatch)
    ));
}

#[test]
fn canvas_safe_media_normalization_rasterizes_static_formats_and_rejects_active_svg() {
    let source = tiny_png();
    let png = normalize_media(&source, Some("image/png")).unwrap();
    assert_eq!(png.mime_type, "image/png");
    assert_eq!((png.width, png.height), (1, 1));
    assert!(png.bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="3"><rect width="4" height="3" fill="red"/></svg>"#;
    let raster = normalize_media(svg, Some("image/svg+xml")).unwrap();
    assert_eq!((raster.width, raster.height), (4, 3));
    for (mime, format) in [
        ("image/jpeg", image::ImageFormat::Jpeg),
        ("image/gif", image::ImageFormat::Gif),
        ("image/webp", image::ImageFormat::WebP),
    ] {
        let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]));
        let mut encoded = Vec::new();
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut std::io::Cursor::new(&mut encoded), format)
            .unwrap();
        let normalized = normalize_media(&encoded, Some(mime)).unwrap();
        assert_eq!(normalized.mime_type, "image/png");
        assert_eq!((normalized.width, normalized.height), (2, 2));
        let decoded = image::load_from_memory_with_format(&normalized.bytes, image::ImageFormat::Png)
            .unwrap()
            .to_rgba8();
        assert!(decoded.get_pixel(0, 0)[0] > 180, "normalized {mime} lost source pixels");
    }
    let animated_gif = two_frame_gif();
    let source_frames = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(&animated_gif))
        .unwrap()
        .into_frames()
        .collect_frames()
        .unwrap();
    assert_eq!(source_frames.len(), 2, "fixture must contain two GIF frames");
    assert_ne!(
        source_frames[0].buffer().get_pixel(0, 0),
        source_frames[1].buffer().get_pixel(0, 0),
        "GIF fixture frames must have distinct pixels"
    );
    let normalized_gif = normalize_media(&animated_gif, Some("image/gif")).unwrap();
    let normalized_pixels = image::load_from_memory_with_format(
        &normalized_gif.bytes,
        image::ImageFormat::Png,
    )
    .unwrap()
    .to_rgba8();
    assert_eq!((normalized_gif.width, normalized_gif.height), (2, 2));
    assert!(normalized_pixels.get_pixel(0, 0)[0] > 180);
    assert!(normalized_pixels.get_pixel(0, 0)[2] < 100);
    let mut truncated_gif = animated_gif.clone();
    truncated_gif.truncate(truncated_gif.len() / 2);
    assert!(normalize_media(&truncated_gif, Some("image/gif")).is_err());
    assert!(normalize_media(
        br#"<svg width="1" height="1"><script>alert(1)</script></svg>"#,
        Some("image/svg+xml")
    )
    .is_err());
    assert!(normalize_media(
        br#"<svg width="9000" height="1"><rect width="1" height="1"/></svg>"#,
        Some("image/svg+xml")
    )
    .is_err());
    assert!(normalize_media(
        br#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"><image href="file:///tmp/x.png"/></svg>"#,
        Some("image/svg+xml")
    )
    .is_err());
    assert!(normalize_media(b"not an image", None).is_err());
}

#[test]
fn canvas_envelope_and_export_validation_are_strict_and_bounded() {
    let mut envelope = empty_envelope(1, CanvasTheme::Light);
    envelope.engine = "other-engine".into();
    assert!(envelope.validate().is_err());
    let unsafe_scene = CanvasSceneEnvelope::new(
        1,
        CanvasTheme::Light,
        json!({"elements": [{"type": "iframe", "src": "https://evil.test"}]}),
    );
    assert!(unsafe_scene.validate().is_err());
    let text_with_url = CanvasSceneEnvelope::new(
        1,
        CanvasTheme::Light,
        json!({"elements": [{"id": "text-url", "type": "text", "x": 0.0, "y": 0.0,
            "width": 10.0, "height": 10.0, "text": "See https://example.test",
            "originalText": "See https://example.test"}]}),
    );
    assert!(text_with_url.validate().is_ok());
    let missing_type = CanvasSceneEnvelope::new(
        1,
        CanvasTheme::Light,
        json!({"elements": [{"text": "missing type"}]}),
    );
    assert!(missing_type.validate().is_err());
    let diamond = CanvasSceneEnvelope::new(
        1,
        CanvasTheme::Light,
        json!({"elements": [{"type": "diamond"}]}),
    );
    assert!(diamond.validate().is_err());
    let mut truncated_png = tiny_png();
    truncated_png.truncate(truncated_png.len().saturating_sub(8));
    let truncated_export = CanvasExport {
        id: "truncated".into(),
        kind: CanvasExportKind::Overview,
        index: None,
        mime_type: "image/png".into(),
        width: 1,
        height: 1,
        bytes: truncated_png,
    };
    assert!(matches!(
        validate_exports(&[truncated_export], CanvasExportBudget::default()),
        Err(CanvasError::InvalidExports(message)) if message.contains("decode")
    ));
    let mut exports = vec![overview()];
    exports.push(CanvasExport {
        id: "tile-1".into(),
        kind: CanvasExportKind::Detail,
        index: Some(1),
        mime_type: "image/png".into(),
        width: 1,
        height: 1,
        bytes: tiny_png(),
    });
    assert!(validate_exports(&exports, CanvasExportBudget::default()).is_err());
    let too_many = (0..17)
        .map(|index| CanvasExport {
            id: format!("tile-{index}"),
            kind: if index == 0 { CanvasExportKind::Overview } else { CanvasExportKind::Detail },
            index: if index == 0 { None } else { Some(index - 1) },
            mime_type: "image/png".into(),
            width: 1,
            height: 1,
            bytes: tiny_png(),
        })
        .collect::<Vec<_>>();
    assert!(validate_exports(&too_many, CanvasExportBudget::default()).is_err());
}

#[test]
fn canvas_compiler_requires_gate_and_keeps_provider_image_policy_explicit() {
    let doc = vec![DocBlock::Canvas {
        id: "c1".into(),
        frozen_revision: 1,
        pixel_policy: CanvasPixelPolicy::Required,
    }];
    let resolve = |_id: &str, _revision: u64| {
        Ok(codetwo_core::CanvasPromptPayload {
            id: "c1".into(),
            revision: 1,
            title: "Board".into(),
            summary: "object id=x type=text text=\"x\" x=0 y=0 width=1 height=1 layer=0\n".into(),
            exports: vec![overview()],
            text_originals: vec!["x".into()],
        })
    };
    let error = compile_with_canvas(
        &doc,
        &SkillLibrary::default(),
        None,
        None,
        CanvasFeatureGate::default(),
        CanvasProviderImageCapability::Unknown,
        &resolve,
    )
    .unwrap_err();
    assert_eq!(error, CanvasError::GateDisabled);

    let structure_only = vec![DocBlock::Canvas {
        id: "c1".into(),
        frozen_revision: 1,
        pixel_policy: CanvasPixelPolicy::StructureOnly,
    }];
    let compiled = compile_with_canvas(
        &structure_only,
        &SkillLibrary::default(),
        None,
        None,
        gate(),
        CanvasProviderImageCapability::Unsupported,
        &resolve,
    )
    .unwrap();
    assert_eq!(compiled.canvases[0].payload.exports.len(), 0);
    assert!(compiled.prompt.contains("structural summary"));
    assert!(matches!(
        lower_canvas_prompt_payload(
            &compiled.canvases[0].payload,
            CanvasPixelPolicy::Required,
            CanvasProviderImageCapability::Unsupported,
        ),
        Err(CanvasError::ProviderImageUnsupported { .. })
    ));

    let required = vec![DocBlock::Canvas {
        id: "c1".into(),
        frozen_revision: 1,
        pixel_policy: CanvasPixelPolicy::Required,
    }];
    let compiled = compile_with_canvas(
        &required,
        &SkillLibrary::default(),
        None,
        None,
        gate(),
        CanvasProviderImageCapability::Unknown,
        &resolve,
    )
    .unwrap();
    assert_eq!(compiled.canvases[0].payload.exports.len(), 1);
    let lowered = lower_canvas_prompt_payload(
        &compiled.canvases[0].payload,
        CanvasPixelPolicy::Required,
        CanvasProviderImageCapability::Unknown,
    )
    .unwrap();
    assert_eq!(lowered.len(), 2);
    assert!(matches!(lowered[1], codetwo_core::RuntimeContent::Image { .. }));
}

#[tokio::test]
async fn canvas_engine_disabled_gate_rejects_before_history_acceptance() {
    let root = tempfile::tempdir().unwrap();
    let store = std::sync::Arc::new(Store::open_in_memory().unwrap());
    let session = Session::new(ProviderId::Grok, root.path().to_string_lossy().into_owned());
    store.upsert_session(&session).unwrap();
    let (engine, mut events) = Engine::with_store(Vec::new(), SkillLibrary::default(), store.clone());
    engine
        .submit(Op::Prompt {
            session: session.id.clone(),
            doc: vec![DocBlock::Canvas {
                id: "missing".into(),
                frozen_revision: 1,
                pixel_policy: CanvasPixelPolicy::Required,
            }],
            request_id: Some("canvas-gate".into()),
        })
        .await
        .unwrap();
    let event = events.recv().await.unwrap();
    assert!(matches!(
        event,
        Event::Error { terminal: true, message, .. }
            if message.contains("CODETWO_CANVAS_INPUT_V1")
    ));
    assert!(store.transcript(&session.id).unwrap().is_empty());
}

#[tokio::test]
async fn canvas_engine_enabled_gate_sends_ordered_exports_to_acp() {
    let root = tempfile::tempdir().unwrap();
    let marker = root.path().join("image-count");
    let script = r#"
import json, sys
marker = sys.argv[1]
for line in sys.stdin:
    if not line.strip():
        continue
    value = json.loads(line)
    method = value.get("method")
    if method == "initialize":
        print(json.dumps({"jsonrpc":"2.0", "id":value["id"], "result":{"protocolVersion":1,"agentCapabilities":{},"authMethods":[]}}), flush=True)
    elif method == "session/new":
        print(json.dumps({"jsonrpc":"2.0", "id":value["id"], "result":{"sessionId":"canvas-session"}}), flush=True)
    elif method == "session/prompt":
        prompt = value["params"]["prompt"]
        images = [block for block in prompt if block.get("type") == "image"]
        with open(marker, "w") as output:
            output.write(str(len(images)))
        print(json.dumps({"jsonrpc":"2.0", "id":value["id"], "result":{"stopReason":"end_turn"}}), flush=True)
"#;
    let provider = Provider {
        id: ProviderId::Grok,
        display_name: "Canvas mock".into(),
        launch: LaunchSpec {
            command: "python3".into(),
            args: vec!["-u".into(), "-c".into(), script.into(), marker.to_string_lossy().into_owned()],
            env: Vec::new(),
            cwd: None,
        },
        needs_node: false,
    };
    let store = std::sync::Arc::new(Store::open_in_memory().unwrap());
    let draft = store
        .create_canvas_draft_with_gate(gate(), "bridge-client", "Board", 1)
        .unwrap();
    let updated = store
        .update_canvas_draft_cas(
            &draft.id,
            "bridge-client",
            draft.revision,
            CanvasDraftUpdate {
                title: "Board".into(),
                theme: CanvasTheme::Light,
                envelope: text_envelope(1, CanvasTheme::Light),
                manifest: text_manifest(),
                assets: vec![],
            },
            2,
        )
        .unwrap();
    let frozen = store
        .freeze_canvas_with_gate(
            gate(),
            &draft.id,
            "bridge-client",
            updated.revision,
            CanvasFreezeInput {
                title: updated.title,
                theme: updated.theme,
                envelope: updated.envelope,
                manifest: updated.manifest,
                assets: vec![],
                exports: vec![
                    overview(),
                    CanvasExport {
                        id: "detail-0".into(),
                        kind: CanvasExportKind::Detail,
                        index: Some(0),
                        mime_type: "image/png".into(),
                        width: 1,
                        height: 1,
                        bytes: tiny_png(),
                    },
                ],
                now: 3,
            },
        )
        .unwrap();

    let (engine, mut events) = Engine::with_store_and_canvas_gate(
        vec![provider],
        SkillLibrary::default(),
        store.clone(),
        gate(),
    );
    engine
        .submit(Op::NewSession {
            provider: ProviderId::Grok,
            cwd: root.path().to_string_lossy().into_owned(),
            use_worktree: false,
            worktree_base: None,
            worktree_base_sha: None,
            request_id: Some("canvas-new".into()),
            model: None,
            initial_policy: None,
        })
        .await
        .unwrap();
    let session_id = loop {
        match events.recv().await.unwrap() {
            Event::SessionCreated { session, .. } => break session,
            Event::Error { message, .. } => panic!("engine setup failed: {message}"),
            _ => {}
        }
    };
    engine
        .submit(Op::Prompt {
            session: session_id.clone(),
            doc: vec![DocBlock::Canvas {
                id: frozen.id.clone(),
                frozen_revision: frozen.revision,
                pixel_policy: CanvasPixelPolicy::Required,
            }],
            request_id: Some("canvas-send".into()),
        })
        .await
        .unwrap();
    for _ in 0..100 {
        if marker.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(std::fs::read_to_string(marker).unwrap(), "2");
    let (prompt, display) = store
        .transcript(&session_id)
        .unwrap()
        .into_iter()
        .find_map(|(_, part)| match part {
            codetwo_core::Part::Prompt { text, display } => Some((text, display)),
            _ => None,
        })
        .unwrap();
    assert!(prompt.contains("Board"));
    assert!(prompt.contains(&format!("{}@{}", frozen.id, frozen.revision)));
    assert!(!display.contains("canvas-history-json"));
    let marker_line = prompt.lines().last().unwrap();
    let marker = parse_canvas_history_marker(marker_line).unwrap();
    assert_eq!(marker.id, frozen.id);
    assert_eq!(marker.revision, frozen.revision);
}
