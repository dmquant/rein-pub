//! Compute-tool contracts (§4): strict surfaces, refusals with words, and
//! arithmetic pinned by hand-computed cases.

use rein_core::time::Timestamp;
use rein_finance::compute::bridge::*;
use rein_finance::compute::comps::*;
use rein_finance::compute::dcf::*;
use rein_finance::compute::odds::*;
use rein_finance::compute::series::*;
use rein_finance::compute::wacc::*;
use rein_finance::frame::{FxRate, PeriodLabel};

fn t(s: &str) -> Timestamp {
    Timestamp::parse(s).unwrap()
}

#[test]
fn dcf_hand_computed_case_with_gordon_terminal() {
    // FCF 100, 110 at r=10%, g=2%: PVs 90.909…, 90.909…; TV = 110·1.02/0.08
    // = 1402.5; PV(TV) = 1402.5/1.21 = 1159.09…
    let out = dcf(&DcfInput {
        fcf: vec![100.0, 110.0],
        discount_rate: 0.10,
        terminal: Terminal::Gordon { growth: 0.02 },
        long_run_growth_reference: None,
    })
    .unwrap();
    assert!((out.pv_fcf[0] - 90.909090).abs() < 1e-4);
    assert!((out.pv_fcf[1] - 90.909090).abs() < 1e-4);
    assert!((out.terminal_value.unwrap() - 1402.5).abs() < 1e-9);
    assert!((out.pv_terminal.unwrap() - 1159.0909).abs() < 1e-3);
    assert!((out.enterprise_value - (90.909090 + 90.909090 + 1159.0909)).abs() < 1e-3);
    // TV dominates: flagged, not hidden.
    assert!(out.tv_share_of_ev.unwrap() > 0.75);
    assert!(out.warnings.iter().any(|w| w.contains("terminal value")));
}

#[test]
fn dcf_strict_surface_refusals() {
    assert!(matches!(
        dcf(&DcfInput {
            fcf: vec![],
            discount_rate: 0.1,
            terminal: Terminal::None,
            long_run_growth_reference: None
        }),
        Err(DcfError::BadSchedule(0))
    ));
    assert!(matches!(
        dcf(&DcfInput {
            fcf: vec![1.0; 31],
            discount_rate: 0.1,
            terminal: Terminal::None,
            long_run_growth_reference: None
        }),
        Err(DcfError::BadSchedule(31))
    ));
    for r in [0.0, 1.0, -0.2, 1.5] {
        assert!(matches!(
            dcf(&DcfInput {
                fcf: vec![1.0],
                discount_rate: r,
                terminal: Terminal::None,
                long_run_growth_reference: None
            }),
            Err(DcfError::BadRate(_))
        ));
    }
    // Gordon needs g < r.
    assert!(matches!(
        dcf(&DcfInput {
            fcf: vec![1.0],
            discount_rate: 0.08,
            terminal: Terminal::Gordon { growth: 0.08 },
            long_run_growth_reference: None
        }),
        Err(DcfError::GordonGrowthNotBelowRate { .. })
    ));
}

#[test]
fn dcf_growth_reference_and_divergence_warnings() {
    let out = dcf(&DcfInput {
        fcf: vec![100.0; 5],
        discount_rate: 0.09,
        terminal: Terminal::Both {
            growth: 0.045,
            terminal_ebitda: 120.0,
            multiple: 8.0,
        },
        long_run_growth_reference: Some(0.04),
    })
    .unwrap();
    assert!(out
        .warnings
        .iter()
        .any(|w| w.contains("long-run nominal growth reference")));
    assert!(out.warnings.iter().any(|w| w.contains("diverge")));
}

