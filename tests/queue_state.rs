use t4e::installer::queue::{QueueItem, QueueState, QueueTransitionError};

#[test]
fn queue_allows_documented_transitions() {
    let mut item = QueueItem::new("ripgrep", "brew");
    item.transition(QueueState::Installing)
        .expect("queued -> installing");
    item.transition(QueueState::Failed)
        .expect("installing -> failed");
    item.transition(QueueState::Queued)
        .expect("failed -> queued retry");
    item.transition(QueueState::Installing)
        .expect("queued -> installing");
    item.transition(QueueState::Success)
        .expect("installing -> success");
}

#[test]
fn queue_rejects_invalid_jump() {
    let mut item = QueueItem::new("ripgrep", "brew");
    let err = item
        .transition(QueueState::Success)
        .expect_err("queued -> success should fail");

    assert_eq!(
        err,
        QueueTransitionError::Invalid {
            from: QueueState::Queued,
            to: QueueState::Success,
        }
    );
}
