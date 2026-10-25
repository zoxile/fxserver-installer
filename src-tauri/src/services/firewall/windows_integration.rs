use super::*;

const ACK: &str = "github-hosted-disposable-v1";
const ENV_KEYS: [&str; 5] = [
    "GITHUB_ACTIONS",
    "RUNNER_ENVIRONMENT",
    "RUNNER_OS",
    "GITHUB_RUN_ID",
    "FXI_DISPOSABLE_WINDOWS_FIREWALL",
];

fn allowed_environment(values: &[String; 5]) -> bool {
    values[0] == "true"
        && values[1] == "github-hosted"
        && values[2] == "Windows"
        && values[3].parse::<u64>().is_ok_and(|id| id > 0)
        && values[4] == ACK
}

#[test]
fn disposable_guard_rejects_workstations_and_partial_opt_in() {
    let expected = [
        "true".into(),
        "github-hosted".into(),
        "Windows".into(),
        "123".into(),
        ACK.into(),
    ];
    assert!(allowed_environment(&expected));
    for i in 0..expected.len() {
        let mut values = expected.clone();
        values[i] = String::new();
        assert!(!allowed_environment(&values));
    }
    let mut self_hosted = expected;
    self_hosted[1] = "self-hosted".into();
    assert!(!allowed_environment(&self_hosted));
}

fn fixture(operation: &str, nonce: &str, request: FirewallRequest) -> Result<(), String> {
    assert!(matches!(
        operation,
        "create" | "verify" | "modify" | "cleanup"
    ));
    assert!(nonce.len() == 32 && nonce.bytes().all(|b| b.is_ascii_hexdigit()));
    let script = format!(
        "$fixtureOperation='{operation}';$nonce='{nonce}';$tcpPort={};$udpPort={};\n{}",
        request.tcp_port,
        request.udp_port,
        include_str!("windows-roundtrip.ps1")
    );
    let mut command = powershell_command(&script)?;
    for key in ENV_KEYS {
        command.env(key, std::env::var(key).unwrap_or_default());
    }
    super::super::runtime::checked(&mut command).map(|_| ())
}

#[test]
#[ignore = "Mutates only fixture rules on explicitly opted-in disposable GitHub-hosted Windows runners"]
fn disposable_windows_firewall_roundtrip() {
    let env = ENV_KEYS.map(|key| std::env::var(key).unwrap_or_default());
    assert!(
        allowed_environment(&env),
        "Refusing firewall mutations outside the disposable CI opt-in."
    );
    assert!(
        super::super::elevation::is_elevated().unwrap(),
        "CI must already have an administrator token; never request UAC in this test."
    );
    let mut random = [0u8; 16];
    getrandom::fill(&mut random).unwrap();
    let nonce: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let port = 50000 + (u16::from_be_bytes([random[0], random[1]]) % 2000) * 3;
    let request = FirewallRequest {
        backend: FirewallBackend::Windows,
        tcp_port: port,
        udp_port: port + 1,
    };
    let before = query(request, Operation::Inspect, &[]).unwrap();
    assert!(!before.requires_elevation);
    assert!(
        before.owned.is_empty(),
        "Reserved fixture ports already have managed rules; do not touch them."
    );
    fixture("create", &nonce, request).unwrap();
    let outcome = (|| -> Result<(), String> {
        // Exercise all four exact rule shapes without altering default policies/profiles.
        let added = query(request, Operation::Configure, &[0, 1, 2, 3])?;
        for check in request.checks() {
            assert!(added.owned.iter().any(|r| owned(r, &check)));
        }
        fixture("verify", &nonce, request)?;
        fixture("modify", &nonce, request)?;
        let removed = execute(request, Operation::Remove)?;
        assert!(removed.checks.iter().all(|c| !c.owned_rule_present));
        let raw = query(request, Operation::Inspect, &[])?;
        assert_eq!(
            raw.owned.len(),
            1,
            "Only the deliberately modified rule must remain."
        );
        assert_eq!(raw.owned[0].name, request.checks()[1].id());
        assert_eq!(raw.owned[0].description, format!("FXI CI modified {nonce}"));
        fixture("verify", &nonce, request)?;
        Ok(())
    })();
    // Never flush/reset profiles; cleanup checks each fixture name and description again.
    let cleanup = fixture("cleanup", &nonce, request);
    outcome.unwrap();
    cleanup.unwrap();
}
