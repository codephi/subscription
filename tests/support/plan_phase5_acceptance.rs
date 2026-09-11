use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn active_plan_slot_tracks_commercial_and_renewal_status() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let (_, pool, repository, workspace_id, product_id) = setup_active_workspace().await;
    let offer = create_free_plan(&repository, product_id, PlanRecurrence::Monthly, 20).await;
    let current = join_plan(
        &repository,
        workspace_id,
        offer.plan_version_id,
        "slot-status",
    )
    .await;
    let mut migration = pool.begin().await.unwrap();
    sqlx::raw_sql(include_str!(
        "../../migrations/202609110004_active_plan_slot_lifecycle.down.sql"
    ))
    .execute(&mut *migration)
    .await
    .unwrap();
    sqlx::raw_sql(include_str!(
        "../../migrations/202609110004_active_plan_slot_lifecycle.up.sql"
    ))
    .execute(&mut *migration)
    .await
    .unwrap();
    migration.commit().await.unwrap();
    sqlx::query("UPDATE customer_plans SET commercial_status='PAST_DUE',renewal_status='RENEWAL_INACTIVE' WHERE customer_plan_id=$1")
        .bind(current.customer_plan_id).execute(&pool).await.unwrap();
    assert_plan_state(&pool, workspace_id, 0, 1, 1, 20).await;
    let replacement = join_plan(
        &repository,
        workspace_id,
        offer.plan_version_id,
        "slot-replacement",
    )
    .await;
    assert_plan_state(&pool, workspace_id, 1, 2, 2, 40).await;
    assert!(sqlx::query("UPDATE customer_plans SET commercial_status='ACTIVE',renewal_status='CURRENT' WHERE customer_plan_id=$1")
        .bind(current.customer_plan_id).execute(&pool).await.is_err());
    let preserved: (String, String) = sqlx::query_as(
        "SELECT commercial_status,renewal_status FROM customer_plans WHERE customer_plan_id=$1",
    )
    .bind(current.customer_plan_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(preserved, ("PAST_DUE".into(), "RENEWAL_INACTIVE".into()));
    revoke_for_cleanup(&repository, workspace_id, replacement.customer_plan_id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn published_plan_is_fully_immutable_except_for_revocation() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let (_, pool, repository, _, product_id) = setup_active_workspace().await;
    let offer = create_free_plan(&repository, product_id, PlanRecurrence::Monthly, 20).await;
    let other_product_id = create_metered_product(&repository).await;
    for query in [
        "UPDATE subscription_plan_versions SET name='changed' WHERE plan_version_id=$1",
        "UPDATE subscription_plan_versions SET commercial_model='PAID' WHERE plan_version_id=$1",
        "UPDATE subscription_plan_versions SET price_amount_minor=100 WHERE plan_version_id=$1",
        "UPDATE subscription_plan_versions SET currency='BRL' WHERE plan_version_id=$1",
        "UPDATE subscription_plan_versions SET recurrence='WEEKLY' WHERE plan_version_id=$1",
        "UPDATE subscription_plan_versions SET admission_policy='APPROVAL_REQUIRED' WHERE plan_version_id=$1",
        "UPDATE subscription_plan_versions SET accepted_payment_methods=ARRAY['CARD'] WHERE plan_version_id=$1",
        "UPDATE subscription_plan_versions SET granted_credit_units=99 WHERE plan_version_id=$1",
        "UPDATE subscription_plan_versions SET subscription_id=gen_random_uuid() WHERE plan_version_id=$1",
        "UPDATE subscription_plan_versions SET published_at=published_at+interval '1 day' WHERE plan_version_id=$1"
    ] {
        assert!(sqlx::query(query).bind(offer.plan_version_id).execute(&pool).await.is_err(), "query {query} must fail");
    }
    assert!(
        sqlx::query("DELETE FROM subscription_plan_products WHERE plan_version_id=$1")
            .bind(offer.plan_version_id)
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(sqlx::query(
        "INSERT INTO subscription_plan_products(plan_version_id,product_id) VALUES($1,$2)"
    )
    .bind(offer.plan_version_id)
    .bind(other_product_id)
    .execute(&pool)
    .await
    .is_err());
    let second = plans::create_plan(
        &repository,
        offer.subscription_id,
        plan_request(
            product_id,
            CommercialModel::Free,
            PlanRecurrence::Weekly,
            30,
        ),
    )
    .await
    .unwrap();
    assert_ne!(second.plan_version_id, offer.plan_version_id);
    assert_eq!(
        plans::get_plan(&repository, offer.plan_version_id)
            .await
            .unwrap()
            .granted_credit_units
            .value(),
        20
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plan_and_customer_revocation_have_distinct_idempotent_effects() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let (_, pool, repository, workspace_id, product_id) = setup_active_workspace().await;
    let offer = create_free_plan(&repository, product_id, PlanRecurrence::Monthly, 20).await;
    let current = join_plan(
        &repository,
        workspace_id,
        offer.plan_version_id,
        "offer-revoke",
    )
    .await;
    let end = current
        .current_cycle
        .clone()
        .unwrap()
        .current_period_end
        .unwrap();
    let withdrawn_target =
        create_free_plan(&repository, product_id, PlanRecurrence::Monthly, 10).await;
    plans::revoke_plan(
        &repository,
        withdrawn_target.plan_version_id,
        revoke_plan_request(),
    )
    .await
    .unwrap();
    let rejected_transition = plans::transition_customer_plan(
        &repository,
        workspace_id,
        current.customer_plan_id,
        "withdrawn-target",
        subscription::dto::plans::CreatePlanTransitionRequest {
            new_plan_version_id: withdrawn_target.plan_version_id,
            transition_kind: subscription::dto::plans::PlanTransitionKind::Downgrade,
            transaction_id: "withdrawn-target".into(),
            actor_reference: "customer:test".into(),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(rejected_transition.code(), "subscription_plan_revoked");
    let rejected_effects: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM customer_plan_transitions), (SELECT count(*) FROM collection_requests)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(rejected_effects, (0, 0));
    plans::revoke_plan(&repository, offer.plan_version_id, revoke_plan_request())
        .await
        .unwrap();
    let retained = plans::get_customer_plan(&repository, workspace_id, current.customer_plan_id)
        .await
        .unwrap();
    assert_eq!(retained.commercial_status, "ACTIVE");
    let retained_balance: i64 = sqlx::query_scalar("SELECT balance_credit_units FROM customer_wallets w JOIN wallets r ON r.wallet_id=w.wallet_id WHERE r.customer_id=$1")
        .bind(workspace_id).fetch_one(&pool).await.unwrap();
    assert_eq!(retained_balance, 20);
    assert_eq!(
        plans::run_due_cycles(&repository, end)
            .await
            .unwrap()
            .canceled_customer_plans,
        1
    );
    let expired = plans::get_customer_plan(&repository, workspace_id, current.customer_plan_id)
        .await
        .unwrap();
    assert_eq!(expired.commercial_status, "EXPIRED");
    assert_eq!(expired.end_reason.as_deref(), Some("PLAN_REVOKED"));
    assert_eq!(
        plans::run_due_cycles(&repository, end)
            .await
            .unwrap()
            .processed_customer_plans,
        0
    );
    assert_plan_state(&pool, workspace_id, 0, 1, 2, 0).await;
    let other = create_free_plan(&repository, product_id, PlanRecurrence::Monthly, 10).await;
    let individual = join_plan(
        &repository,
        workspace_id,
        other.plan_version_id,
        "individual-revoke",
    )
    .await;
    let revoked = plans::revoke_customer_plan(
        &repository,
        workspace_id,
        individual.customer_plan_id,
        RevokeCustomerPlanRequest {
            reason: "administrative".into(),
            actor_reference: "operator:test".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(revoked.commercial_status, "REVOKED");
    assert_eq!(revoked.end_reason.as_deref(), Some("ADMIN_REVOKED"));
    let repeated = plans::revoke_customer_plan(
        &repository,
        workspace_id,
        individual.customer_plan_id,
        RevokeCustomerPlanRequest {
            reason: "ignored retry".into(),
            actor_reference: "operator:retry".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        serde_json::to_value(revoked).unwrap(),
        serde_json::to_value(repeated).unwrap()
    );
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM audit_events WHERE resource_id=$1 AND action='customer_plan.revoked'")
        .bind(individual.customer_plan_id).fetch_one(&pool).await.unwrap(),1);
}
