use super::support::*;

#[tokio::test]
async fn the_harness_opens_a_conversation_and_answers_a_message() {
    let f = fixture().await;
    let run = f.open().await;
    assert_eq!(f.supervisor.seeds().len(), 1);
    let r = f
        .host
        .send_message(f.ws, "Hello".to_owned(), vec![])
        .await
        .unwrap();
    assert_eq!(r.position_in_queue, 0);
    let sup = f.supervisor.clone();
    assert!(eventually(|| sup.turn_texts().iter().any(|t| t.ends_with("Hello"))).await);
    assert_eq!(f.host.current_run(f.ws), Some(run));
}
