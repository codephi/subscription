use super::*;
use subscription::services::subscription_calendar::{dispatch_once, CalendarDispatchOutcome};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn durable_calendar_workers_grant_and_expire_each_cycle_once() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let (_, pool, repository, account_id, product_id) = setup_active_account().await;
    let offer = create_free_plan(&repository, product_id, PlanRecurrence::Monthly, 30).await;
    let current = join_plan(&repository, account_id, offer.plan_version_id, "jobs").await;
    let cycle = current.current_cycle.unwrap();
    let boundary = cycle.current_period_end.unwrap();
    assert_eq!(
        dispatch_once(&repository, boundary - Duration::seconds(1))
            .await
            .unwrap(),
        CalendarDispatchOutcome::Idle
    );
    let (first, second) = tokio::join!(
        dispatch_once(&repository, boundary),
        dispatch_once(&repository, boundary)
    );
    let outcomes = [first.unwrap(), second.unwrap()];
    assert_eq!(
        outcomes
            .iter()
            .filter(|value| **value == CalendarDispatchOutcome::Advanced)
            .count(),
        1
    );
    assert_plan_state(&pool, account_id, 1, 2, 3, 30).await;
    let jobs: (i64, i64) =
        sqlx::query_as("SELECT count(*),count(completed_at) FROM subscription_calendar_jobs")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(jobs, (2, 1));
    assert_eq!(
        dispatch_once(&repository, boundary).await.unwrap(),
        CalendarDispatchOutcome::Idle
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn durable_calendar_reclaims_lost_worker_and_backfills_existing_cycles() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let (_, pool, repository, account_id, product_id) = setup_active_account().await;
    let offer = create_free_plan(&repository, product_id, PlanRecurrence::Monthly, 30).await;
    let current = join_plan(
        &repository,
        account_id,
        offer.plan_version_id,
        "recover-job",
    )
    .await;
    let cycle = current.current_cycle.unwrap();
    let boundary = cycle.current_period_end.unwrap();
    let mut migration = pool.begin().await.unwrap();
    sqlx::raw_sql(include_str!(
        "../../migrations/202609110002_subscription_calendar_jobs.down.sql"
    ))
    .execute(&mut *migration)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../../migrations/202609110002_subscription_calendar_jobs.up.sql"
    ))
    .execute(&mut *migration)
    .await
    .unwrap();
    migration.commit().await.unwrap();
    sqlx::query("UPDATE subscription_calendar_jobs SET lease_token=$1,lease_expires_at=clock_timestamp()+interval '1 hour',attempts=1")
        .bind(Uuid::new_v4()).execute(&pool).await.unwrap();
    assert_eq!(
        dispatch_once(&repository, boundary).await.unwrap(),
        CalendarDispatchOutcome::Idle
    );
    sqlx::query("UPDATE subscription_calendar_jobs SET lease_expires_at=clock_timestamp()-interval '1 second'")
        .execute(&pool).await.unwrap();
    let restarted = DatabaseRepository::new(pool.clone());
    assert_eq!(
        dispatch_once(&restarted, boundary).await.unwrap(),
        CalendarDispatchOutcome::Advanced
    );
    let attempts: i64 = sqlx::query_scalar(
        "SELECT attempts FROM subscription_calendar_jobs WHERE completed_at IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(attempts, 2);
    assert_plan_state(&pool, account_id, 1, 2, 3, 30).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn blocked_calendar_job_is_deferred_without_starving_another_account() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let (_, pool, repository, blocked_id, product_id) = setup_active_account().await;
    let offer = create_free_plan(&repository, product_id, PlanRecurrence::Monthly, 30).await;
    let blocked = join_plan(
        &repository,
        blocked_id,
        offer.plan_version_id,
        "blocked-job",
    )
    .await;
    let other_id = Uuid::new_v4();
    apply_account_event(&repository, other_id, "account.created", 1).await;
    apply_account_event(&repository, other_id, "account.activated", 2).await;
    let other = join_plan(&repository, other_id, offer.plan_version_id, "other-job").await;
    let cutoff = other.current_cycle.unwrap().current_period_end.unwrap();
    apply_account_event(&repository, blocked_id, "account.blocked", 3).await;
    assert_eq!(
        dispatch_once(&repository, cutoff).await.unwrap(),
        CalendarDispatchOutcome::Deferred
    );
    assert_eq!(
        dispatch_once(&repository, cutoff).await.unwrap(),
        CalendarDispatchOutcome::Advanced
    );
    assert_eq!(
        dispatch_once(&repository, cutoff).await.unwrap(),
        CalendarDispatchOutcome::Idle
    );
    assert_plan_state(&pool, blocked_id, 1, 1, 1, 30).await;
    assert_plan_state(&pool, other_id, 1, 2, 3, 30).await;
    let cycle_id = blocked.current_cycle.unwrap().customer_plan_cycle_id;
    let error: String = sqlx::query_scalar(
        "SELECT last_error_code FROM subscription_calendar_jobs WHERE customer_plan_cycle_id=$1",
    )
    .bind(cycle_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(error, "account_not_operational");
    apply_account_event(&repository, blocked_id, "account.activated", 4).await;
    sqlx::query("UPDATE subscription_calendar_jobs SET retry_at='-infinity' WHERE customer_plan_cycle_id=$1")
        .bind(cycle_id).execute(&pool).await.unwrap();
    assert_eq!(
        dispatch_once(&repository, cutoff).await.unwrap(),
        CalendarDispatchOutcome::Advanced
    );
    assert_plan_state(&pool, blocked_id, 1, 2, 3, 30).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn calendar_commit_failure_preserves_job_and_rolls_back_all_cycle_effects() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let (_, pool, repository, account_id, product_id) = setup_active_account().await;
    let offer = create_free_plan(&repository, product_id, PlanRecurrence::Monthly, 30).await;
    let current = join_plan(&repository, account_id, offer.plan_version_id, "failed-job").await;
    let boundary = current.current_cycle.unwrap().current_period_end.unwrap();
    sqlx::raw_sql("CREATE FUNCTION fail_calendar_commit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.event_type='customer_plan.cycle_started' THEN RAISE EXCEPTION 'injected calendar failure'; END IF; RETURN NEW; END $$; CREATE CONSTRAINT TRIGGER fail_calendar_commit AFTER INSERT ON outbox_events DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION fail_calendar_commit();")
        .execute(&pool).await.unwrap();
    assert_eq!(
        dispatch_once(&repository, boundary).await.unwrap(),
        CalendarDispatchOutcome::Deferred
    );
    assert_plan_state(&pool, account_id, 1, 1, 1, 30).await;
    let jobs: (i64, i64) =
        sqlx::query_as("SELECT count(*),count(completed_at) FROM subscription_calendar_jobs")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(jobs, (1, 0));
    sqlx::raw_sql("DROP TRIGGER fail_calendar_commit ON outbox_events; DROP FUNCTION fail_calendar_commit(); UPDATE subscription_calendar_jobs SET retry_at='-infinity';")
        .execute(&pool).await.unwrap();
    assert_eq!(
        dispatch_once(&repository, boundary).await.unwrap(),
        CalendarDispatchOutcome::Advanced
    );
    assert_plan_state(&pool, account_id, 1, 2, 3, 30).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn calendar_background_worker_runs_only_materialized_recurring_free_cycles() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let (_, pool, repository, account_id, product_id) = setup_active_account().await;
    let lifetime = create_free_plan(&repository, product_id, PlanRecurrence::None, 0).await;
    join_plan(
        &repository,
        account_id,
        lifetime.plan_version_id,
        "lifetime-job",
    )
    .await;
    let subscription = plans::create_subscription(&repository, subscription_request())
        .await
        .unwrap();
    let paid = plans::create_plan(
        &repository,
        subscription.subscription_id,
        paid_plan_request(product_id),
    )
    .await
    .unwrap();
    join_plan(&repository, account_id, paid.plan_version_id, "paid-job").await;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM subscription_calendar_jobs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let offer = create_free_plan(&repository, product_id, PlanRecurrence::Weekly, 30).await;
    let current = join_plan(
        &repository,
        account_id,
        offer.plan_version_id,
        "background-job",
    )
    .await;
    let cycle_id = current.current_cycle.unwrap().customer_plan_cycle_id;
    let anchor = Utc::now() - Duration::days(8);
    sqlx::query("UPDATE customer_plans SET anchor_at=$2 WHERE customer_plan_id=$1")
        .bind(current.customer_plan_id)
        .bind(anchor)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE customer_plan_cycles SET current_period_start=$2,current_period_end=$2+interval '7 days' WHERE customer_plan_cycle_id=$1")
        .bind(cycle_id).bind(anchor).execute(&pool).await.unwrap();
    sqlx::query(
        "UPDATE credit_lots SET expires_at=$1::timestamptz+interval '7 days' WHERE source_kind='SUBSCRIPTION'",
    )
    .bind(anchor)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE subscription_calendar_jobs SET due_at=$1::timestamptz+interval '7 days'")
        .bind(anchor)
        .execute(&pool)
        .await
        .unwrap();
    let worker = tokio::spawn(
        subscription::services::subscription_calendar::run_scheduler(repository.clone()),
    );
    let finished = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let completed: bool = sqlx::query_scalar("SELECT completed_at IS NOT NULL FROM subscription_calendar_jobs WHERE customer_plan_cycle_id=$1")
                .bind(cycle_id).fetch_one(&pool).await.unwrap();
            if completed { break; }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }).await;
    worker.abort();
    let _ = worker.await;
    finished.expect("background worker completes persisted overdue cycle");
    assert_plan_state(&pool, account_id, 3, 3, 3, 30).await;
}
