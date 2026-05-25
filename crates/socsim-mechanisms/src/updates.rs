//! Bare message-set opinion-update functions (Δ / delta form).
//!
//! These are the **Chuang–Rogers / Deffuant message-set family**: each function
//! takes the agent's current attitude `a_i` and a *message set* `messages`
//! (= `{ f_message(a_j) | j ∈ J_i }`) and **returns a delta `Δa`** — the caller
//! applies `clamp_attitude(a_i + Δa)`.  They are ported **byte-for-byte** from
//! the `mou2024` (HiSim) replication's `simulation/src/abm.rs`, so a hybrid
//! model (LLM core + ABM periphery, e.g. `mou2024`) can drive its periphery
//! layer through this pack rather than maintaining a private copy.
//!
//! These are **distinct** from the standalone mechanisms in [`crate::opinion`]:
//!
//! - [`bounded_confidence_update`] / [`hk_update`] here are the *message-set Δ*
//!   form with an assimilation rate α.  In particular [`hk_update`] is the HK
//!   *arithmetic mean (including self) → α-fraction move* — **not** the same as
//!   [`HegselmannKrauseMechanism`](crate::HegselmannKrauseMechanism), which is a
//!   full move to a (generalised) mean of the ε-confidence set.
//! - The pack's [`DeffuantMechanism`](crate::DeffuantMechanism) is the *pairwise*
//!   bounded-confidence update — a different (event-based) family — and is **not**
//!   routed through here.
//!
//! [`social_judgement_update`] and [`lorenz_update`] are the single source of
//! truth for the polarising family: the pack's
//! [`SocialJudgementMechanism`](crate::SocialJudgementMechanism) and
//! [`LorenzMechanism`](crate::LorenzMechanism) call them.
//!
//! All functions match `mou2024::abm` exactly: an **empty** message set yields
//! `Δa = 0.0`; the bounded-confidence `within_bound` test uses a **strict** `<`.

/// Lower bound of the attitude range `A = [-1, 1]` (matches `mou2024::abm`).
pub const ATTITUDE_MIN: f64 = -1.0;
/// Upper bound of the attitude range `A = [-1, 1]` (matches `mou2024::abm`).
pub const ATTITUDE_MAX: f64 = 1.0;

/// Message function `m_j = f_message(a_j) = a_j` (identity).
///
/// Most ABMs transmit the sender's attitude unbiased (HiSim §6).  Ported from
/// `mou2024::abm::f_message`.
#[inline]
pub fn f_message(a_j: f64) -> f64 {
    a_j
}

/// Clamp an attitude to the range `[-1, 1]`.
///
/// Ported from `mou2024::abm::clamp_attitude`.
#[inline]
pub fn clamp_attitude(a: f64) -> f64 {
    a.clamp(ATTITUDE_MIN, ATTITUDE_MAX)
}

/// Confidence-bound similarity: `1` (in-bound) iff `|m_j − a_i| < ε`, else `0`.
///
/// Note the **strict** `<`, matching `mou2024::abm::within_bound`.
#[inline]
fn within_bound(a_i: f64, m_j: f64, epsilon: f64) -> bool {
    (m_j - a_i).abs() < epsilon
}

/// Bounded Confidence (Deffuant et al. 2000) message-set Δ update.
///
/// Averages `α · (m_j − a_i)` over the in-bound messages (`|m_j − a_i| < ε`); if
/// no message is in bound (or the set is empty), `Δa = 0`.  Ported byte-for-byte
/// from `mou2024::abm::bc_update` (with the empty-set guard of `f_update`).
pub fn bounded_confidence_update(a_i: f64, messages: &[f64], epsilon: f64, alpha: f64) -> f64 {
    if messages.is_empty() {
        return 0.0;
    }
    let mut sum = 0.0;
    let mut count = 0usize;
    for &m_j in messages {
        if within_bound(a_i, m_j, epsilon) {
            sum += alpha * (m_j - a_i);
            count += 1;
        }
    }
    if count == 0 {
        0.0
    } else {
        sum / count as f64
    }
}

/// Hegselmann–Krause (2002) message-set Δ update (multi-source BC).
///
/// Moves an α-fraction toward the **arithmetic mean (including self)** of the
/// in-bound messages: `Δa = α · (mean_{|m_j − a_i| < ε ∪ {a_i}} − a_i)`.  Ported
/// byte-for-byte from `mou2024::abm::hk_update` (with the empty-set guard of
/// `f_update`).
///
/// NOTE: this is the *message-set Δ* HK and is **distinct** from
/// [`HegselmannKrauseMechanism`](crate::HegselmannKrauseMechanism) (a full move
/// to a generalised mean of the ε-confidence set).
pub fn hk_update(a_i: f64, messages: &[f64], epsilon: f64, alpha: f64) -> f64 {
    if messages.is_empty() {
        return 0.0;
    }
    // Self is always in the confidence set (HK standard; always in bound).
    let mut sum = a_i;
    let mut count = 1usize;
    for &m_j in messages {
        if within_bound(a_i, m_j, epsilon) {
            sum += m_j;
            count += 1;
        }
    }
    let mean = sum / count as f64;
    alpha * (mean - a_i)
}

