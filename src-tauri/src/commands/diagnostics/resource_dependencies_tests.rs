use super::super::tests::{link_directory, Fixture};
use super::*;
use std::fs;

fn request(fixture: &Fixture) -> DependencyRequest {
    let request = fixture.request();
    DependencyRequest {
        artifact_path: request.artifact_path,
        tx_data_path: request.tx_data_path,
        profile: request.profile,
    }
}

#[test]
fn includes_artifacts_aliases_constraints_and_exec_order_without_config_secrets() {
    let fixture = Fixture::new();
    fixture.artifact_resource("chat", "dependency '/onesync'\nprovide 'chat-api'");
    fixture.resource(
        "job",
        "dependencies { 'chat-api', 'missing' }\ndependency (dynamic_name)",
    );
    fs::write(
        fixture.root.join("data/server.cfg"),
        "ensure chat\nexec extra.cfg\nstart job\nrcon_password private-secret",
    )
    .unwrap();
    fs::write(fixture.root.join("data/extra.cfg"), "ensure chat-api").unwrap();
    let inventory = scan(request(&fixture)).unwrap();
    assert!(!inventory.resources_incomplete);
    let stock = inventory
        .resources
        .iter()
        .find(|resource| resource.name == "chat")
        .unwrap();
    assert_eq!(stock.origin, "artifact");
    assert_eq!(stock.dependencies, ["/onesync"]);
    assert_eq!(stock.provides, ["chat-api"]);
    assert!(
        inventory
            .resources
            .iter()
            .find(|resource| resource.name == "job")
            .unwrap()
            .dynamic
    );
    assert_eq!(
        inventory
            .startup
            .iter()
            .map(|entry| entry.target.as_str())
            .collect::<Vec<_>>(),
        ["chat", "chat-api", "job"]
    );
    assert!(!serde_json::to_string(&inventory)
        .unwrap()
        .contains("private-secret"));
}

#[test]
fn linked_resources_are_read_and_ancestor_cycles_are_reported() {
    let fixture = Fixture::new();
    let external = fixture.root.join("external");
    fs::create_dir_all(&external).unwrap();
    fs::write(external.join("fxmanifest.lua"), "dependency 'missing'").unwrap();
    let resources = fixture.root.join("data/resources");
    let alias = resources.join("linked");
    let cycle = resources.join("[cycle]");
    link_directory(&external, &alias);
    link_directory(&resources, &cycle);
    let inventory = scan(request(&fixture)).unwrap();
    assert!(inventory
        .resources
        .iter()
        .any(|resource| resource.name == "linked"));
    assert!(inventory.resources_incomplete);
    assert!(inventory
        .warnings
        .iter()
        .any(|warning| warning.code == "resource-link-cycle"));
    // Remove the links before the fixture's recursive cleanup.
    #[cfg(windows)]
    {
        fs::remove_dir(alias).unwrap();
        fs::remove_dir(cycle).unwrap();
    }
    #[cfg(unix)]
    {
        fs::remove_file(alias).unwrap();
        fs::remove_file(cycle).unwrap();
    }
}

#[test]
fn unavailable_artifact_is_explicitly_incomplete_and_invalid_profile_fails() {
    let fixture = Fixture::new();
    let mut absent = request(&fixture);
    absent.artifact_path.clear();
    assert!(scan(absent).unwrap().resources_incomplete);
    let mut invalid = request(&fixture);
    invalid.profile = "../outside".into();
    assert!(scan(invalid).is_err());
}
