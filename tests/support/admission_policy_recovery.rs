use super::admission_policy_contract::evidence;
use super::*;
use subscription::{
    dto::admission::{AdmissionFact, CreateAdmissionPolicyRequest},
    services::admission,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn admission_withdrawal_serializes_with_join_and_failed_commit_leaves_no_decision() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let (_, pool, repository, workspace_id, product_id) = setup_active_workspace().await;
    let policy = admission::create_policy(
        &repository,
        CreateAdmissionPolicyRequest {
            policy_id: Uuid::new_v4(),
            version: 1,
            required_facts: vec![AdmissionFact::EmailVerified],
        },
    )
    .await
    .unwrap();
    let subscription = plans::create_subscription(&repository, subscription_request())
        .await
        .unwrap();
    let mut offer = plan_request(
        product_id,
        CommercialModel::Free,
        PlanRecurrence::Monthly,
        60,
    );
    offer.admission_policy = AdmissionPolicy::ApprovalRequired;
    offer.admission_policy_version_id = Some(policy.policy_version_id);
    let plan = plans::create_plan(&repository, subscription.subscription_id, offer)
        .await
        .unwrap();
    admission::receive_evidence(
        &repository,
        evidence(workspace_id, policy.policy_version_id, 1),
    )
    .await
    .unwrap();
    let mut withdrawal = pool.begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *withdrawal)
        .await
        .unwrap();
    sqlx::query("SELECT workspace_id FROM workspace_projections WHERE workspace_id=$1 FOR UPDATE")
        .bind(workspace_id)
        .fetch_one(&mut *withdrawal)
        .await
        .unwrap();
    sqlx::query("INSERT INTO subscription_admission_evidence(event_id,workspace_id,policy_version_id,sequence,verified_facts,evidence_reference,valid_until,request_hash) VALUES ($1,$2,$3,2,'{}','accounts:withdrawal',now()+interval '1 hour','test-withdrawal')")
        .bind(Uuid::new_v4()).bind(workspace_id).bind(policy.policy_version_id).execute(&mut *withdrawal).await.unwrap();
    let worker_repository = repository.clone();
    let request = customer_plan_request(plan.plan_version_id, "withdrawal-join");
    let worker_request = request.clone();
    let worker = tokio::spawn(async move {
        plans::create_customer_plan(
            &worker_repository,
            workspace_id,
            "withdrawal-join",
            worker_request,
        )
        .await
    });
    let blocked = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE $1=ANY(pg_blocking_pids(pid)))",
            )
            .bind(pid)
            .fetch_one(&pool)
            .await
            .unwrap();
            if waiting {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await;
    withdrawal.commit().await.unwrap();
    blocked.expect("join waits for evidence serialization");
    assert_eq!(
        worker.await.unwrap().unwrap_err().code(),
        "customer_plan_approval_required"
    );
    assert_plan_state(&pool, workspace_id, 0, 0, 0, 0).await;
    admission::receive_evidence(
        &repository,
        evidence(workspace_id, policy.policy_version_id, 3),
    )
    .await
    .unwrap();
    sqlx::raw_sql("CREATE FUNCTION fail_admission_commit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'injected admission commit failure'; END $$; CREATE CONSTRAINT TRIGGER fail_admission_commit AFTER INSERT ON subscription_admission_decisions DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION fail_admission_commit();")
        .execute(&pool).await.unwrap();
    assert!(plans::create_customer_plan(
        &repository,
        workspace_id,
        "withdrawal-join",
        request.clone()
    )
    .await
    .is_err());
    assert_plan_state(&pool, workspace_id, 0, 0, 0, 0).await;
    let decisions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM subscription_admission_decisions")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(decisions, 0);
    sqlx::raw_sql("DROP TRIGGER fail_admission_commit ON subscription_admission_decisions; DROP FUNCTION fail_admission_commit();")
        .execute(&pool).await.unwrap();
    plans::create_customer_plan(&repository, workspace_id, "withdrawal-join", request)
        .await
        .unwrap();
    assert_plan_state(&pool, workspace_id, 1, 1, 1, 60).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admission_migration_round_trips_existing_open_contracts() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let (_, pool, repository, workspace_id, product_id) = setup_active_workspace().await;
    let offer = create_free_plan(&repository, product_id, PlanRecurrence::Monthly, 30).await;
    let current = join_plan(
        &repository,
        workspace_id,
        offer.plan_version_id,
        "migration-admission",
    )
    .await;
    let mut migration = pool.begin().await.unwrap();
    sqlx::raw_sql(include_str!(
        "../../migrations/202609110003_admission_policies.down.sql"
    ))
    .execute(&mut *migration)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../../migrations/202609110003_admission_policies.up.sql"
    ))
    .execute(&mut *migration)
    .await
    .unwrap();
    migration.commit().await.unwrap();
    let restored = plans::get_customer_plan(&repository, workspace_id, current.customer_plan_id)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(current).unwrap(),
        serde_json::to_value(restored).unwrap()
    );
    assert_eq!(
        plans::get_plan(&repository, offer.plan_version_id)
            .await
            .unwrap()
            .admission_policy_version_id,
        None
    );
    assert_plan_state(&pool, workspace_id, 1, 1, 1, 30).await;
}
