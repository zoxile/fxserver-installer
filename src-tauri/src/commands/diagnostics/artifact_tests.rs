use super::tests::{link_directory, Fixture};
use super::*;

#[test]
fn enhanced_artifacts_resolve_resources_and_mixed_editions_block() {
    let fixture = Fixture::new();
    fs::rename(
        fixture.root.join("artifacts/FXServer.exe"),
        fixture.root.join("artifacts/cfx-server.exe"),
    )
    .unwrap();
    fixture.artifact_resource("chat", "fx_version 'cerulean'");
    fs::write(fixture.root.join("data/server.cfg"), "ensure chat").unwrap();
    let result = report(&inspect(&fixture.request()));
    assert!(!result.blocking);
    assert_eq!(result.resource_count, 1);
    fs::write(fixture.root.join("artifacts/FXServer.exe"), "fixture").unwrap();
    let result = report(&inspect(&fixture.request()));
    assert!(result.blocking);
    assert!(result
        .checks
        .iter()
        .any(|check| check.code == "artifact-conflict"));
}

#[test]
fn artifact_manifests_resolve_startup_groups_dependencies_providers_and_execs() {
    let fixture = Fixture::new();
    fixture.artifact_resource("chat", "dependency 'build-api'");
    fixture.artifact_resource("[builders]/builder", "provide 'build-api'");
    fixture.artifact_resource("[builders]/legacy", "fx_version 'cerulean'");
    let legacy = fixture
        .root
        .join("artifacts/citizen/system_resources/[builders]/legacy");
    fs::rename(legacy.join("fxmanifest.lua"), legacy.join("__resource.lua")).unwrap();
    fixture.resource("job", "dependency 'chat'");
    let included = fixture
        .root
        .join("artifacts/citizen/system_resources/chat/settings.cfg");
    fs::write(&included, "ensure build-api\n").unwrap();
    let source = "ensure chat\nstart job\nensure [builders]\nexec @chat/settings.cfg\n";
    fs::write(fixture.root.join("data/server.cfg"), source).unwrap();

    let inspection = inspect(&fixture.request());
    assert!(!report(&inspection).blocking);
    assert_eq!(inspection.resources.len(), 4);
    assert_eq!(inspection.configs.len(), 2);
    assert_eq!(
        inspection
            .resources
            .iter()
            .filter(|item| item.origin == ResourceOrigin::Artifact)
            .count(),
        3
    );
    assert_eq!(read_bounded(&included).unwrap(), "ensure build-api\n");
    assert_eq!(
        read_bounded(&fixture.root.join("data/server.cfg")).unwrap(),
        source
    );
}

#[test]
fn missing_artifact_resources_are_not_assumed_and_unrelated_artifact_folders_are_not_scanned() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("data/server.cfg"), "ensure chat").unwrap();
    let misplaced = fixture.root.join("artifacts/resources/chat");
    fs::create_dir_all(&misplaced).unwrap();
    fs::write(misplaced.join("fxmanifest.lua"), "fx_version 'cerulean'").unwrap();
    for has_empty_system_folder in [false, true] {
        if has_empty_system_folder {
            fs::create_dir_all(fixture.root.join("artifacts/citizen/system_resources/chat"))
                .unwrap();
        }
        let result = report(&inspect(&fixture.request()));
        assert_eq!(result.resource_count, 0);
        assert!(result
            .checks
            .iter()
            .any(|item| item.code == "configured-resource-missing"
                && item.resource.as_deref() == Some("chat")
                && item.severity == Severity::Error));
    }
    fixture.resource("chat", "fx_version 'cerulean'");
    assert!(!report(&inspect(&fixture.request())).blocking);
}