#[test]
fn wacc_all_or_none_and_cost_of_equity_naming() {
    let base = WaccInput {
        risk_free: 0.042,
        equity_risk_premium: 0.05,
        beta: Beta {
            value: 1.2,
            source: "capture sha256:…".into(),
            form: BetaForm::Levered,
            relever_target_de: None,
        },
        debt: None,
        weights: None,
    };
    let out = wacc(&base).unwrap();
    assert!((out.cost_of_equity - (0.042 + 1.2 * 0.05)).abs() < 1e-12);
    assert!(
        out.wacc.is_none(),
        "CAPM-only output is cost_of_equity, never wacc"
    );
    assert!(out.warnings.iter().any(|w| w.contains("not a wacc")));

    // Trio without weights: refused (all-or-none).
    let mut partial = base.clone();
    partial.debt = Some(DebtTrio {
        cost_of_debt: 0.05,
        tax_rate: 0.21,
        debt_to_equity: 0.5,
    });
    assert!(matches!(wacc(&partial), Err(WaccError::PartialDebtTrio)));

    // Book weights demand justification.
    let mut book = partial.clone();
    book.weights = Some(Weights {
        equity: 0.8,
        debt: 0.2,
        basis: WeightBasis::Book,
        justification: None,
    });
    assert!(matches!(
        wacc(&book),
        Err(WaccError::BookWeightsUnjustified)
    ));

    // Full declaration computes.
    let mut full = book.clone();
    full.weights.as_mut().unwrap().basis = WeightBasis::Market;
    let out = wacc(&full).unwrap();
    let expect = 0.8 * (0.042 + 1.2 * 0.05) + 0.2 * 0.05 * (1.0 - 0.21);
    assert!((out.wacc.unwrap() - expect).abs() < 1e-12);
}

#[test]
fn wacc_unlevered_beta_relever() {
    let input = WaccInput {
        risk_free: 0.04,
        equity_risk_premium: 0.05,
        beta: Beta {
            value: 0.9,
            source: "peer set".into(),
            form: BetaForm::Unlevered,
            relever_target_de: Some(0.4),
        },
        debt: Some(DebtTrio {
            cost_of_debt: 0.05,
            tax_rate: 0.25,
            debt_to_equity: 0.4,
        }),
        weights: Some(Weights {
            equity: 0.75,
            debt: 0.25,
            basis: WeightBasis::Market,
            justification: None,
        }),
    };
    let out = wacc(&input).unwrap();
    let relevered = 0.9 * (1.0 + 0.75 * 0.4);
    assert!((out.beta_used - relevered).abs() < 1e-12);

    // Unlevered without a trio cannot relever: refused.
    let mut lone = input.clone();
    lone.debt = None;
    lone.weights = None;
    assert!(matches!(wacc(&lone), Err(WaccError::UnleveredWithoutTrio)));
}

#[test]
fn bridge_arithmetic_and_share_count_discipline() {
    let out = bridge(&BridgeInput {
        enterprise_value: 1000.0,
        net_debt: DatedValue {
            value: 200.0,
            as_of: t("2026-06-30T00:00:00Z"),
        },
        minority_interest: 50.0,
        associates: 30.0,
        other_claims: 10.0,
        share_count: ShareCount {
            value: 100.0,
            method: ShareCountMethod::Diluted,
            as_of: t("2026-06-30T00:00:00Z"),
        },
    })
    .unwrap();
    assert!((out.equity_value - 770.0).abs() < 1e-12);
    assert!((out.per_share - 7.7).abs() < 1e-12);

    assert!(matches!(
        bridge(&BridgeInput {
            enterprise_value: 1.0,
            net_debt: DatedValue {
                value: 0.0,
                as_of: t("2026-06-30T00:00:00Z")
            },
            minority_interest: 0.0,
            associates: 0.0,
            other_claims: 0.0,
            share_count: ShareCount {
                value: 0.0,
                method: ShareCountMethod::Basic,
                as_of: t("2026-06-30T00:00:00Z")
            },
        }),
        Err(BridgeError::BadShareCount(_))
    ));
}

