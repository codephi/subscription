use super::*;
use subscription::dto::plans::{CreatePlanTransitionRequest, PlanTransitionKind};
use subscription::services::usage;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pending_or_revoked_subscription_does_not_hide_another_entitlement() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let (_, pool, repository, workspace_id, product_id) = setup_active_workspace().await;
    let offer = create_free_plan(&repository, product_id, PlanRecurrence::Monthly, 60).await;
    let active = join_plan(&repository, workspace_id, offer.plan_version_id, "eligible").await;
    sqlx::query(
        "UPDATE customer_plans SET renewal_status='RENEWAL_INACTIVE' WHERE customer_plan_id=$1",
    )
    .bind(active.customer_plan_id)
    .execute(&pool)
    .await
    .unwrap();
    let other_subscription = plans::create_subscription(&repository, subscription_request())
        .await
        .unwrap();
    let mut request = plan_request(
        product_id,
        CommercialModel::Free,
        PlanRecurrence::Monthly,
        20,
    );
    request.accepted_payment_methods = vec!["CARD".to_string()];
    let pending_offer =
        plans::create_plan(&repository, other_subscription.subscription_id, request)
            .await
            .unwrap();
    let pending = join_plan(
        &repository,
        workspace_id,
        pending_offer.plan_version_id,
        "pending",
    )
    .await;
    for revoke_pending in [false, true] {
        if revoke_pending {
            revoke_for_cleanup(&repository, workspace_id, pending.customer_plan_id).await;
        }
        let eligibility = usage::eligibility(&repository, workspace_id, product_id)
            .await
            .unwrap();
        assert!(eligibility.eligible);
        assert!(eligibility.entitled);
        assert_eq!(eligibility.commercial_status.as_deref(), Some("ACTIVE"));
        assert_eq!(
            eligibility.renewal_status.as_deref(),
            Some("RENEWAL_INACTIVE")
        );
        assert_eq!(eligibility.balance_credit_units.unwrap().value(), 60);
    }
    revoke_for_cleanup(&repository, workspace_id, active.customer_plan_id).await;
    assert!(
        !usage::eligibility(&repository, workspace_id, product_id)
            .await
            .unwrap()
            .eligible
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn downgrade_cannot_bypass_approval_or_card_requirements() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let (_, pool, repository, workspace_id, product_id) = setup_active_workspace().await;
    let source = create_free_plan(&repository, product_id, PlanRecurrence::Monthly, 60).await;
    let current = join_plan(
        &repository,
        workspace_id,
        source.plan_version_id,
        "admission",
    )
    .await;
    let before = serde_json::to_value(&current).unwrap();
    for (approval, expected_code) in [
        (true, "customer_plan_approval_required"),
        (false, "plan_transition_card_validation_required"),
    ] {
        let mut request = plan_request(
            product_id,
            CommercialModel::Free,
            PlanRecurrence::Monthly,
            10,
        );
        if approval {
            request.admission_policy = AdmissionPolicy::ApprovalRequired;
        } else {
            request.accepted_payment_methods = vec!["CARD".to_string()];
        }
        let target = plans::create_plan(&repository, source.subscription_id, request)
            .await
            .unwrap();
        let transition = CreatePlanTransitionRequest {
            new_plan_version_id: target.plan_version_id,
            transition_kind: PlanTransitionKind::Downgrade,
            transaction_id: "reusable-transition".to_string(),
            actor_reference: "customer:test".to_string(),
        };
        let rejected = plans::transition_customer_plan(
            &repository,
            workspace_id,
            current.customer_plan_id,
            "reusable-key",
            transition,
        )
        .await
        .unwrap_err();
        assert_eq!(rejected.code(), expected_code);
        let unchanged =
            plans::get_customer_plan(&repository, workspace_id, current.customer_plan_id)
                .await
                .unwrap();
        assert_eq!(serde_json::to_value(unchanged).unwrap(), before);
        assert_plan_state(&pool, workspace_id, 1, 1, 1, 60).await;
        let effects: (i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM customer_plan_transitions), (SELECT count(*) FROM credit_lot_reclassifications)")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(effects, (0, 0));
    }
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
    let accepted = plans::transition_customer_plan(
        &repository,
        workspace_id,
        current.customer_plan_id,
        "reusable-key",
        CreatePlanTransitionRequest {
            new_plan_version_id: target.plan_version_id,
            transition_kind: PlanTransitionKind::Downgrade,
            transaction_id: "reusable-transition".to_string(),
            actor_reference: "customer:test".to_string(),
        },
    )
    .await
    .unwrap();
    assert_eq!(accepted.reclassified_credit_units.value(), 60);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejected_admission_creates_no_contract_and_does_not_reserve_keys() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let (_, pool, repository, workspace_id, product_id) = setup_active_workspace().await;
    let subscription = plans::create_subscription(&repository, subscription_request())
        .await
        .unwrap();
    let mut request = plan_request(
        product_id,
        CommercialModel::Free,
        PlanRecurrence::Monthly,
        10,
    );
    request.admission_policy = AdmissionPolicy::ApprovalRequired;
    let restricted = plans::create_plan(&repository, subscription.subscription_id, request)
        .await
        .unwrap();
    let result = plans::create_customer_plan(
        &repository,
        workspace_id,
        "key-admission",
        customer_plan_request(restricted.plan_version_id, "transaction-admission"),
    )
    .await;
    assert_eq!(
        result.unwrap_err().code(),
        "customer_plan_approval_required"
    );
    assert_plan_state(&pool, workspace_id, 0, 0, 0, 0).await;
    let open = plans::create_plan(
        &repository,
        subscription.subscription_id,
        plan_request(
            product_id,
            CommercialModel::Free,
            PlanRecurrence::Monthly,
            10,
        ),
    )
    .await
    .unwrap();
    join_plan(&repository, workspace_id, open.plan_version_id, "admission").await;
    assert_plan_state(&pool, workspace_id, 1, 1, 1, 10).await;
}