#[test]
fn artifact_inventory_does_not_hide_missing_data_resources_or_invalid_artifact_path() {
    let fixture = Fixture::new();
    fixture.artifact_resource("chat", "fx_version 'cerulean'");
    fs::remove_dir(fixture.root.join("data/resources")).unwrap();
    fs::write(fixture.root.join("data/server.cfg"), "ensure chat").unwrap();
    let result = report(&inspect(&fixture.request()));
    assert_eq!(result.resource_count, 1);
    assert!(result
        .checks
        .iter()
        .any(|item| item.code == "resources-missing"));
    assert!(!result
        .checks
        .iter()
        .any(|item| item.code == "configured-resource-missing"));

    fs::remove_file(fixture.root.join("artifacts/FXServer.exe")).unwrap();
    assert_eq!(report(&inspect(&fixture.request())).resource_count, 0);
    let mut request = fixture.request();
    request.artifact_path.clear();
    assert_eq!(report(&inspect(&request)).resource_count, 0);
}

#[test]
fn same_name_across_roots_retains_custom_manifest_provides_and_groups_without_picking_a_config() {
    let fixture = Fixture::new();
    fixture.resource(
        "[data]/shared",
        "dependency 'data-only-missing'\nprovide 'old-api'",
    );
    fixture.artifact_resource("[system]/shared", "provide 'new-api'");
    let system_cfg = fixture
        .root
        .join("artifacts/citizen/system_resources/[system]/shared/settings.cfg");
    fs::write(&system_cfg, "ensure new-api").unwrap();
    fs::write(
        fixture
            .root
            .join("data/resources/[data]/shared/settings.cfg"),
        "ensure wrong-config",
    )
    .unwrap();
    fs::write(
        fixture.root.join("data/server.cfg"),
        "ensure shared\nensure [system]\nexec @shared/settings.cfg",
    )
    .unwrap();
    let inspection = inspect(&fixture.request());
    let result = report(&inspection);
    assert!(!result.blocking);
    assert_eq!(result.resource_count, 2);
    assert!(result
        .checks
        .iter()
        .any(|item| item.code == "resource-shadowed"));
    assert!(!result
        .checks
        .iter()
        .any(|item| item.code == "duplicate-resource"));
    assert!(result
        .checks
        .iter()
        .any(|item| item.code == "dependency-missing"
            && item.severity == Severity::Warning
            && item.detail.contains("data-only-missing")));
    assert!(result
        .checks
        .iter()
        .any(|item| item.code == "exec-unresolved"));
    assert_eq!(
        config_target(
            "@shared/settings.cfg",
            &fixture.root.join("data"),
            &inspection.resources
        ),
        None
    );

    fs::write(
        fixture.root.join("data/server.cfg"),
        "ensure [data]\nensure old-api",
    )
    .unwrap();
    let result = report(&inspect(&fixture.request()));
    assert!(!result.blocking);
    assert!(!result
        .checks
        .iter()
        .any(|item| item.code == "configured-resource-missing"));
}

#[test]
fn custom_chat_is_not_dropped_when_a_system_chat_manifest_also_exists() {
    let fixture = Fixture::new();
    fixture.resource(
        "[custom]/chat",
        "provide 'custom-chat-api'\ndependency 'custom-dependency'",
    );
    fixture.artifact_resource("chat", "dependency 'system-dependency'");
    fs::write(
        fixture.root.join("data/server.cfg"),
        "ensure chat\nensure [custom]\nensure custom-chat-api",
    )
    .unwrap();
    let result = report(&inspect(&fixture.request()));
    assert_eq!(result.resource_count, 2);
    assert!(!result.blocking);
    assert!(result
        .checks
        .iter()
        .any(|item| item.code == "resource-shadowed"));
    for dependency in ["custom-dependency", "system-dependency"] {
        assert!(result
            .checks
            .iter()
            .any(|item| item.code == "dependency-missing"
                && item.detail.contains(dependency)
                && item.severity == Severity::Warning));
    }
}

