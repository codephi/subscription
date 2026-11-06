use chrono::{DateTime, Utc};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CollectionState {
    Scheduled,
    Collecting,
    PendingPayment,
    Paid,
    Exhausted,
    Expired,
    Canceled,
    Unmatched,
}

impl CollectionState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Paid | Self::Exhausted | Self::Expired | Self::Canceled | Self::Unmatched
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BillingSignal {
    AttemptStarted,
    AwaitingWebhook,
    RequiresAction,
    DefinitiveFailure,
    Uncertain,
    WebhookConfirmed,
    CommercialExpiration,
    Cancel,
    UnmatchedPayment,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BillingEffect {
    None,
    ApplyConfirmedPayment,
    RecordDefinitiveFailure,
    RecordCommercialExpiration,
    RecordUnmatchedPayment,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BillingTransition {
    pub state: CollectionState,
    pub effect: BillingEffect,
    pub changed: bool,
}

pub fn transition_collection(
    current: CollectionState,
    signal: BillingSignal,
    observed_at: DateTime<Utc>,
    payment_expires_at: DateTime<Utc>,
) -> BillingTransition {
    if current.is_terminal() {
        return unchanged(current);
    }
    if signal == BillingSignal::WebhookConfirmed && observed_at >= payment_expires_at {
        return changed(
            CollectionState::Expired,
            BillingEffect::RecordCommercialExpiration,
        );
    }
    match (current, signal) {
        (CollectionState::Scheduled, BillingSignal::AttemptStarted) => {
            changed(CollectionState::Collecting, BillingEffect::None)
        }
        (
            CollectionState::Collecting | CollectionState::PendingPayment,
            BillingSignal::AwaitingWebhook
            | BillingSignal::RequiresAction
            | BillingSignal::Uncertain,
        ) => changed(CollectionState::PendingPayment, BillingEffect::None),
        (
            CollectionState::Collecting | CollectionState::PendingPayment,
            BillingSignal::DefinitiveFailure,
        ) => changed(
            CollectionState::Exhausted,
            BillingEffect::RecordDefinitiveFailure,
        ),
        (
            CollectionState::Collecting | CollectionState::PendingPayment,
            BillingSignal::WebhookConfirmed,
        ) => changed(CollectionState::Paid, BillingEffect::ApplyConfirmedPayment),
        (_, BillingSignal::CommercialExpiration) if observed_at >= payment_expires_at => changed(
            CollectionState::Expired,
            BillingEffect::RecordCommercialExpiration,
        ),
        (_, BillingSignal::Cancel) => changed(CollectionState::Canceled, BillingEffect::None),
        (
            CollectionState::Collecting | CollectionState::PendingPayment,
            BillingSignal::UnmatchedPayment,
        ) => changed(
            CollectionState::Unmatched,
            BillingEffect::RecordUnmatchedPayment,
        ),
        _ => unchanged(current),
    }
}

fn changed(state: CollectionState, effect: BillingEffect) -> BillingTransition {
    BillingTransition {
        state,
        effect,
        changed: true,
    }
}

fn unchanged(state: CollectionState) -> BillingTransition {
    BillingTransition {
        state,
        effect: BillingEffect::None,
        changed: false,
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;

    fn window() -> (DateTime<Utc>, DateTime<Utc>) {
        let now = Utc::now();
        (now, now + TimeDelta::minutes(15))
    }

    #[test]
    fn pending_requires_action_and_uncertain_preserve_one_request() {
        let (now, expires_at) = window();
        for signal in [
            BillingSignal::AwaitingWebhook,
            BillingSignal::RequiresAction,
            BillingSignal::Uncertain,
        ] {
            let transition =
                transition_collection(CollectionState::Collecting, signal, now, expires_at);
            assert_eq!(transition.state, CollectionState::PendingPayment);
            assert_eq!(transition.effect, BillingEffect::None);
        }
    }

    #[test]
    fn confirmation_before_deadline_applies_once_and_terminal_states_do_not_regress() {
        let (now, expires_at) = window();
        let confirmed = transition_collection(
            CollectionState::PendingPayment,
            BillingSignal::WebhookConfirmed,
            now,
            expires_at,
        );
        assert_eq!(confirmed.effect, BillingEffect::ApplyConfirmedPayment);
        let duplicate = transition_collection(
            confirmed.state,
            BillingSignal::WebhookConfirmed,
            now,
            expires_at,
        );
        assert!(!duplicate.changed);
        assert_eq!(duplicate.effect, BillingEffect::None);
    }

    #[test]
    fn failure_expiration_and_late_confirmation_never_apply_payment() {
        let (now, expires_at) = window();
        let failed = transition_collection(
            CollectionState::PendingPayment,
            BillingSignal::DefinitiveFailure,
            now,
            expires_at,
        );
        assert_eq!(failed.effect, BillingEffect::RecordDefinitiveFailure);
        let expired = transition_collection(
            CollectionState::PendingPayment,
            BillingSignal::CommercialExpiration,
            expires_at,
            expires_at,
        );
        assert_eq!(expired.effect, BillingEffect::RecordCommercialExpiration);
        let late = transition_collection(
            CollectionState::PendingPayment,
            BillingSignal::WebhookConfirmed,
            expires_at,
            expires_at,
        );
        assert_eq!(late.state, CollectionState::Expired);
        assert_eq!(late.effect, BillingEffect::RecordCommercialExpiration);
        let early = transition_collection(
            CollectionState::PendingPayment,
            BillingSignal::CommercialExpiration,
            now,
            expires_at,
        );
        assert!(!early.changed);
    }

    #[test]
    fn unmatched_payment_is_recorded_without_confirming_collection() {
        let (now, expires_at) = window();
        let transition = transition_collection(
            CollectionState::PendingPayment,
            BillingSignal::UnmatchedPayment,
            now,
            expires_at,
        );
        assert_eq!(transition.state, CollectionState::Unmatched);
        assert_eq!(transition.effect, BillingEffect::RecordUnmatchedPayment);
    }
}
