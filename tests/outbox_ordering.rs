mod support;

use chrono::{Duration, Utc};
use subscription::repositories::database::DatabaseRepository;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn outbox_serializes_aggregate_through_lease_retry_dead_letter_and_replay() {
    let (_, pool) = support::setup_router_with_options(false, None).await;
    let repository = DatabaseRepository::new(pool);
    let aggregate = Uuid::new_v4();
    let first = insert_event(&repository, aggregate, 1).await;
    let second = insert_event(&repository, aggregate, 2).await;
    let independent = insert_event(&repository, Uuid::new_v4(), 1).await;
    let (left, right) = tokio::join!(
        repository.claim_outbox_event(Uuid::new_v4()),
        repository.claim_outbox_event(Uuid::new_v4())
    );
    let mut claimed = vec![
        left.unwrap().unwrap().event_id,
        right.unwrap().unwrap().event_id,
    ];
    claimed.sort();
    let mut expected = vec![first, independent];
    expected.sort();
    assert_eq!(claimed, expected);
    repository.mark_outbox_delivered(independent).await.unwrap();
    assert_no_claim(&repository).await;
    repository
        .mark_outbox_failed(first, 1, Utc::now() + Duration::hours(1), "retry")
        .await
        .unwrap();
    assert_no_claim(&repository).await;
    repository
        .mark_outbox_failed(first, 12, Utc::now(), "dead letter")
        .await
        .unwrap();
    assert_no_claim(&repository).await;
    repository.replay_dead_letter(first).await.unwrap();
    assert_eq!(
        repository
            .claim_outbox_event(Uuid::new_v4())
            .await
            .unwrap()
            .unwrap()
            .event_id,
        first
    );
    repository.mark_outbox_delivered(first).await.unwrap();
    assert_eq!(
        repository
            .claim_outbox_event(Uuid::new_v4())
            .await
            .unwrap()
            .unwrap()
            .event_id,
        second
    );
    repository.mark_outbox_delivered(second).await.unwrap();
    let delivered: i64 =
        sqlx::query_scalar("SELECT count(*) FROM outbox_events WHERE delivered_at IS NOT NULL")
            .fetch_one(&repository.pool())
            .await
            .unwrap();
    assert_eq!(delivered, 3);
}

async fn assert_no_claim(repository: &DatabaseRepository) {
    assert!(repository
        .claim_outbox_event(Uuid::new_v4())
        .await
        .unwrap()
        .is_none());
}

async fn insert_event(repository: &DatabaseRepository, aggregate: Uuid, sequence: i64) -> Uuid {
    let event = Uuid::new_v4();
    sqlx::query("INSERT INTO outbox_events (event_id,event_type,aggregate_type,aggregate_id,aggregate_sequence,account_id,correlation_id,payload) VALUES ($1,'test.event','test',$2,$3,$2,$1,'{}')")
        .bind(event).bind(aggregate).bind(sequence).execute(&repository.pool()).await.unwrap();
    event
}