#[test]
fn duplicate_names_within_artifact_root_remain_ambiguous() {
    let fixture = Fixture::new();
    fixture.artifact_resource("[one]/shared", "fx_version 'cerulean'");
    fixture.artifact_resource("[two]/shared", "fx_version 'cerulean'");
    let result = report(&inspect(&fixture.request()));
    assert!(result
        .checks
        .iter()
        .any(|item| item.code == "duplicate-resource" && item.severity == Severity::Error));
}

#[test]
fn artifact_provider_transitive_dependencies_are_checked_and_not_arbitrarily_selected() {
    let fixture = Fixture::new();
    fixture.resource("job", "dependency 'build-api'");
    fixture.artifact_resource("builder", "provide 'build-api'\ndependency 'helper'");
    fixture.artifact_resource("helper", "dependency 'missing-lib'");
    fs::write(fixture.root.join("data/server.cfg"), "ensure job").unwrap();
    let result = report(&inspect(&fixture.request()));
    assert!(result
        .checks
        .iter()
        .any(|item| item.code == "dependency-missing"
            && item.resource.as_deref() == Some("helper")
            && item.severity == Severity::Error));

    for replacement in ["alternative", "build-api"] {
        fixture.resource(replacement, "provide 'build-api'");
        let result = report(&inspect(&fixture.request()));
        assert!(!result.blocking);
        assert!(result
            .checks
            .iter()
            .any(|item| item.code == "resource-provider-ambiguous"));
        assert!(result
            .checks
            .iter()
            .any(|item| item.code == "dependency-missing"
                && item.resource.as_deref() == Some("helper")
                && item.severity == Severity::Warning));
    }
    fs::write(
        fixture.root.join("data/server.cfg"),
        "ensure job\nensure builder",
    )
    .unwrap();
    assert!(report(&inspect(&fixture.request())).blocking);
}

#[test]
fn artifact_and_data_scans_share_entry_and_read_limits() {
    let fixture = Fixture::new();
    fixture.resource("job", "fx_version 'cerulean'");
    fixture.artifact_resource("chat", "fx_version 'cerulean'");
    for exhaust_entries in [true, false] {
        let mut inspection = Inspection::default();
        let mut scan = ResourceScan::default();
        let root = fixture.root.join("data/resources");
        scan_resources(&root, &root, 0, &mut scan, &mut inspection);
        assert_eq!(inspection.resources.len(), 1);
        assert!(scan.entries > 0);
        if exhaust_entries {
            scan.entries = MAX_RESOURCE_ENTRIES;
        } else {
            inspection.scan_bytes = MAX_SCAN_BYTES;
        }
        scan_artifact_resources(&fixture.root.join("artifacts"), &mut scan, &mut inspection);
        assert_eq!(inspection.resources.len(), 1);
        assert!(inspection
            .checks
            .iter()
            .any(|item| item.code == "scan-limit"));
        assert!(inspection.scan_bytes <= MAX_SCAN_BYTES);
        assert!(scan.entries <= MAX_RESOURCE_ENTRIES);
    }
}

#[test]
fn linked_artifact_root_groups_and_aliases_are_read_only_and_cycles_are_bounded() {
    let fixture = Fixture::new();
    let shared = fixture.root.join("shared");
    fs::create_dir_all(shared.join("group/actual")).unwrap();
    fs::write(
        shared.join("group/actual/fxmanifest.lua"),
        "fx_version 'cerulean'",
    )
    .unwrap();
    let system = fixture.root.join("system");
    fs::create_dir(&system).unwrap();
    fs::create_dir(fixture.root.join("artifacts/citizen")).unwrap();
    link_directory(
        &system,
        &fixture.root.join("artifacts/citizen/system_resources"),
    );
    link_directory(&shared.join("group"), &system.join("[linked]"));
    link_directory(&shared.join("group/actual"), &system.join("alias"));
    link_directory(&system, &system.join("[cycle]"));
    fs::write(
        fixture.root.join("data/server.cfg"),
        "ensure [linked]\nensure alias",
    )
    .unwrap();
    let inspection = inspect(&fixture.request());
    assert!(!report(&inspection).blocking);
    assert_eq!(inspection.resources.len(), 2);
    assert!(inspection
        .resources
        .iter()
        .all(|item| item.origin == ResourceOrigin::Artifact));
    assert_eq!(
        inspection
            .checks
            .iter()
            .filter(|item| item.code == "resource-link-cycle")
            .count(),
        1
    );
    assert_eq!(
        read_bounded(&shared.join("group/actual/fxmanifest.lua")).unwrap(),
        "fx_version 'cerulean'"
    );
}

