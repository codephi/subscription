use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_pending_recurring_plan_releases_slot_once_without_financial_effects() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let (_, pool, repository, account_id, product_id) = setup_active_account().await;
    let subscription = plans::create_subscription(&repository, subscription_request())
        .await
        .unwrap();
    for paid in [false, true] {
        let mut request = plan_request(
            product_id,
            CommercialModel::Free,
            PlanRecurrence::Monthly,
            10,
        );
        request.accepted_payment_methods = vec!["CARD".to_string()];
        if paid {
            request = paid_plan_request(product_id);
        }
        let offer = plans::create_plan(&repository, subscription.subscription_id, request)
            .await
            .unwrap();
        let pending = join_plan(
            &repository,
            account_id,
            offer.plan_version_id,
            if paid { "paid-cancel" } else { "card-cancel" },
        )
        .await;
        let canceled =
            plans::cancel_customer_plan(&repository, account_id, pending.customer_plan_id)
                .await
                .unwrap();
        assert_eq!(canceled.commercial_status, "CANCELED");
        assert_eq!(canceled.end_reason.as_deref(), Some("CUSTOMER_CANCELED"));
        assert!(canceled.ended_at.is_some());
        assert!(!canceled.cancel_at_period_end);
        assert_plan_state(&pool, account_id, 0, 0, 0, 0).await;
        let repeated =
            plans::cancel_customer_plan(&repository, account_id, pending.customer_plan_id)
                .await
                .unwrap();
        assert_eq!(
            serde_json::to_value(&canceled).unwrap(),
            serde_json::to_value(repeated).unwrap()
        );
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM outbox_events WHERE aggregate_id=$1 AND event_type='customer_plan.canceled'")
            .bind(pending.customer_plan_id).fetch_one(&pool).await.unwrap();
        assert_eq!(count, 1);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scheduled_cancellation_is_idempotent_and_keeps_current_entitlement() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let (_, pool, repository, account_id, product_id) = setup_active_account().await;
    let offer = create_free_plan(&repository, product_id, PlanRecurrence::Monthly, 20).await;
    let current = join_plan(
        &repository,
        account_id,
        offer.plan_version_id,
        "scheduled-cancel",
    )
    .await;
    let canceled = plans::cancel_customer_plan(&repository, account_id, current.customer_plan_id)
        .await
        .unwrap();
    let repeated = plans::cancel_customer_plan(&repository, account_id, current.customer_plan_id)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&canceled).unwrap(),
        serde_json::to_value(repeated).unwrap()
    );
    assert!(canceled.cancel_at_period_end);
    assert_eq!(canceled.commercial_status, "ACTIVE");
    assert!(
        subscription::services::usage::eligibility(&repository, account_id, product_id)
            .await
            .unwrap()
            .access_allowed
    );
    assert_plan_state(&pool, account_id, 0, 1, 1, 20).await;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM outbox_events WHERE aggregate_id=$1 AND event_type='customer_plan.cancellation_scheduled'")
        .bind(current.customer_plan_id).fetch_one(&pool).await.unwrap();
    assert_eq!(count, 1);
    let boundary = canceled.current_cycle.unwrap().current_period_end.unwrap();
    plans::run_due_cycles(&repository, boundary).await.unwrap();
    let terminal = plans::get_customer_plan(&repository, account_id, current.customer_plan_id)
        .await
        .unwrap();
    let retried = plans::cancel_customer_plan(&repository, account_id, current.customer_plan_id)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(terminal).unwrap(),
        serde_json::to_value(retried).unwrap()
    );
    assert_plan_state(&pool, account_id, 0, 1, 2, 0).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_preserves_admin_revocation_and_closes_nonrecurring_entitlements() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let (_, pool, repository, account_id, product_id) = setup_active_account().await;
    let offer = create_free_plan(&repository, product_id, PlanRecurrence::None, 20).await;
    let current = join_plan(
        &repository,
        account_id,
        offer.plan_version_id,
        "none-cancel",
    )
    .await;
    let canceled = plans::cancel_customer_plan(&repository, account_id, current.customer_plan_id)
        .await
        .unwrap();
    assert_eq!(canceled.commercial_status, "CANCELED");
    assert!(canceled.ended_at.is_some());
    assert!(canceled.current_cycle.is_none());
    let open: i64 = sqlx::query_scalar("SELECT count(*) FROM customer_plan_entitlements WHERE customer_plan_id=$1 AND effective_until IS NULL")
        .bind(current.customer_plan_id).fetch_one(&pool).await.unwrap();
    assert_eq!(open, 0);
    assert_plan_state(&pool, account_id, 0, 1, 1, 20).await;
    let replacement = join_plan(
        &repository,
        account_id,
        offer.plan_version_id,
        "replacement",
    )
    .await;
    revoke_for_cleanup(&repository, account_id, replacement.customer_plan_id).await;
    let revoked = plans::get_customer_plan(&repository, account_id, replacement.customer_plan_id)
        .await
        .unwrap();
    let retried =
        plans::cancel_customer_plan(&repository, account_id, replacement.customer_plan_id)
            .await
            .unwrap();
    assert_eq!(
        serde_json::to_value(revoked).unwrap(),
        serde_json::to_value(retried).unwrap()
    );
    assert_plan_state(&pool, account_id, 0, 2, 2, 40).await;
}