#[test]
fn comps_frame_refusals_and_counted_exclusions() {
    let peer = |name: &str, num: f64, den: f64, ccy: &str, period: PeriodLabel| Peer {
        name: name.into(),
        numerator: num,
        denominator: den,
        currency: ccy.into(),
        period,
    };
    // Cross-currency without FX: refused with the peer named.
    let refused = comps(&CompsInput {
        level: MultipleLevel::EnterpriseValue,
        multiple_name: "EV/EBITDA".into(),
        peers: vec![
            peer("a", 100.0, 10.0, "USD", PeriodLabel::Ltm),
            peer("b", 900.0, 100.0, "HKD", PeriodLabel::Ltm),
        ],
        target_metric: 12.0,
        target_currency: "USD".into(),
        target_period: PeriodLabel::Ltm,
        fx: vec![],
    });
    assert!(matches!(
        refused,
        Err(CompsError::CrossCurrencyWithoutFx { .. })
    ));

    // LTM/NTM mixing: refused.
    let mixed = comps(&CompsInput {
        level: MultipleLevel::EnterpriseValue,
        multiple_name: "EV/EBITDA".into(),
        peers: vec![
            peer("a", 100.0, 10.0, "USD", PeriodLabel::Ltm),
            peer("b", 90.0, 10.0, "USD", PeriodLabel::Ntm),
        ],
        target_metric: 12.0,
        target_currency: "USD".into(),
        target_period: PeriodLabel::Ltm,
        fx: vec![],
    });
    assert!(matches!(mixed, Err(CompsError::PeriodMix { .. })));

    // Negative denominators: excluded AND counted, never silent.
    let out = comps(&CompsInput {
        level: MultipleLevel::EnterpriseValue,
        multiple_name: "EV/EBITDA".into(),
        peers: vec![
            peer("a", 100.0, 10.0, "USD", PeriodLabel::Ltm),
            peer("loss-maker", 80.0, -5.0, "USD", PeriodLabel::Ltm),
            peer("c", 240.0, 20.0, "USD", PeriodLabel::Ltm),
            peer("hk", 780.0, 78.0, "HKD", PeriodLabel::Ltm),
        ],
        target_metric: 12.0,
        target_currency: "USD".into(),
        target_period: PeriodLabel::Ltm,
        fx: vec![FxRate {
            from: "HKD".into(),
            to: "USD".into(),
            rate: 0.128,
            as_of: t("2026-08-18T00:00:00Z"),
        }],
    })
    .unwrap();
    assert_eq!(out.eligible, 4);
    assert_eq!(out.used, 3);
    assert_eq!(out.excluded.len(), 1);
    assert!(out.excluded[0]
        .reason
        .contains("negative or zero denominator"));
    // Median of {10, 12, 10} = 10 → implied EV 120.
    assert!((out.median_multiple - 10.0).abs() < 1e-12);
    assert!((out.implied_value_median - 120.0).abs() < 1e-12);
}

#[test]
fn series_diff_is_moved_only() {
    let prior = DriverSeries {
        subject: "security:nvda".into(),
        metric: "dc_revenue".into(),
        points: vec![
            DriverPoint {
                as_of: t("2026-06-30T00:00:00Z"),
                value: 100.0,
                unit: "ccy".into(),
            },
            DriverPoint {
                as_of: t("2026-07-31T00:00:00Z"),
                value: 110.0,
                unit: "ccy".into(),
            },
        ],
    };
    let mut new = prior.clone();
    new.points[1].value = 115.0; // moved
    new.points.push(DriverPoint {
        as_of: t("2026-08-18T00:00:00Z"),
        value: 120.0,
        unit: "ccy".into(),
    }); // inserted — NOT a movement
    let d = diff(&prior, &new);
    assert_eq!(d.moved.len(), 1, "a row inserted is not a value changed");
    assert_eq!(d.moved[0].prior, 110.0);
    assert_eq!(d.moved[0].new, 115.0);
    assert_eq!(d.inserted.len(), 1);
    assert!(d.removed.is_empty());
}

#[test]
fn odds_edge_and_window() {
    let out = edge(&EdgeInput {
        p_house: 0.62,
        p_market: 0.50,
        window: SettleWindow {
            opens: t("2026-08-18T00:00:00Z"),
            closes: t("2026-12-31T00:00:00Z"),
        },
    })
    .unwrap();
    assert!((out.edge - 0.12).abs() < 1e-12);
    assert!((out.kelly_fraction - 0.24).abs() < 1e-12);

    assert!(matches!(
        edge(&EdgeInput {
            p_house: 1.0,
            p_market: 0.5,
            window: SettleWindow {
                opens: t("2026-08-18T00:00:00Z"),
                closes: t("2026-12-31T00:00:00Z")
            },
        }),
        Err(OddsError::BadProbability { .. })
    ));
}