#[test]
fn artifact_exec_cannot_escape_its_registered_resource() {
    let fixture = Fixture::new();
    fixture.artifact_resource("chat", "fx_version 'cerulean'");
    let resource = fixture.root.join("artifacts/citizen/system_resources/chat");
    fs::create_dir(fixture.root.join("private")).unwrap();
    fs::write(
        fixture.root.join("private/secret.cfg"),
        "ensure secret-marker",
    )
    .unwrap();
    link_directory(&fixture.root.join("private"), &resource.join("external"));
    fs::write(
        fixture.root.join("data/server.cfg"),
        "exec @chat/external/secret.cfg\nexec @chat/../secret.cfg",
    )
    .unwrap();
    let result = report(&inspect(&fixture.request()));
    assert_eq!(result.config_count, 1);
    assert_eq!(
        result
            .checks
            .iter()
            .filter(|item| item.code == "exec-unresolved")
            .count(),
        2
    );
    assert!(!serde_json::to_string(&result)
        .unwrap()
        .contains("secret-marker"));
}

#[test]
fn incomplete_artifact_inventory_does_not_prove_resources_or_dependencies_missing() {
    for unreadable_manifest in [false, true] {
        let fixture = Fixture::new();
        fixture.resource("job", "dependency 'chat'");
        fs::write(
            fixture.root.join("data/server.cfg"),
            "ensure job\nensure chat",
        )
        .unwrap();
        if unreadable_manifest {
            fixture.artifact_resource("chat", "");
            fs::write(
                fixture
                    .root
                    .join("artifacts/citizen/system_resources/chat/fxmanifest.lua"),
                [0xff],
            )
            .unwrap();
        } else {
            fixture.artifact_resource(
                &format!("{}chat", "[deep]/".repeat(13)),
                "fx_version 'cerulean'",
            );
        }
        let inspection = inspect(&fixture.request());
        assert!(inspection.resources_incomplete);
        let result = report(&inspection);
        assert!(!result.blocking);
        assert!(result
            .checks
            .iter()
            .any(|check| check.code == "configured-resource-unresolved"
                && check.severity == Severity::Warning));
        assert!(result
            .checks
            .iter()
            .any(|check| check.code == "dependency-unresolved"
                && check.severity == Severity::Warning));
        assert!(!result.checks.iter().any(|check| matches!(
            check.code.as_str(),
            "configured-resource-missing" | "dependency-missing"
        )));
    }
}

#[test]
fn broken_artifact_root_link_is_unknown_not_a_missing_resource() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.root.join("artifacts/citizen")).unwrap();
    let target = fixture.root.join("link-target");
    fs::create_dir(&target).unwrap();
    link_directory(
        &target,
        &fixture.root.join("artifacts/citizen/system_resources"),
    );
    fs::remove_dir(&target).unwrap();
    fs::write(fixture.root.join("data/server.cfg"), "ensure chat").unwrap();
    let inspection = inspect(&fixture.request());
    assert!(inspection.resources_incomplete);
    let result = report(&inspection);
    assert!(!result.blocking);
    assert!(result
        .checks
        .iter()
        .any(|check| check.code == "resource-unreadable"));
    assert!(result
        .checks
        .iter()
        .any(|check| check.code == "configured-resource-unresolved"));
}

