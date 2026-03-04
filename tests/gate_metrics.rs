use t4e::gates::{
    AttemptResult, FailureClassification, ToolGateResult, build_gate_report, compute_gate_summary,
    validate_gate_input, CANONICAL_SAMPLE_TOOLS,
};

#[test]
fn gate_summary_respects_retry_and_classification_rules() {
    let sample = vec![
        ToolGateResult {
            tool_id: "jq".to_string(),
            manager: "brew".to_string(),
            attempt_count: 1,
            final_status: "success".to_string(),
            failure_classification: FailureClassification::None,
            attempts: vec![AttemptResult {
                attempt: 1,
                exit_code: 0,
                duration_ms: 1000,
                stderr_summary: String::new(),
                classification: FailureClassification::None,
            }],
        },
        ToolGateResult {
            tool_id: "ripgrep".to_string(),
            manager: "brew".to_string(),
            attempt_count: 2,
            final_status: "success".to_string(),
            failure_classification: FailureClassification::Infra,
            attempts: vec![
                AttemptResult {
                    attempt: 1,
                    exit_code: 1,
                    duration_ms: 1000,
                    stderr_summary: "timeout".to_string(),
                    classification: FailureClassification::Infra,
                },
                AttemptResult {
                    attempt: 2,
                    exit_code: 0,
                    duration_ms: 900,
                    stderr_summary: String::new(),
                    classification: FailureClassification::None,
                },
            ],
        },
        ToolGateResult {
            tool_id: "yt-dlp".to_string(),
            manager: "brew".to_string(),
            attempt_count: 1,
            final_status: "failed".to_string(),
            failure_classification: FailureClassification::Product,
            attempts: vec![AttemptResult {
                attempt: 1,
                exit_code: 1,
                duration_ms: 1200,
                stderr_summary: "failed".to_string(),
                classification: FailureClassification::Product,
            }],
        },
    ];

    let summary = compute_gate_summary(&sample, 0.2);
    assert_eq!(summary.first_attempt_successes, 1);
    assert_eq!(summary.successful_tools, 2);
    assert_eq!(summary.tools_with_retry, 1);
    assert_eq!(summary.attempts_total, 4);
    assert_eq!(summary.infra_failures, 1);
    assert_eq!(summary.product_failures, 1);
    assert_eq!(summary.success_rate, 0.2);
    assert_eq!(summary.status, "pass");
}

#[test]
fn gate_input_rejects_non_canonical_tools() {
    let sample = vec![ToolGateResult {
        tool_id: "unknown-tool".to_string(),
        manager: "brew".to_string(),
        attempt_count: 1,
        final_status: "success".to_string(),
        failure_classification: FailureClassification::None,
        attempts: vec![AttemptResult {
            attempt: 1,
            exit_code: 0,
            duration_ms: 1000,
            stderr_summary: String::new(),
            classification: FailureClassification::None,
        }],
    }];
    assert!(validate_gate_input(&sample).is_err());
}

#[test]
fn gate_report_contains_rich_tool_metadata_contract() {
    let sample = CANONICAL_SAMPLE_TOOLS
        .iter()
        .map(|tool| ToolGateResult {
            tool_id: (*tool).to_string(),
            manager: "brew".to_string(),
            attempt_count: 1,
            final_status: "success".to_string(),
            failure_classification: FailureClassification::None,
            attempts: vec![AttemptResult {
                attempt: 1,
                exit_code: 0,
                duration_ms: 123,
                stderr_summary: String::new(),
                classification: FailureClassification::None,
            }],
        })
        .collect::<Vec<_>>();

    let report = build_gate_report("gate2", "20260304T000000Z", "ubuntu-24.04", sample, 0.60)
        .expect("report builds");
    let as_json = serde_json::to_value(report).expect("serialize");
    let first = &as_json["tool_results"][0];
    assert!(first.get("manager").is_some());
    assert!(first.get("attempt_count").is_some());
    assert!(first.get("final_status").is_some());
    assert!(first.get("failure_classification").is_some());
    assert!(first["attempts"][0].get("duration_ms").is_some());
    assert!(first["attempts"][0].get("stderr_summary").is_some());
}

#[test]
fn gate_input_rejects_partial_canonical_set() {
    let sample = vec![ToolGateResult {
        tool_id: "jq".to_string(),
        manager: "brew".to_string(),
        attempt_count: 1,
        final_status: "success".to_string(),
        failure_classification: FailureClassification::None,
        attempts: vec![AttemptResult {
            attempt: 1,
            exit_code: 0,
            duration_ms: 123,
            stderr_summary: String::new(),
            classification: FailureClassification::None,
        }],
    }];
    assert!(validate_gate_input(&sample).is_err());
}
