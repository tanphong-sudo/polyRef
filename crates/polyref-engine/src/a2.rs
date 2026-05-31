//! Algorithm A2 — frontier status assignment (paper Figure 6).
//!
//! `validate_frontier` is a **pure reducer**: given the required frontier items,
//! their obligations, the Layer 5 pre-check Unknowns, and the checker verdicts
//! collected for each item, it assigns each item one [`Outcome`] and reduces those
//! to one [`ObservationDecision`]. No graph, plugin, or filesystem access.
//!
//! # Per-item precedence (load-bearing — see ADR-005 §3, architecture.md)
//!
//! `Broken > Unknown > Migrated > Pres`. Evaluated per item over all its verdicts:
//!
//! 1. any required checker concretely refutes a predicate ⇒ `Broken`;
//! 2. else a Layer 5 pre-check Unknown, or any required checker reports missing /
//!    unsupported / ambiguous / timed-out evidence ⇒ `Unknown`;
//! 3. else every verdict is `Migrated` (≥1) / `Pres` and at least one is `Migrated`
//!    ⇒ `Migrated`;
//! 4. else every verdict is `Pres` ⇒ `Pres`;
//! 5. else (no verdict at all) ⇒ `Unknown(NoAcceptingRuleApplied)` — fail-closed.
//!
//! When several verdicts share the dominating status, the headline reason is the
//! one whose canonical snake-case tag is lexicographically smallest, ties broken by
//! ascending `(checker_version, rule_version)` (replay byte-stability). All verdicts
//! are retained in [`ValidateFrontierOutput::evidence`].
//!
//! # Observation decision (meet over `required(o)`)
//!
//! `Accepted` iff every required item is `Pres`/`Migrated`; `Broken` if any required
//! item is `Broken`; otherwise `Unknown`.

use std::collections::{BTreeMap, BTreeSet};

use polyref_core::{
    evidence::Evidence,
    report::ObservationDecision,
    status::{BrokenReason, Outcome, UnknownReason},
};
use polyref_frontier::FrontierItem;

use crate::obligation::FrontierObligationSet;

/// Checker verdicts collected per frontier item. Each item maps to the evidence
/// records returned by the required checkers that voted on it.
pub type ItemVerdicts = BTreeMap<FrontierItem, Vec<Evidence>>;

/// Input to A2 frontier validation.
#[derive(Debug, Clone)]
pub struct ValidateFrontierInput {
    /// Observation being validated.
    pub observation_id: String,
    /// `required(o)` — the items whose status decides acceptance.
    pub required_items: Vec<FrontierItem>,
    /// Typed obligations for the frontier (carries Layer 5 pre-check Unknowns).
    pub obligations: FrontierObligationSet,
    /// Checker verdicts per item.
    pub verdicts: ItemVerdicts,
}

/// Output of A2 frontier validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidateFrontierOutput {
    /// Observation being validated.
    pub observation_id: String,
    /// Per-item assigned outcome, sorted by item.
    pub statuses: BTreeMap<FrontierItem, Outcome>,
    /// Per-item headline evidence (the verdict that produced the status), sorted
    /// by item. Items with no verdict have no evidence entry.
    pub evidence: BTreeMap<FrontierItem, Evidence>,
    /// Reduced per-observation decision.
    pub decision: ObservationDecision,
}

/// Run A2 over one observation's required frontier.
#[must_use]
pub fn validate_frontier(input: &ValidateFrontierInput) -> ValidateFrontierOutput {
    // Items carrying a Layer 5 pre-check Unknown are forced Unknown before any
    // checker verdict can produce an accepting status (fail-closed).
    let precheck_items: BTreeSet<&str> = input
        .obligations
        .precheck_unknowns
        .iter()
        .map(|p| p.item.as_str())
        .collect();

    let mut statuses = BTreeMap::<FrontierItem, Outcome>::new();
    let mut evidence = BTreeMap::<FrontierItem, Evidence>::new();

    for item in &input.required_items {
        let verdicts = input.verdicts.get(item).map_or(&[][..], Vec::as_slice);
        let (outcome, headline) =
            assign_item_status(verdicts, precheck_items.contains(item_key(item).as_str()));
        statuses.insert(item.clone(), outcome);
        if let Some(ev) = headline {
            evidence.insert(item.clone(), ev.clone());
        }
    }

    let decision = observation_decision(&input.required_items, &statuses);

    ValidateFrontierOutput {
        observation_id: input.observation_id.clone(),
        statuses,
        evidence,
        decision,
    }
}