#[test]
fn artifact_reads_reserve_budget_for_the_main_config_and_includes() {
    let fixture = Fixture::new();
    let manifest = format!("--{}", "x".repeat(MAX_FILE_BYTES as usize - 2));
    for index in 0..=MAX_RESOURCE_SCAN_BYTES / MAX_FILE_BYTES as usize {
        fixture.artifact_resource(&format!("large-{index}"), &manifest);
    }
    fs::write(fixture.root.join("data/server.cfg"), "exec extra.cfg\n").unwrap();
    fs::write(
        fixture.root.join("data/extra.cfg"),
        "ensure unverified-resource\n",
    )
    .unwrap();
    let inspection = inspect(&fixture.request());
    assert!(inspection.resources_incomplete);
    assert!(!inspection.configs_incomplete);
    assert_eq!(inspection.configs.len(), 2);
    assert!(inspection.scan_bytes > MAX_RESOURCE_SCAN_BYTES);
    assert!(inspection.scan_bytes <= MAX_SCAN_BYTES);
    let result = report(&inspection);
    assert!(!result.blocking);
    assert!(result.checks.iter().any(|check| check.code == "scan-limit"));
    assert!(result
        .checks
        .iter()
        .any(|check| check.code == "configured-resource-unresolved"));
    assert!(!result
        .checks
        .iter()
        .any(|check| check.code == "config-unreadable"));
}

#[test]
fn provider_warning_flood_cannot_hide_config_dependency_or_duplicate_blockers() {
    let fixture = Fixture::new();
    for pair in 0..8 {
        let manifest = (0..256)
            .map(|alias| format!("provide 'alias-{pair}-{alias}'\n"))
            .collect::<String>();
        fixture.resource(&format!("provider-{pair}"), &manifest);
        fixture.artifact_resource(&format!("other-provider-{pair}"), &manifest);
    }
    for blocker in [
        "configured-resource-missing",
        "dependency-missing",
        "duplicate-resource",
    ] {
        match blocker {
            "configured-resource-missing" => fs::write(
                fixture.root.join("data/server.cfg"),
                "ensure definitely-missing",
            )
            .unwrap(),
            "dependency-missing" => {
                fixture.resource("job", "dependency 'missing-dependency'");
                fs::write(fixture.root.join("data/server.cfg"), "ensure job").unwrap();
            }
            _ => {
                fixture.resource("[first]/duplicate", "fx_version 'cerulean'");
                fixture.artifact_resource("[second]/duplicate", "fx_version 'cerulean'");
                fixture.artifact_resource("[third]/duplicate", "fx_version 'cerulean'");
                fs::write(fixture.root.join("data/server.cfg"), "").unwrap();
            }
        }
        let inspection = inspect(&fixture.request());
        assert!(inspection.checks_limited);
        assert!(!inspection.resources_incomplete);
        let result = report(&inspection);
        assert!(result.blocking, "{blocker}");
        assert!(result.checks.len() <= MAX_CHECKS);
        assert!(
            result
                .checks
                .iter()
                .any(|check| check.code == blocker && check.severity == Severity::Error),
            "{blocker}"
        );
        assert!(result
            .checks
            .iter()
            .any(|check| check.code == "check-limit"));
    }
}

#[test]
fn dynamic_startup_warning_flood_does_not_skip_late_dependency_validation() {
    let fixture = Fixture::new();
    fixture.resource("job", "dependency 'missing-dependency'");
    fs::write(
        fixture.root.join("data/server.cfg"),
        format!("{}ensure job\n", "ensure $dynamic\n".repeat(MAX_CHECKS + 1)),
    )
    .unwrap();
    let result = report(&inspect(&fixture.request()));
    assert!(result.blocking);
    assert!(result.checks.len() <= MAX_CHECKS);
    assert!(result
        .checks
        .iter()
        .any(|check| check.code == "dependency-missing" && check.severity == Severity::Error));
}
