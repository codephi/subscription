use super::*;
use subscription::{
    dto::admission::{AdmissionEvidenceRequest, AdmissionFact, CreateAdmissionPolicyRequest},
    services::admission,
};

pub(super) fn evidence(
    workspace_id: Uuid,
    policy_version_id: Uuid,
    sequence: i64,
) -> AdmissionEvidenceRequest {
    AdmissionEvidenceRequest {
        event_id: Uuid::new_v4(),
        workspace_id,
        policy_version_id,
        sequence,
        verified_facts: vec![AdmissionFact::EmailVerified],
        evidence_reference: "accounts:verification-123".into(),
        valid_until: Utc::now() + Duration::hours(1),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admission_policy_versions_are_immutable_and_plan_reference_cannot_change() {
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let (_, pool, repository, _, product_id) = setup_active_workspace().await;
    let mut request = CreateAdmissionPolicyRequest {
        policy_id: Uuid::new_v4(),
        version: 1,
        required_facts: vec![AdmissionFact::EmailVerified],
    };
    let first = admission::create_policy(&repository, request.clone())
        .await
        .unwrap();
    assert_eq!(
        admission::create_policy(&repository, request.clone())
            .await
            .unwrap_err()
            .code(),
        "admission_policy_version_exists"
    );
    request.version = 2;
    request.required_facts.push(AdmissionFact::IdentityVerified);
    let second = admission::create_policy(&repository, request.clone())
        .await
        .unwrap();
    assert_ne!(first.policy_version_id, second.policy_version_id);
    assert_eq!(
        admission::get_policy(&repository, first.policy_version_id)
            .await
            .unwrap()
            .required_facts,
        vec!["EMAIL_VERIFIED"]
    );
    request.version = 0;
    assert!(admission::create_policy(&repository, request.clone())
        .await
        .is_err());
    request.version = 3;
    request.required_facts = vec![AdmissionFact::EmailVerified; 2];
    assert!(admission::create_policy(&repository, request.clone())
        .await
        .is_err());
    request.required_facts.clear();
    assert!(admission::create_policy(&repository, request)
        .await
        .is_err());
    assert!(sqlx::query(
        "UPDATE subscription_admission_policies SET version=99 WHERE policy_version_id=$1"
    )
    .bind(first.policy_version_id)
    .execute(&pool)
    .await
    .is_err());
    let subscription = plans::create_subscription(&repository, subscription_request())
        .await
        .unwrap();
    let mut offer = plan_request(
        product_id,
        CommercialModel::Free,
        PlanRecurrence::Monthly,
        60,
    );
    offer.admission_policy_version_id = Some(first.policy_version_id);
    assert_eq!(
        plans::create_plan(&repository, subscription.subscription_id, offer.clone())
            .await
            .unwrap_err()
            .code(),
        "invalid_admission_policy_reference"
    );
    offer.admission_policy = AdmissionPolicy::ApprovalRequired;
    let plan = plans::create_plan(&repository, subscription.subscription_id, offer)
        .await
        .unwrap();
    assert!(sqlx::query("UPDATE subscription_plan_versions SET admission_policy_version_id=$2,revoked_at=now() WHERE plan_version_id=$1")
        .bind(plan.plan_version_id).bind(second.policy_version_id).execute(&pool).await.is_err());
    assert_eq!(
        plans::get_plan(&repository, plan.plan_version_id)
            .await
            .unwrap()
            .admission_policy_version_id,
        Some(first.policy_version_id)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admission_evidence_controls_join_and_transition_with_immutable_decisions() {
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
    let plan = plans::create_plan(&repository, subscription.subscription_id, offer.clone())
        .await
        .unwrap();
    let request = customer_plan_request(plan.plan_version_id, "transaction-proof");
    assert!(
        plans::create_customer_plan(&repository, workspace_id, "key-proof", request.clone())
            .await
            .is_err()
    );
    let mut expired = evidence(workspace_id, policy.policy_version_id, 1);
    expired.valid_until = Utc::now() - Duration::seconds(1);
    admission::receive_evidence(&repository, expired)
        .await
        .unwrap();
    assert!(
        plans::create_customer_plan(&repository, workspace_id, "key-proof", request.clone())
            .await
            .is_err()
    );
    assert_plan_state(&pool, workspace_id, 0, 0, 0, 0).await;
    let approved = evidence(workspace_id, policy.policy_version_id, 2);
    admission::receive_evidence(&repository, approved.clone())
        .await
        .unwrap();
    let current = plans::create_customer_plan(&repository, workspace_id, "key-proof", request)
        .await
        .unwrap();
    let other = Uuid::new_v4();
    apply_workspace_event(&repository, other, "workspace.created", 1).await;
    apply_workspace_event(&repository, other, "workspace.activated", 2).await;
    assert!(plans::create_customer_plan(
        &repository,
        other,
        "other-proof",
        customer_plan_request(plan.plan_version_id, "other-proof")
    )
    .await
    .is_err());
    offer.granted_credit_units = CreditUnits::new(10);
    let target = plans::create_plan(&repository, subscription.subscription_id, offer)
        .await
        .unwrap();
    let mut withdrawn = evidence(workspace_id, policy.policy_version_id, 3);
    withdrawn.verified_facts.clear();
    admission::receive_evidence(&repository, withdrawn)
        .await
        .unwrap();
    let transition = subscription::dto::plans::CreatePlanTransitionRequest {
        new_plan_version_id: target.plan_version_id,
        transition_kind: subscription::dto::plans::PlanTransitionKind::Downgrade,
        transaction_id: "proof-transition".into(),
        actor_reference: "customer:test".into(),
    };
    assert!(plans::transition_customer_plan(
        &repository,
        workspace_id,
        current.customer_plan_id,
        "proof-transition",
        transition.clone()
    )
    .await
    .is_err());
    assert_plan_state(&pool, workspace_id, 1, 1, 1, 60).await;
    admission::receive_evidence(
        &repository,
        evidence(workspace_id, policy.policy_version_id, 4),
    )
    .await
    .unwrap();
    plans::transition_customer_plan(
        &repository,
        workspace_id,
        current.customer_plan_id,
        "proof-transition",
        transition,
    )
    .await
    .unwrap();
    let decisions: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM subscription_admission_decisions WHERE customer_plan_id=$1",
    )
    .bind(current.customer_plan_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(decisions, 2);
    let original: Uuid = sqlx::query_scalar("SELECT evidence_event_id FROM subscription_admission_decisions WHERE customer_plan_id=$1 AND plan_transition_id IS NULL").bind(current.customer_plan_id).fetch_one(&pool).await.unwrap();
    assert_eq!(original, approved.event_id);
    assert!(
        sqlx::query("DELETE FROM subscription_admission_decisions WHERE customer_plan_id=$1")
            .bind(current.customer_plan_id)
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(sqlx::query(
        "UPDATE subscription_admission_evidence SET verified_facts='{}' WHERE event_id=$1"
    )
    .bind(approved.event_id)
    .execute(&pool)
    .await
    .is_err());
    assert!(
        subscription::services::credits::reconcile(&repository, workspace_id)
            .await
            .unwrap()
            .consistent
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admission_evidence_requires_signature_and_preserves_event_identity_and_sequence() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    let _guard = TEST_LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let (router, pool) = setup_router_with_options(false, Some("admission-secret".into())).await;
    let repository = DatabaseRepository::new(pool.clone());
    let workspace_id = Uuid::new_v4();
    apply_workspace_event(&repository, workspace_id, "workspace.created", 1).await;
    let policy_body = serde_json::json!({"policy_id":Uuid::new_v4(),"version":1,"required_facts":["EMAIL_VERIFIED"]});
    let created = router
        .clone()
        .oneshot(
            Request::post("/v1/admission-policies")
                .header("content-type", "application/json")
                .body(Body::from(policy_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(created.status(), 201);
    let policy_id: Uuid =
        serde_json::from_value(support::response_json(created).await["policy_version_id"].clone())
            .unwrap();
    let fetched = get_json(&router, &format!("/v1/admission-policies/{policy_id}")).await;
    assert_eq!(fetched["version"], 1);
    let original = evidence(workspace_id, policy_id, 1);
    let now = subscription::services::signatures::current_timestamp().unwrap();
    assert_eq!(
        post_signed_evidence(&router, &original, "wrong-secret", now)
            .await
            .status(),
        401
    );
    assert_eq!(
        post_signed_evidence(&router, &original, "admission-secret", now - 600)
            .await
            .status(),
        401
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM subscription_admission_evidence")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let accepted = post_signed_evidence(&router, &original, "admission-secret", now).await;
    assert_eq!(accepted.status(), 202);
    assert_eq!(support::response_json(accepted).await["duplicate"], false);
    let repeated = post_signed_evidence(&router, &original, "admission-secret", now).await;
    assert_eq!(support::response_json(repeated).await["duplicate"], true);
    let mut altered = original.clone();
    altered.verified_facts.clear();
    assert_eq!(
        post_signed_evidence(&router, &altered, "admission-secret", now)
            .await
            .status(),
        409
    );
    let skipped = evidence(workspace_id, policy_id, 3);
    assert_eq!(
        post_signed_evidence(&router, &skipped, "admission-secret", now)
            .await
            .status(),
        409
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM subscription_admission_evidence")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

async fn post_signed_evidence(
    router: &Router,
    request: &AdmissionEvidenceRequest,
    secret: &str,
    timestamp: i64,
) -> axum::response::Response {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    let body = serde_json::to_vec(request).unwrap();
    let signature =
        subscription::services::signatures::sign_body(secret, timestamp, &body).unwrap();
    router
        .clone()
        .oneshot(
            Request::post("/v1/internal/accounts/admission-evidence")
                .header("content-type", "application/json")
                .header("x-runvibe-timestamp", timestamp.to_string())
                .header("x-runvibe-signature", signature)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap()
}