/// Social Judgement message-set Δ update.
///
/// Acceptance region (`|diff| < ε`): assimilate, `Δ += α · diff`.  Rejection
/// region (`|diff| > rejection`): repel, `Δ -= repulsion · sign(diff)`.
/// Non-commitment region (`ε ≤ |diff| ≤ rejection`): no contribution.  The delta
/// is the mean over the contributing messages.  Ported byte-for-byte from
/// `mou2024::abm::sj_update` (with the empty-set guard of `f_update`).
pub fn social_judgement_update(
    a_i: f64,
    messages: &[f64],
    epsilon: f64,
    alpha: f64,
    rejection: f64,
    repulsion: f64,
) -> f64 {
    if messages.is_empty() {
        return 0.0;
    }
    let mut delta = 0.0;
    let mut count = 0usize;
    for &m_j in messages {
        let diff = m_j - a_i;
        if diff.abs() < epsilon {
            // Acceptance region: assimilate.
            delta += alpha * diff;
            count += 1;
        } else if diff.abs() > rejection {
            // Rejection region: repel (away from the message).
            delta -= repulsion * diff.signum();
            count += 1;
        }
        // Non-commitment region (ε <= |diff| <= rejection): no contribution.
    }
    if count == 0 {
        0.0
    } else {
        delta / count as f64
    }
}

/// Lorenz (2021) message-set Δ update — assimilation + reinforcement + polarisation.
///
/// Assimilates the mean in-region gap (`α · mean_{|diff| < ε} diff`), then adds a
/// `polarization = repulsion · sign(a_i) · |a_i|` term that pushes the current
/// attitude further out in its own direction (extreme attitudes amplified more).
/// Ported byte-for-byte from `mou2024::abm::lorenz_update` (with the empty-set
/// guard of `f_update`).
pub fn lorenz_update(a_i: f64, messages: &[f64], epsilon: f64, alpha: f64, repulsion: f64) -> f64 {
    if messages.is_empty() {
        return 0.0;
    }
    let mut assimilation = 0.0;
    let mut count = 0usize;
    for &m_j in messages {
        let diff = m_j - a_i;
        if diff.abs() < epsilon {
            assimilation += alpha * diff;
            count += 1;
        }
    }
    let assimilation = if count == 0 {
        0.0
    } else {
        assimilation / count as f64
    };
    // Reinforcement + polarisation: push toward the current sign, scaled by
    // |a_i| (the more extreme, the stronger).
    let polarization = repulsion * a_i.signum() * a_i.abs();
    assimilation + polarization
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALPHA: f64 = 0.5;
    const EPSILON: f64 = 0.4;
    const REJECTION: f64 = 0.8;
    const REPULSION: f64 = 0.2;

    #[test]
    fn empty_messages_no_change() {
        assert_eq!(bounded_confidence_update(0.3, &[], EPSILON, ALPHA), 0.0);
        assert_eq!(hk_update(0.3, &[], EPSILON, ALPHA), 0.0);
        assert_eq!(
            social_judgement_update(0.3, &[], EPSILON, ALPHA, REJECTION, REPULSION),
            0.0
        );
        assert_eq!(lorenz_update(0.3, &[], EPSILON, ALPHA, REPULSION), 0.0);
    }

    #[test]
    fn bc_assimilates_within_bound() {
        // a_i=0.0, m_j=0.2 (within ε=0.4) → Δ = 0.5*(0.2-0) = 0.1
        let d = bounded_confidence_update(0.0, &[0.2], EPSILON, ALPHA);
        assert!((d - 0.1).abs() < 1e-12);
    }

    #[test]
    fn bc_ignores_outside_bound() {
        // a_i=0.0, m_j=0.9 (outside ε=0.4) → Δ = 0
        let d = bounded_confidence_update(0.0, &[0.9], EPSILON, ALPHA);
        assert_eq!(d, 0.0);
    }

    #[test]
    fn bc_moves_toward_neighbors() {
        // BC: should move toward nearby positive neighbours.
        let d = bounded_confidence_update(-0.1, &[0.1, 0.05], EPSILON, ALPHA);
        assert!(d > 0.0, "should move toward positive neighbors");
    }

    #[test]
    fn hk_moves_toward_mean() {
        // a_i=0.0, messages 0.2,0.2 within ε → mean of {0,0.2,0.2}≈0.1333, Δ=0.5*0.1333
        let d = hk_update(0.0, &[0.2, 0.2], EPSILON, ALPHA);
        assert!(d > 0.0 && d < 0.2);
    }

    #[test]
    fn sj_repels_in_rejection_region() {
        // a_i=0.0, m_j=0.9 (>rejection 0.8) → repel away (negative Δ)
        let d = social_judgement_update(0.0, &[0.9], EPSILON, ALPHA, REJECTION, REPULSION);
        assert!(d < 0.0, "SJ should repel from far-positive message");
    }

    #[test]
    fn sj_assimilates_in_acceptance_region() {
        let d = social_judgement_update(0.0, &[0.2], EPSILON, ALPHA, REJECTION, REPULSION);
        assert!(d > 0.0, "SJ should assimilate near message");
    }

    #[test]
    fn lorenz_polarizes_extremes() {
        // An extreme positive attitude is pushed further positive by the
        // polarisation term even with only an out-of-bound message.
        let d = lorenz_update(0.8, &[-0.9], EPSILON, ALPHA, REPULSION);
        assert!(
            d > 0.0,
            "Lorenz should push an extreme attitude further out"
        );
    }

    #[test]
    fn clamp_keeps_in_range() {
        assert_eq!(clamp_attitude(1.5), 1.0);
        assert_eq!(clamp_attitude(-2.0), -1.0);
        assert_eq!(clamp_attitude(0.3), 0.3);
    }

    #[test]
    fn f_message_is_identity() {
        assert_eq!(f_message(0.42), 0.42);
        assert_eq!(f_message(-0.7), -0.7);
    }
}
