use super::*;
use subscription::services::{calendar::cycle_end, credits};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn downgrade_renewals_use_new_anchor_and_preserve_reclassified_credit() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let (_, pool, repository, account_id, product_id) = setup_active_account().await;
    let source = create_free_plan(&repository, product_id, PlanRecurrence::Monthly, 60).await;
    let current = join_plan(&repository, account_id, source.plan_version_id, "anchor").await;
    let target = plans::create_plan(
        &repository,
        source.subscription_id,
        plan_request(
            product_id,
            CommercialModel::Free,
            PlanRecurrence::Monthly,
            10,
        ),
    )
    .await
    .unwrap();
    assert_downgrade(
        &repository,
        &pool,
        account_id,
        current.customer_plan_id,
        target.plan_version_id,
    )
    .await;
    let changed = plans::get_customer_plan(&repository, account_id, current.customer_plan_id)
        .await
        .unwrap();
    let mut migration = pool.begin().await.unwrap();
    sqlx::raw_sql(include_str!(
        "../../migrations/202609110001_plan_calendar_anchor.down.sql"
    ))
    .execute(&mut *migration)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../../migrations/202609110001_plan_calendar_anchor.up.sql"
    ))
    .execute(&mut *migration)
    .await
    .unwrap();
    let restored: i64 = sqlx::query_scalar(
        "SELECT anchor_cycle_ordinal FROM customer_plans WHERE customer_plan_id=$1",
    )
    .bind(current.customer_plan_id)
    .fetch_one(&mut *migration)
    .await
    .unwrap();
    assert_eq!(restored, 2);
    migration.commit().await.unwrap();
    let anchor = changed.anchor_at;
    for ordinal in 1..=3 {
        let boundary = cycle_end(anchor, PlanRecurrence::Monthly, ordinal)
            .unwrap()
            .unwrap();
        let outcome = plans::run_due_cycles(&repository, boundary).await.unwrap();
        assert_eq!(outcome.created_cycles, 1);
        let renewed = plans::get_customer_plan(&repository, account_id, current.customer_plan_id)
            .await
            .unwrap();
        let cycle = renewed.current_cycle.unwrap();
        assert_eq!(cycle.current_period_start, boundary);
        assert_eq!(
            cycle.current_period_end,
            cycle_end(anchor, PlanRecurrence::Monthly, ordinal + 1).unwrap()
        );
        assert_eq!(cycle.cycle_ordinal, ordinal + 2);
        let balance: i64 = sqlx::query_scalar("SELECT balance_credit_units FROM customer_wallets w JOIN wallets r ON r.wallet_id=w.wallet_id WHERE r.customer_id=$1")
            .bind(account_id).fetch_one(&pool).await.unwrap();
        assert_eq!(balance, 70);
        assert!(
            credits::reconcile(&repository, account_id)
                .await
                .unwrap()
                .consistent
        );
        assert_eq!(
            plans::run_due_cycles(&repository, boundary)
                .await
                .unwrap()
                .created_cycles,
            0
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn renewal_observes_revocation_committed_while_waiting_for_plan_lock() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let (_, pool, repository, account_id, product_id) = setup_active_account().await;
    let offer = create_free_plan(&repository, product_id, PlanRecurrence::Monthly, 60).await;
    let current = join_plan(
        &repository,
        account_id,
        offer.plan_version_id,
        "revocation-race",
    )
    .await;
    let boundary = current.current_cycle.unwrap().current_period_end.unwrap();
    let mut revocation = pool.begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *revocation)
        .await
        .unwrap();
    sqlx::query("UPDATE subscription_plan_versions SET revoked_at=now(),revocation_reason='withdrawn' WHERE plan_version_id=$1")
        .bind(offer.plan_version_id).execute(&mut *revocation).await.unwrap();
    let worker_repository = repository.clone();
    let worker =
        tokio::spawn(async move { plans::run_due_cycles(&worker_repository, boundary).await });
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let blocked: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))",
            )
            .bind(pid)
            .fetch_one(&pool)
            .await
            .unwrap();
            if blocked {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("renewal waits for revocation lock");
    revocation.commit().await.unwrap();
    let outcome = worker.await.unwrap().unwrap();
    assert_eq!(outcome.created_cycles, 0);
    assert_eq!(outcome.canceled_customer_plans, 1);
    let expired = plans::get_customer_plan(&repository, account_id, current.customer_plan_id)
        .await
        .unwrap();
    assert_eq!(expired.commercial_status, "EXPIRED");
    assert_eq!(expired.end_reason.as_deref(), Some("PLAN_REVOKED"));
    assert_plan_state(&pool, account_id, 0, 1, 2, 0).await;
    assert_eq!(
        plans::run_due_cycles(&repository, boundary)
            .await
            .unwrap()
            .processed_customer_plans,
        0
    );
}
