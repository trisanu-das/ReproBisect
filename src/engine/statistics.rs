use crate::model::{StochasticEffectClassification, StochasticEffectEvidence};

/// Compare the rate at which matched baseline and variant trials depart from the
/// stable control artifact signature. The p-value is the one-sided Fisher exact
/// probability of observing at least this much concentration of changed trials
/// in the variant group under equal change propensity.
pub fn assess_stochastic_effect(
    baseline_changed: usize,
    baseline_trials: usize,
    variant_changed: usize,
    variant_trials: usize,
    alpha: f64,
) -> StochasticEffectEvidence {
    let baseline_change_rate = rate(baseline_changed, baseline_trials);
    let variant_change_rate = rate(variant_changed, variant_trials);
    let p_value = fisher_exact_greater(
        baseline_changed,
        baseline_trials.saturating_sub(baseline_changed),
        variant_changed,
        variant_trials.saturating_sub(variant_changed),
    );

    let classification = if baseline_changed > 0 {
        // ReproBisect's deterministic causal gate requires a stable baseline.
        // Even a statistically larger variant rate is therefore not promoted
        // when the matched baseline itself contradicted that gate.
        StochasticEffectClassification::BaselineUnstable
    } else if variant_changed > baseline_changed
        && variant_change_rate > baseline_change_rate
        && p_value <= alpha
    {
        StochasticEffectClassification::Supported
    } else {
        StochasticEffectClassification::NotSupported
    };

    StochasticEffectEvidence {
        baseline_trials,
        variant_trials,
        baseline_changed_trials: baseline_changed,
        variant_changed_trials: variant_changed,
        baseline_change_rate,
        variant_change_rate,
        absolute_rate_difference: variant_change_rate - baseline_change_rate,
        fisher_exact_p_value: p_value,
        alpha,
        classification,
    }
}

fn rate(changed: usize, trials: usize) -> f64 {
    if trials == 0 {
        0.0
    } else {
        changed as f64 / trials as f64
    }
}

/// One-sided Fisher exact test for a 2x2 table:
///
/// ```text
///                 changed  unchanged
/// baseline           a         b
/// variant            c         d
/// ```
///
/// The alternative is that changed outcomes are enriched in the variant group.
/// Computation is performed in log-space and is intended for the bounded trial
/// counts accepted by configuration validation.
pub(crate) fn fisher_exact_greater(a: usize, b: usize, c: usize, d: usize) -> f64 {
    let baseline_n = a + b;
    let variant_n = c + d;
    let total_n = baseline_n + variant_n;
    let total_changed = a + c;
    if total_n == 0 || variant_n == 0 || baseline_n == 0 {
        return 1.0;
    }

    let max_variant_changed = total_changed.min(variant_n);
    if c > max_variant_changed {
        return 0.0;
    }
    let denominator = ln_choose(total_n, total_changed);
    let mut probability = 0.0_f64;
    for x in c..=max_variant_changed {
        let baseline_changed = total_changed.saturating_sub(x);
        if baseline_changed > baseline_n {
            continue;
        }
        let log_p = ln_choose(variant_n, x)
            + ln_choose(baseline_n, baseline_changed)
            - denominator;
        probability += log_p.exp();
    }
    probability.clamp(0.0, 1.0)
}

fn ln_choose(n: usize, k: usize) -> f64 {
    if k > n {
        return f64::NEG_INFINITY;
    }
    let k = k.min(n - k);
    (1..=k)
        .map(|i| ((n - k + i) as f64).ln() - (i as f64).ln())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fisher_detects_complete_matched_separation() {
        let p = fisher_exact_greater(0, 5, 5, 0);
        assert!((p - (1.0 / 252.0)).abs() < 1e-12);
    }

    #[test]
    fn fisher_does_not_promote_one_change_in_five() {
        let evidence = assess_stochastic_effect(0, 5, 1, 5, 0.05);
        assert_eq!(
            evidence.classification,
            StochasticEffectClassification::NotSupported
        );
        assert!(evidence.fisher_exact_p_value > 0.05);
    }

    #[test]
    fn matched_baseline_instability_blocks_causal_promotion() {
        let evidence = assess_stochastic_effect(1, 8, 8, 8, 0.05);
        assert!(evidence.fisher_exact_p_value < 0.05);
        assert_eq!(
            evidence.classification,
            StochasticEffectClassification::BaselineUnstable
        );
    }
}
