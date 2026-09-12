mod support;
#[path = "support/usage_fixture.rs"]
mod usage_fixture;

use uuid::Uuid;

use usage_fixture::setup_usage;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn billing_references_reject_cross_workspace_connection_plan_and_binding() {
    let usage = setup_usage(1, 1, 10).await;
    let pool = usage.repository.pool();
    let other_workspace_id = Uuid::new_v4();
    insert_workspace(&pool, other_workspace_id).await;
    let connection_id = insert_connection(&pool, usage.workspace_id).await;
    let cross_connection = insert_binding(
        &pool,
        other_workspace_id,
        connection_id,
        None,
        Uuid::new_v4(),
    )
    .await;
    assert!(cross_connection.is_err());

    let other_connection_id = insert_connection(&pool, other_workspace_id).await;
    let cross_plan = insert_binding(
        &pool,
        other_workspace_id,
        other_connection_id,
        Some(usage.customer_plan_id),
        Uuid::new_v4(),
    )
    .await;
    assert!(cross_plan.is_err());

    let binding_id = Uuid::new_v4();
    insert_binding(&pool, usage.workspace_id, connection_id, None, binding_id)
        .await
        .unwrap();
    let cross_binding = sqlx::query(
        "INSERT INTO collection_requests (collection_request_id,workspace_id,customer_id, \
         payment_method_binding_id,request_kind,amount_minor,currency,granted_credit_units,status, \
         transaction_id,idempotency_key,correlation_id,scheduled_at,payment_expires_at) \
         VALUES ($1,$2,$2,$3,'INITIAL',100,'BRL',1,'SCHEDULED',$4,$5,$6,now(),now()+interval '15 minutes')",
    )
    .bind(Uuid::new_v4())
    .bind(other_workspace_id)
    .bind(binding_id)
    .bind(format!("transaction-{}", Uuid::new_v4()))
    .bind(format!("key-{}", Uuid::new_v4()))
    .bind(Uuid::new_v4())
    .execute(&pool)
    .await;
    assert!(cross_binding.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn v1_billing_rejects_every_payment_method_except_tokenized_card() {
    let usage = setup_usage(1, 1, 10).await;
    let pool = usage.repository.pool();
    let connection_id = insert_connection(&pool, usage.workspace_id).await;
    let invalid_binding = sqlx::query(
        "INSERT INTO payment_method_bindings (payment_method_binding_id,billing_connection_id, \
         workspace_id,customer_id,payment_method,provider_payment_method_reference,status) \
         VALUES ($1,$2,$3,$3,'PIX',$4,'ACTIVE')",
    )
    .bind(Uuid::new_v4())
    .bind(connection_id)
    .bind(usage.workspace_id)
    .bind(format!("pix-{}", Uuid::new_v4()))
    .execute(&pool)
    .await;
    assert!(invalid_binding.is_err());

    let binding_id = Uuid::new_v4();
    insert_binding(&pool, usage.workspace_id, connection_id, None, binding_id)
        .await
        .unwrap();
    let request_id = insert_valid_request(&pool, usage.workspace_id, binding_id).await;
    let invalid_attempt = sqlx::query(
        "INSERT INTO collection_attempts (collection_attempt_id,collection_request_id, \
         attempt_number,connector,payment_method,provider_idempotency_key,status,scheduled_at) \
         VALUES ($1,$2,1,'FAKE','PIX',$3,'SCHEDULED',now())",
    )
    .bind(Uuid::new_v4())
    .bind(request_id)
    .bind(format!("attempt-{request_id}"))
    .execute(&pool)
    .await;
    assert!(invalid_attempt.is_err());
}

async fn insert_workspace(pool: &sqlx::PgPool, workspace_id: Uuid) {
    sqlx::query(
        "INSERT INTO workspace_projections (workspace_id,operational_status,external_sequence, \
         external_occurred_at,last_event_id) VALUES ($1,'ACTIVE',1,now(),$2)",
    )
    .bind(workspace_id)
    .bind(Uuid::new_v4())
    .execute(pool)
    .await
    .unwrap();
}

async fn insert_connection(pool: &sqlx::PgPool, workspace_id: Uuid) -> Uuid {
    let connection_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO billing_connections (billing_connection_id,workspace_id,provider, \
         external_account_reference,secret_reference,capabilities,status) \
         VALUES ($1,$2,'FAKE',$3,'secret://fake',ARRAY['CARD'],'ACTIVE')",
    )
    .bind(connection_id)
    .bind(workspace_id)
    .bind(format!("account-{connection_id}"))
    .execute(pool)
    .await
    .unwrap();
    connection_id
}

async fn insert_binding(
    pool: &sqlx::PgPool,
    workspace_id: Uuid,
    connection_id: Uuid,
    customer_plan_id: Option<Uuid>,
    binding_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO payment_method_bindings (payment_method_binding_id,billing_connection_id, \
         workspace_id,customer_id,customer_plan_id,payment_method, \
         provider_payment_method_reference,status) VALUES ($1,$2,$3,$3,$4,'CARD',$5,'ACTIVE')",
    )
    .bind(binding_id)
    .bind(connection_id)
    .bind(workspace_id)
    .bind(customer_plan_id)
    .bind(format!("pm-{binding_id}"))
    .execute(pool)
    .await?;
    Ok(())
}

async fn insert_valid_request(pool: &sqlx::PgPool, workspace_id: Uuid, binding_id: Uuid) -> Uuid {
    let request_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO collection_requests (collection_request_id,workspace_id,customer_id, \
         payment_method_binding_id,request_kind,amount_minor,currency,granted_credit_units,status, \
         transaction_id,idempotency_key,correlation_id,scheduled_at,payment_expires_at) \
         VALUES ($1,$2,$2,$3,'INITIAL',100,'BRL',1,'SCHEDULED',$4,$5,$6,now(),now()+interval '15 minutes')",
    )
    .bind(request_id)
    .bind(workspace_id)
    .bind(binding_id)
    .bind(format!("transaction-{request_id}"))
    .bind(format!("key-{request_id}"))
    .bind(Uuid::new_v4())
    .execute(pool)
    .await
    .unwrap();
    request_id
}
