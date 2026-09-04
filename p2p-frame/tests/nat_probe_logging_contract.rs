const SCHEDULER_SOURCE: &str = include_str!("../src/sn/service/nat_probe_scheduler.rs");
const SERVICE_SOURCE: &str = include_str!("../src/sn/service/service.rs");
const CLIENT_SOURCE: &str = include_str!("../src/sn/client/sn_service.rs");

fn production_sources() -> [&'static str; 3] {
    [SCHEDULER_SOURCE, SERVICE_SOURCE, CLIENT_SOURCE]
}

#[test]
fn nat_probe_log_events_cover_the_operational_lifecycle() {
    let source = production_sources().concat();
    for event in [
        "nat_probe_config_changed",
        "nat_probe_authority_established",
        "nat_probe_trigger_queued",
        "nat_probe_directive_issued",
        "nat_probe_directive_suppressed",
        "nat_probe_client_started",
        "nat_probe_client_completed",
        "nat_probe_client_failed",
        "nat_probe_result_reported",
        "nat_probe_result_report_failed",
        "nat_probe_result_accepted",
        "nat_probe_result_rejected",
        "nat_probe_directive_timeout",
        "nat_probe_profile_invalidated",
        "nat_probe_authority_removed",
    ] {
        assert!(
            source.contains(&format!("event={event}")),
            "missing event={event}"
        );
    }
}

#[test]
fn nat_probe_log_statements_do_not_reference_secret_or_raw_payload_fields() {
    let forbidden = [
        "certificate=",
        "client_cert=",
        "private_key=",
        "secret=",
        "token=",
        "payload=",
        "packet_body=",
        "raw_bytes=",
    ];
    for source in production_sources() {
        for line in source
            .lines()
            .filter(|line| line.contains("event=nat_probe_"))
        {
            for field in forbidden {
                assert!(!line.contains(field), "forbidden field {field} in {line}");
            }
        }
    }
}

#[test]
fn nat_probe_info_and_warn_events_use_counts_instead_of_endpoint_values() {
    for source in production_sources() {
        let compact: String = source.split_whitespace().collect::<Vec<_>>().join(" ");
        for marker in ["log::info!(", "log::warn!("] {
            for block in compact.split(marker).skip(1) {
                let block = block.split(");").next().unwrap_or(block);
                if !block.contains("event=nat_probe_") {
                    continue;
                }
                assert!(
                    !block.contains("endpoints={:?}"),
                    "endpoint list in {marker}{block}"
                );
                assert!(
                    !block.contains("remote_endpoint="),
                    "remote endpoint in {marker}{block}"
                );
            }
        }
    }
}