/// Assign one item's outcome over its verdicts, honouring the load-bearing
/// `Broken > Unknown > Migrated > Pres` precedence. Returns the outcome plus the
/// headline evidence that produced it (if any).
fn assign_item_status(
    verdicts: &[Evidence],
    precheck_unknown: bool,
) -> (Outcome, Option<&Evidence>) {
    // Step 1: any concrete refutation ⇒ Broken (dominates everything).
    let brokens: Vec<&Evidence> = verdicts
        .iter()
        .filter(|e| matches!(e.outcome(), Outcome::Broken { .. }))
        .collect();
    if let Some(headline) = pick_broken(&brokens) {
        if let Outcome::Broken { reason } = headline.outcome() {
            return (Outcome::Broken { reason: *reason }, Some(headline));
        }
    }

    // Step 2: a Layer 5 pre-check Unknown, or any missing/unsupported/ambiguous/
    // timed-out checker verdict ⇒ Unknown.
    let unknowns: Vec<&Evidence> = verdicts
        .iter()
        .filter(|e| matches!(e.outcome(), Outcome::Unknown { .. }))
        .collect();
    if precheck_unknown {
        // Prefer a concrete checker Unknown reason if present; else MissingEndpoint
        // is the canonical Layer 5 coverage gap.
        if let Some(headline) = pick_unknown(&unknowns) {
            if let Outcome::Unknown { reason } = headline.outcome() {
                return (Outcome::Unknown { reason: *reason }, Some(headline));
            }
        }
        return (
            Outcome::Unknown {
                reason: UnknownReason::MissingEndpoint,
            },
            None,
        );
    }
    if let Some(headline) = pick_unknown(&unknowns) {
        if let Outcome::Unknown { reason } = headline.outcome() {
            return (Outcome::Unknown { reason: *reason }, Some(headline));
        }
    }

    // Steps 3-4: accepting statuses. Need ≥1 verdict; Migrated if any verdict is
    // Migrated, else Pres if all are Pres.
    let mut saw_migrated = None;
    let mut saw_pres = None;
    for e in verdicts {
        match e.outcome() {
            Outcome::Migrated => saw_migrated = saw_migrated.or(Some(e)),
            Outcome::Pres => saw_pres = saw_pres.or(Some(e)),
            // Broken / Unknown already handled in steps 1-2 above; any future
            // non-exhaustive variant is conservatively ignored here (it cannot
            // make an item accepting).
            _ => {}
        }
    }
    if let Some(headline) = saw_migrated {
        return (Outcome::Migrated, Some(headline));
    }
    if let Some(headline) = saw_pres {
        return (Outcome::Pres, Some(headline));
    }

    // Step 5: no accepting rule applied (no verdict for a required item).
    (
        Outcome::Unknown {
            reason: UnknownReason::NoAcceptingRuleApplied,
        },
        None,
    )
}

/// Deterministic headline among Broken verdicts: smallest reason tag, then
/// ascending `(checker_version, rule_version)`.
fn pick_broken<'a>(brokens: &[&'a Evidence]) -> Option<&'a Evidence> {
    brokens
        .iter()
        .min_by(|a, b| broken_key(a).cmp(&broken_key(b)))
        .copied()
}

/// Deterministic headline among Unknown verdicts: smallest reason tag, then
/// ascending `(checker_version, rule_version)`.
fn pick_unknown<'a>(unknowns: &[&'a Evidence]) -> Option<&'a Evidence> {
    unknowns
        .iter()
        .min_by(|a, b| unknown_key(a).cmp(&unknown_key(b)))
        .copied()
}

fn broken_key(e: &Evidence) -> (&'static str, &str, &str) {
    let tag = match e.outcome() {
        Outcome::Broken { reason } => broken_tag(*reason),
        _ => "",
    };
    (tag, e.checker_version().as_str(), e.rule_version().as_str())
}

fn unknown_key(e: &Evidence) -> (&'static str, &str, &str) {
    let tag = match e.outcome() {
        Outcome::Unknown { reason } => reason.as_tag(),
        _ => "",
    };
    (tag, e.checker_version().as_str(), e.rule_version().as_str())
}

fn broken_tag(reason: BrokenReason) -> &'static str {
    reason.as_tag()
}

/// Reduce per-item outcomes to the observation decision (meet over required items).
fn observation_decision(
    required: &[FrontierItem],
    statuses: &BTreeMap<FrontierItem, Outcome>,
) -> ObservationDecision {
    let mut has_broken = false;
    let mut has_non_accepting = false;
    for item in required {
        match statuses.get(item) {
            Some(o) if o.is_accepting() => {}
            Some(Outcome::Broken { .. }) => {
                has_broken = true;
                has_non_accepting = true;
            }
            // Unknown, or a required item missing from the status map.
            _ => has_non_accepting = true,
        }
    }
    if has_broken {
        ObservationDecision::Broken
    } else if has_non_accepting {
        ObservationDecision::Unknown
    } else {
        ObservationDecision::Accepted
    }
}

/// Stable string key for a frontier item, matching the id form used by the
/// Layer 5 frontier diagnostics (and therefore by pre-check Unknown `item`s).
fn item_key(item: &FrontierItem) -> String {
    match item {
        FrontierItem::Correspondence(id) => id.as_str().to_owned(),
        FrontierItem::BuildEdge(id) => id.as_str().to_owned(),
    }
}
