use t4e::gates::{
    AttemptResult, FailureClassification, ToolGateResult, compute_gate_summary, validate_gate_input,
};

#[test]
fn gate_summary_respects_retry_and_classification_rules() {
    let sample = vec![
        ToolGateResult {
            tool_id: "jq".to_string(),
            attempts: vec![AttemptResult {
                exit_code: 0,
                classification: FailureClassification::None,
            }],
        },
        ToolGateResult {
            tool_id: "ripgrep".to_string(),
            attempts: vec![
                AttemptResult {
                    exit_code: 1,
                    classification: FailureClassification::Infra,
                },
                AttemptResult {
                    exit_code: 0,
                    classification: FailureClassification::None,
                },
            ],
        },
        ToolGateResult {
            tool_id: "yt-dlp".to_string(),
            attempts: vec![AttemptResult {
                exit_code: 1,
                classification: FailureClassification::Product,
            }],
        },
    ];

    validate_gate_input(&sample).expect("sample is valid");
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
        attempts: vec![AttemptResult {
            exit_code: 0,
            classification: FailureClassification::None,
        }],
    }];
    assert!(validate_gate_input(&sample).is_err());
}
