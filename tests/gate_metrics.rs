use t4e::gates::{
    AttemptResult, FailureClassification, ToolGateResult, compute_gate_summary,
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

    let summary = compute_gate_summary(&sample, 0.6);
    assert_eq!(summary.first_attempt_successes, 1);
    assert_eq!(summary.successful_tools, 2);
    assert_eq!(summary.tools_with_retry, 1);
    assert_eq!(summary.attempts_total, 4);
    assert_eq!(summary.infra_failures, 1);
    assert_eq!(summary.product_failures, 1);
    assert_eq!(summary.status, "pass");
}
