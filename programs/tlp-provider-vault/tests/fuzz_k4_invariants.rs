
#![allow(clippy::needless_range_loop)]

use proptest::prelude::*;
use tlp_provider_vault::{
    accrue_affiliate_amount, accrue_earmarks, advance_hwm_on_drain, compute_net_ggr,
    compute_swap_credit, compute_weighted_lp_bps, effective_accrual_base, phase_split_bps,
    provider_period_fee_step, require_earmark_invariant, sum_earmarks, AssetPool, MAX_DEV_FEE_BPS,
    MAX_PROVIDER_FEE_BPS, SOVEREIGN_CARVE_BPS,
};


struct Model {
    pool: AssetPool,
    holder_balance: u64,
    base_accrued_since_hwm: u64,
}

fn make_pool(lp_tokens_by_tier: [u64; 5], fb_in_window: u64) -> AssetPool {
    AssetPool {
        vault_config: Default::default(),
        asset_mint: Default::default(),
        is_sol: false,
        bump: 255,
        lp_mint: Default::default(),
        lp_supply: 0,
        cumulative_gross_ggr: 0,
        last_distributed_gross_ggr: 0,
        last_distributed_at: 0,
        pending_dev_fee: 0,
        pending_provider_fee: 0,
        pending_affiliate: 0,
        pending_sovereign: 0,
        pending_yield: 0,
        pending_reserve: 0,
        last_distributed_affiliate: 0,
        pending_promo: 0,
        lp_share_bps: 0,
        lp_tokens_by_tier,
        peak_vault: 0,
        peak_vault_at: 0,
        circuit_state: 0,
        red_entered_at: 0,
        waiver_active: false,
        waiver_started_at: 0,
        waiver_max_until: 0,
        insurance_balance: 0,
        withdraw_batch_counter: 0,
        last_batch_opened_at: 0,
        pending_request_count: 0,
        vault_locked: false,
        vault_locked_at: 0,
        provider_settle_owner: Default::default(),
        pending_settle_owner: Default::default(),
        pending_settle_owner_unlocks_at: 0,
        provider_owed_total: 0,
        founding_banker_lp_tokens_in_window: fb_in_window,
        max_chip_debit_per_24h_per_wallet: 0,
        promo_paid_unreconciled: 0,
        network_reimbursement_owed: 0,
        provider_credit: 0,
        vault_holder: Default::default(),
        pending_reset_peak: 0,
        pending_reset_peak_unlocks_at: 0,
        affiliate_unreconciled: 0,
        reserved: [0u8; 24],
    }
}

impl Model {
    fn new(lp_tokens_by_tier: [u64; 5], fb_in_window: u64, seed_balance: u64) -> Self {
        Model {
            pool: make_pool(lp_tokens_by_tier, fb_in_window),
            holder_balance: seed_balance,
            base_accrued_since_hwm: 0,
        }
    }

    fn profit_above_water_line(&self) -> u64 {
        let water_line = (self.pool.last_distributed_gross_ggr as i128).max(0);
        let d = (self.pool.cumulative_gross_ggr as i128) - water_line;
        if d > 0 {
            d as u64
        } else {
            0
        }
    }

    fn earmarks(&self) -> u64 {
        sum_earmarks(&self.pool)
    }

    fn k4_holds(&self) -> bool {
        require_earmark_invariant(&self.pool, self.holder_balance).is_ok()
    }
}


#[derive(Debug, Clone)]
enum Op {
    SubmitGgr {
        gross_wager: u64,
        gross_payout: u64,
        phase: u8,
        provider_fee_bps: u16,
        dev_fee_bps: u16,
    },
    AccrueAffiliate { amount: u64 },
    TopUpPromo { amount: u64 },
    CreditPromo { amount: u64 },
    FlushProviderFee { amount: u64 },
    SettleProviderInvoice { amount: u64 },
    DistributeDevFee,
    DistributeSovereign,
    DistributeYield,
    DistributeReserve,
    DistributeAffiliate,
    ChipDeposit { amount: u64 },
    ChipWithdraw { amount: u64 },
}

impl Model {
    fn apply(&mut self, op: &Op) {
        match op {
            Op::SubmitGgr {
                gross_wager,
                gross_payout,
                phase,
                provider_fee_bps,
                dev_fee_bps,
            } => {
                if *dev_fee_bps > MAX_DEV_FEE_BPS {
                    return;
                }
                let net = match compute_net_ggr(*gross_wager, *gross_payout) {
                    Ok(n) => n,
                    Err(_) => return,
                };
                let fee_due: u64 = if net > 0 {
                    ((net as u128) * (*provider_fee_bps as u128) / 10_000u128) as u64
                } else {
                    0
                };

                let mut trial = clone_pool(&self.pool);

                let cum_before = trial.cumulative_gross_ggr;
                trial.cumulative_gross_ggr = match cum_before.checked_add(net) {
                    Some(v) => v,
                    None => return,
                };
                let accrual_base_signed = match effective_accrual_base(
                    trial.last_distributed_gross_ggr,
                    cum_before,
                    net,
                ) {
                    Ok(v) => v,
                    Err(_) => return,
                };

                let after_provider_for_net: u64 = if net > 0 {
                    (accrual_base_signed as u64).saturating_sub(fee_due)
                } else {
                    0
                };
                let own_promo = trial
                    .promo_paid_unreconciled
                    .saturating_sub(trial.network_reimbursement_owed);
                let promo_to_net = own_promo.min(after_provider_for_net);
                let remaining_base = after_provider_for_net.saturating_sub(promo_to_net);
                let affiliate_to_net = trial.affiliate_unreconciled.min(remaining_base);
                let cost_netted = match promo_to_net.checked_add(affiliate_to_net) {
                    Some(v) => v,
                    None => return,
                };

                if accrue_earmarks(
                    &mut trial,
                    accrual_base_signed,
                    *phase,
                    *provider_fee_bps,
                    fee_due,
                    *dev_fee_bps,
                    cost_netted,
                    0,
                )
                .is_err()
                {
                    return;
                }
                trial.promo_paid_unreconciled = match trial
                    .promo_paid_unreconciled
                    .checked_sub(promo_to_net)
                {
                    Some(v) => v,
                    None => return,
                };
                trial.affiliate_unreconciled = match trial
                    .affiliate_unreconciled
                    .checked_sub(affiliate_to_net)
                {
                    Some(v) => v,
                    None => return,
                };
                if require_earmark_invariant(&trial, self.holder_balance).is_err() {
                    return;
                }

                self.pool = trial;
                if net > 0 {
                    self.base_accrued_since_hwm = self
                        .base_accrued_since_hwm
                        .saturating_add(accrual_base_signed as u64);
                } else {
                    self.base_accrued_since_hwm = self
                        .base_accrued_since_hwm
                        .saturating_sub(net.unsigned_abs());
                }
            }

            Op::AccrueAffiliate { amount } => {
                let mut trial = clone_pool(&self.pool);
                if accrue_affiliate_amount(&mut trial, *amount).is_err() {
                    return;
                }
                if require_earmark_invariant(&trial, self.holder_balance).is_err() {
                    return;
                }
                self.pool = trial;
            }

            Op::TopUpPromo { amount } => {
                let new_promo = match self.pool.pending_promo.checked_add(*amount) {
                    Some(v) => v,
                    None => return,
                };
                let new_holder = match self.holder_balance.checked_add(*amount) {
                    Some(v) => v,
                    None => return,
                };
                self.pool.pending_promo = new_promo;
                self.holder_balance = new_holder;
            }

            Op::CreditPromo { amount } => {
                if self.pool.pending_promo < *amount {
                    return;
                }
                let new_promo = self.pool.pending_promo - *amount;
                let new_holder = match self.holder_balance.checked_sub(*amount) {
                    Some(v) => v,
                    None => return,
                };
                let mut trial = clone_pool(&self.pool);
                trial.pending_promo = new_promo;
                if require_earmark_invariant(&trial, new_holder).is_err() {
                    return;
                }
                self.pool.pending_promo = new_promo;
                self.holder_balance = new_holder;
            }

            Op::FlushProviderFee { amount } => {
                if self.pool.pending_provider_fee < *amount {
                    return;
                }
                let owed = match self.pool.provider_owed_total.checked_add(*amount) {
                    Some(v) => v,
                    None => return,
                };
                self.pool.pending_provider_fee -= *amount;
                self.pool.provider_owed_total = owed;
            }

            Op::SettleProviderInvoice { amount } => {
                if self.pool.provider_owed_total < *amount {
                    return;
                }
                let new_holder = match self.holder_balance.checked_sub(*amount) {
                    Some(v) => v,
                    None => return,
                };
                self.pool.provider_owed_total -= *amount;
                self.holder_balance = new_holder;
            }

            Op::DistributeDevFee => self.drain(EarmarkBucket::DevFee),
            Op::DistributeSovereign => self.drain(EarmarkBucket::Sovereign),
            Op::DistributeYield => self.drain(EarmarkBucket::Yield),
            Op::DistributeReserve => self.drain(EarmarkBucket::Reserve),
            Op::DistributeAffiliate => self.drain(EarmarkBucket::Affiliate),

            Op::ChipDeposit { amount } => {
                if let Some(v) = self.holder_balance.checked_add(*amount) {
                    self.holder_balance = v;
                }
            }

            Op::ChipWithdraw { amount } => {
                let nav = self.holder_balance.saturating_sub(self.earmarks());
                let amt = (*amount).min(nav);
                self.holder_balance -= amt;
            }
        }
    }

    fn drain(&mut self, bucket: EarmarkBucket) {
        let amount = match bucket {
            EarmarkBucket::DevFee => self.pool.pending_dev_fee,
            EarmarkBucket::Sovereign => self.pool.pending_sovereign,
            EarmarkBucket::Yield => self.pool.pending_yield,
            EarmarkBucket::Reserve => self.pool.pending_reserve,
            EarmarkBucket::Affiliate => self.pool.pending_affiliate,
        };
        if amount == 0 {
            return;
        }
        let new_holder = match self.holder_balance.checked_sub(amount) {
            Some(v) => v,
            None => return,
        };
        match bucket {
            EarmarkBucket::DevFee => self.pool.pending_dev_fee = 0,
            EarmarkBucket::Sovereign => self.pool.pending_sovereign = 0,
            EarmarkBucket::Yield => self.pool.pending_yield = 0,
            EarmarkBucket::Reserve => self.pool.pending_reserve = 0,
            EarmarkBucket::Affiliate => self.pool.pending_affiliate = 0,
        }
        advance_hwm_on_drain(&mut self.pool);
        self.base_accrued_since_hwm = 0;
        self.holder_balance = new_holder;
    }
}

enum EarmarkBucket {
    DevFee,
    Sovereign,
    Yield,
    Reserve,
    Affiliate,
}

fn clone_pool(pool: &AssetPool) -> AssetPool {
    pool.clone()
}


const MAX_AMT: u64 = 1_000_000_000_000_000;

fn arb_amount() -> impl Strategy<Value = u64> {
    prop_oneof![
        Just(0u64),
        Just(1u64),
        Just(9_999u64),
        0u64..10_000u64,
        0u64..MAX_AMT,
        Just(MAX_AMT),
    ]
}

fn arb_op() -> impl Strategy<Value = Op> {
    prop_oneof![
        6 => (arb_amount(), arb_amount(), 0u8..2u8, 0u16..2_000u16, 0u16..1_200u16).prop_map(
            |(gross_wager, gross_payout, phase, provider_fee_bps, dev_fee_bps)| Op::SubmitGgr {
                gross_wager,
                gross_payout,
                phase,
                provider_fee_bps,
                dev_fee_bps,
            }
        ),
        1 => arb_amount().prop_map(|amount| Op::AccrueAffiliate { amount }),
        1 => arb_amount().prop_map(|amount| Op::TopUpPromo { amount }),
        1 => arb_amount().prop_map(|amount| Op::CreditPromo { amount }),
        1 => arb_amount().prop_map(|amount| Op::FlushProviderFee { amount }),
        1 => arb_amount().prop_map(|amount| Op::SettleProviderInvoice { amount }),
        1 => Just(Op::DistributeDevFee),
        1 => Just(Op::DistributeSovereign),
        1 => Just(Op::DistributeYield),
        1 => Just(Op::DistributeReserve),
        1 => Just(Op::DistributeAffiliate),
        1 => arb_amount().prop_map(|amount| Op::ChipDeposit { amount }),
        1 => arb_amount().prop_map(|amount| Op::ChipWithdraw { amount }),
    ]
}

fn arb_initial_hwm() -> impl Strategy<Value = i64> {
    prop_oneof![
        3 => Just(0i64),
        3 => -2_000_000_000i64..0i64,
        1 => Just(-1_851_644_605i64),
        2 => 0i64..1_000_000_000_000i64,
    ]
}

fn arb_tier_tokens() -> impl Strategy<Value = [u64; 5]> {
    let ranges: [std::ops::Range<u64>; 5] = std::array::from_fn(|_| 0u64..1_000_000_000u64);
    ranges.prop_map(|v| [v[0], v[1], v[2], v[3], v[4]])
}


proptest! {
    #![proptest_config(ProptestConfig {
        cases: 4096,
        max_shrink_iters: 100_000,
        .. ProptestConfig::default()
    })]

    #[test]
    fn k4_and_monotonicity_hold_over_random_sequences(
        tier_tokens in arb_tier_tokens(),
        fb_in_window in 0u64..2_000_000_000u64,
        seed_balance in 0u64..MAX_AMT,
        initial_hwm in arb_initial_hwm(),
        ops in proptest::collection::vec(arb_op(), 1..400),
    ) {
        let mut m = Model::new(tier_tokens, fb_in_window, seed_balance);
        m.pool.last_distributed_gross_ggr = initial_hwm;

        prop_assert!(m.k4_holds(), "fresh pool violated K4");

        for (i, op) in ops.iter().enumerate() {
            let before_earmarks = m.earmarks();
            let before_holder = m.holder_balance;

            m.apply(op);

            prop_assert!(
                m.k4_holds(),
                "K4 VIOLATED after op #{i} {:?}\n  holder={}  earmarks={}\n  (before: holder={} earmarks={})",
                op, m.holder_balance, m.earmarks(), before_holder, before_earmarks
            );

            let s = m.pool.pending_dev_fee as u128
                + m.pool.pending_provider_fee as u128
                + m.pool.pending_affiliate as u128
                + m.pool.pending_sovereign as u128
                + m.pool.pending_yield as u128
                + m.pool.pending_reserve as u128
                + m.pool.pending_promo as u128
                + m.pool.provider_owed_total as u128;
            prop_assert!(
                s == m.earmarks() as u128,
                "sum_earmarks() disagrees with manual u128 sum after op #{i} {:?}: \
                 fn={} manual={}",
                op, m.earmarks(), s
            );
            prop_assert!(
                s <= u64::MAX as u128,
                "earmark counters overflowed u64 (wraparound) after op #{i} {:?}", op
            );
        }
    }

    #[test]
    fn hwm_accrual_never_exceeds_profit_above_the_water_line(
        tier_tokens in arb_tier_tokens(),
        fb_in_window in 0u64..2_000_000_000u64,
        seed_balance in 0u64..MAX_AMT,
        initial_hwm in arb_initial_hwm(),
        ops in proptest::collection::vec(arb_op(), 1..400),
    ) {
        let mut m = Model::new(tier_tokens, fb_in_window, seed_balance);
        m.pool.last_distributed_gross_ggr = initial_hwm;
        prop_assert_eq!(m.base_accrued_since_hwm, m.profit_above_water_line());

        for (i, op) in ops.iter().enumerate() {
            let stored_hwm_before = m.pool.last_distributed_gross_ggr;
            let water_line_before = stored_hwm_before.max(0);
            m.apply(op);

            prop_assert!(
                m.base_accrued_since_hwm <= m.profit_above_water_line(),
                "HWM LEAK after op #{} {:?}: accrued base {} exceeds profit above the \
                 water line {} (cum={}, hwm={}) — protocol-external dollars would be \
                 earmarked against a drawdown that was already forgiven",
                i, op, m.base_accrued_since_hwm, m.profit_above_water_line(),
                m.pool.cumulative_gross_ggr, m.pool.last_distributed_gross_ggr
            );
            prop_assert_eq!(
                m.base_accrued_since_hwm,
                m.profit_above_water_line(),
                "HWM drift after op #{} {:?} (cum={}, hwm={})",
                i, op, m.pool.cumulative_gross_ggr, m.pool.last_distributed_gross_ggr
            );
            prop_assert!(
                m.pool.last_distributed_gross_ggr.max(0) >= water_line_before,
                "water line went BACKWARD after op #{} {:?}: {} -> {}. Lowering the mark \
                 re-opens the already-distributed range for re-accrual.",
                i, op, water_line_before, m.pool.last_distributed_gross_ggr.max(0)
            );
            if m.pool.last_distributed_gross_ggr != stored_hwm_before {
                prop_assert!(
                    m.pool.last_distributed_gross_ggr >= 0,
                    "op #{} {:?} wrote a NEGATIVE bookmark ({}) — advance_hwm_on_drain must \
                     normalize the pre-upgrade skip artifact to 0",
                    i, op, m.pool.last_distributed_gross_ggr
                );
            }
        }
    }

    #[test]
    fn waterfall_conserves_value_per_receipt(
        gross_wager in 0u64..MAX_AMT,
        gross_payout in 0u64..MAX_AMT,
        phase in 0u8..2u8,
        provider_fee_bps in 0u16..2_000u16,
        dev_fee_bps in 0u16..(MAX_DEV_FEE_BPS + 1),
        tier_tokens in arb_tier_tokens(),
        fb_in_window in 0u64..2_000_000_000u64,
    ) {
        let net = match compute_net_ggr(gross_wager, gross_payout) {
            Ok(n) => n,
            Err(_) => return Ok(()),
        };
        if net <= 0 {
            return Ok(());
        }

        let mut pool = make_pool(tier_tokens, fb_in_window);
        let fee_due: u64 = ((net as u128) * (provider_fee_bps as u128) / 10_000u128) as u64;

        accrue_earmarks(&mut pool, net, phase, provider_fee_bps, fee_due, dev_fee_bps, 0, 0)
            .expect("positive accrual within bounds must not error");
        let got_provider = pool.pending_provider_fee;
        let got_dev = pool.pending_dev_fee;
        let got_sov = pool.pending_sovereign;
        let got_yield = pool.pending_yield;
        let got_reserve = pool.pending_reserve;

        let r = reference_waterfall(&pool, net as u64, fee_due, phase, dev_fee_bps);

        prop_assert_eq!(got_provider, r.provider_fee, "provider_fee bucket drift");
        prop_assert_eq!(got_dev, r.dev_fee, "dev_fee bucket drift");
        prop_assert_eq!(got_sov, r.sovereign, "sovereign bucket drift");
        prop_assert_eq!(got_yield, r.yield_b, "yield bucket drift");
        prop_assert_eq!(got_reserve, r.reserve, "reserve bucket drift");

        let exp_sov = (r.protocol_due as u128 * SOVEREIGN_CARVE_BPS as u128 / 10_000u128) as u64;
        prop_assert_eq!(r.sovereign, exp_sov, "sovereign carve != 5% of protocol_due");

        let total = r.provider_fee
            + r.dev_fee
            + r.lp_due
            + r.sovereign
            + r.yield_b
            + r.compound
            + r.reserve;
        prop_assert_eq!(
            total, net as u64,
            "VALUE NOT CONSERVED: net={} buckets sum to {} \
             (provider={} dev={} lp={} sov={} yield={} compound={} reserve={})",
            net, total, r.provider_fee, r.dev_fee, r.lp_due, r.sovereign, r.yield_b, r.compound, r.reserve
        );
    }

    #[test]
    fn negative_delta_unwind_never_wraps_or_grows(
        seed_dev in 0u64..MAX_AMT,
        seed_sov in 0u64..MAX_AMT,
        seed_yield in 0u64..MAX_AMT,
        seed_reserve in 0u64..MAX_AMT,
        loss in 1u64..MAX_AMT,
        phase in 0u8..2u8,
        dev_fee_bps in 0u16..(MAX_DEV_FEE_BPS + 1),
        tier_tokens in arb_tier_tokens(),
    ) {
        prop_assume!(dev_fee_bps <= MAX_DEV_FEE_BPS);
        let mut pool = make_pool(tier_tokens, 0);
        pool.pending_dev_fee = seed_dev;
        pool.pending_sovereign = seed_sov;
        pool.pending_yield = seed_yield;
        pool.pending_reserve = seed_reserve;

        let before = [seed_dev, seed_sov, seed_yield, seed_reserve];

        let net_neg = -(loss as i64);
        accrue_earmarks(&mut pool, net_neg, phase, 0, 0, dev_fee_bps, 0, 0)
            .expect("negative unwind must not error (saturating)");

        let after = [
            pool.pending_dev_fee,
            pool.pending_sovereign,
            pool.pending_yield,
            pool.pending_reserve,
        ];
        for k in 0..4 {
            prop_assert!(
                after[k] <= before[k],
                "unwind INCREASED counter {}: before={} after={}",
                k, before[k], after[k]
            );
        }
    }

    #[test]
    fn swap_credit_is_delta_bound_and_guarded(
        holder in 0u64..MAX_AMT,
        escrow in 0u64..MAX_AMT,
        floor in 0u64..MAX_AMT,
    ) {
        let res = compute_swap_credit(holder, escrow, floor);
        if holder < escrow {
            prop_assert!(res.is_err(), "credit must reject holder<escrow (would decrease chips)");
        } else {
            let delta = holder - escrow;
            if floor > 0 && delta < floor {
                prop_assert!(res.is_err(), "credit must reject delta<floor");
            } else {
                prop_assert_eq!(res.unwrap(), delta, "credit must equal measured holder-escrow delta");
            }
        }
    }

    #[test]
    fn period_fee_tracks_period_net_over_random_receipt_sequences(
        receipts in prop::collection::vec(-(MAX_AMT as i64)..(MAX_AMT as i64), 1..24),
        bps in 0u16..(MAX_PROVIDER_FEE_BPS + 1),
    ) {
        let mut period_net: i64 = 0;
        let mut charged: u64 = 0;
        let mut pool_mirror: u64 = 0;
        let mut provider_mirror: u64 = 0;

        for r in &receipts {
            let step = provider_period_fee_step(period_net, *r, charged, bps)
                .expect("no overflow within MAX_AMT×24");

            prop_assert!(
                step.increase == 0 || step.decrease == 0,
                "increase {} and decrease {} must be mutually exclusive",
                step.increase, step.decrease
            );
            prop_assert!(
                step.decrease <= charged,
                "decrease {} exceeded period_fee_charged {} — would desync mirrors",
                step.decrease, charged
            );

            pool_mirror = pool_mirror
                .checked_add(step.increase).expect("pool mirror add")
                .checked_sub(step.decrease).expect("pool mirror sub must not underflow");
            provider_mirror = provider_mirror
                .checked_add(step.increase).expect("provider mirror add")
                .checked_sub(step.decrease).expect("provider mirror sub must not underflow");
            prop_assert_eq!(pool_mirror, provider_mirror, "mirrors must move in lockstep");

            period_net = step.period_net_after;
            charged = step.fee_target;
            prop_assert_eq!(charged, pool_mirror, "the mirrors ARE the period charge");
        }

        let expected: u64 = if period_net > 0 {
            ((period_net as u128) * bps as u128 / 10_000u128) as u64
        } else {
            0
        };
        prop_assert_eq!(
            charged, expected,
            "period fee must be max(0, period_net={}) × {}bps, not the sum of winning days",
            period_net, bps
        );
        if period_net <= 0 {
            prop_assert_eq!(charged, 0u64, "a net-negative period must owe zero");
        }
        prop_assert!(
            (charged as u128) <= (period_net.max(0) as u128),
            "fee {} exceeded the period's net profit {}",
            charged, period_net
        );
    }

    #[test]
    fn falling_provider_fee_never_grows_sum_earmarks(
        seed_dev in 0u64..MAX_AMT,
        seed_sov in 0u64..MAX_AMT,
        seed_yield in 0u64..MAX_AMT,
        seed_reserve in 0u64..MAX_AMT,
        seed_affiliate in 0u64..MAX_AMT,
        seed_promo in 0u64..MAX_AMT,
        seed_owed_total in 0u64..MAX_AMT,
        seed_provider_fee in 0u64..MAX_AMT,
        decrease in 0u64..MAX_AMT,
        tier_tokens in arb_tier_tokens(),
    ) {
        let mut pool = make_pool(tier_tokens, 0);
        pool.pending_dev_fee = seed_dev;
        pool.pending_sovereign = seed_sov;
        pool.pending_yield = seed_yield;
        pool.pending_reserve = seed_reserve;
        pool.pending_affiliate = seed_affiliate;
        pool.pending_promo = seed_promo;
        pool.provider_owed_total = seed_owed_total;
        pool.pending_provider_fee = seed_provider_fee;

        let before = sum_earmarks(&pool);
        pool.pending_provider_fee = pool.pending_provider_fee.saturating_sub(decrease);
        let after = sum_earmarks(&pool);

        prop_assert!(after <= before, "a fee DECREASE grew sum_earmarks: {} -> {}", before, after);
        prop_assert!(
            require_earmark_invariant(&pool, before).is_ok(),
            "the tightest pre-reduction-solvent holder ({}) must remain solvent \
             after the fee reduction (post-sum {})",
            before, after
        );
    }
}


fn earmark_setters() -> Vec<(&'static str, fn(&mut AssetPool, u64))> {
    vec![
        ("pending_dev_fee", |p, v| p.pending_dev_fee = v),
        ("pending_provider_fee", |p, v| p.pending_provider_fee = v),
        ("pending_affiliate", |p, v| p.pending_affiliate = v),
        ("pending_sovereign", |p, v| p.pending_sovereign = v),
        ("pending_yield", |p, v| p.pending_yield = v),
        ("pending_reserve", |p, v| p.pending_reserve = v),
        ("pending_promo", |p, v| p.pending_promo = v),
        ("provider_owed_total", |p, v| p.provider_owed_total = v),
    ]
}

#[test]
fn sum_earmarks_totals_every_committed_earmark_field() {
    let setters = earmark_setters();

    let mut pool = make_pool([0; 5], 0);
    let mut expected: u64 = 0;
    for (i, (_label, set)) in setters.iter().enumerate() {
        let v = (i as u64 + 1) * 1_000_000;
        set(&mut pool, v);
        expected = expected.checked_add(v).unwrap();
    }
    assert_eq!(
        sum_earmarks(&pool),
        expected,
        "sum_earmarks dropped or double-counted a committed earmark field",
    );

    for (label, set) in setters.iter() {
        let mut p = make_pool([0; 5], 0);
        set(&mut p, 777_000);
        assert_eq!(
            sum_earmarks(&p),
            777_000,
            "sum_earmarks does not include `{label}` — a committed earmark is missing from the K4 sum",
        );
    }
}

#[test]
fn sum_earmarks_excludes_ngr_receivables() {
    let mut pool = make_pool([0; 5], 0);
    pool.promo_paid_unreconciled = 1_000_000;
    pool.network_reimbursement_owed = 2_000_000;
    pool.provider_credit = 3_000_000;
    pool.affiliate_unreconciled = 4_000_000;
    assert_eq!(
        sum_earmarks(&pool),
        0,
        "an NGR receivable / netting tracker was wrongly folded into sum_earmarks — NAV would be understated",
    );

    let mut pool2 = make_pool([0; 5], 0);
    pool2.pending_affiliate = 4_000_000;
    pool2.affiliate_unreconciled = 4_000_000;
    assert_eq!(
        sum_earmarks(&pool2),
        4_000_000,
        "affiliate must be reserved exactly once (pending_affiliate), not double-counted with the tracker",
    );
}

#[test]
fn affiliate_netting_matches_reduced_base_and_conserves() {
    let g: u64 = 100_000_000_000;
    let a: u64 = 1_000_000_000;
    let phase = 1u8;
    let dev_bps = 250u16;
    let fee = 0u64;

    let protocol_drain =
        |p: &AssetPool| -> u64 { p.pending_dev_fee + p.pending_sovereign + p.pending_yield + p.pending_reserve };

    let whale: [u64; 5] = [0, 0, 0, 0, 1_000_000];
    let mut base = make_pool(whale, 0);
    accrue_earmarks(&mut base, g as i64, phase, 0, fee, dev_bps, 0, 0).unwrap();
    let base_protocol = protocol_drain(&base);
    let base_lp = g - base_protocol;

    let mut p = make_pool(whale, 0);
    accrue_affiliate_amount(&mut p, a).unwrap();
    assert_eq!(p.pending_affiliate, a);
    assert_eq!(p.affiliate_unreconciled, a);
    let cost_netted = a.min(g - fee);
    accrue_earmarks(&mut p, g as i64, phase, 0, fee, dev_bps, cost_netted, 0).unwrap();
    p.affiliate_unreconciled -= cost_netted;

    let r = reference_waterfall(&p, g - a, fee, phase, dev_bps);
    assert_eq!(p.pending_dev_fee, r.dev_fee, "dev on reduced base");
    assert_eq!(p.pending_sovereign, r.sovereign, "sovereign on reduced base");
    assert_eq!(p.pending_yield, r.yield_b, "yield on reduced base");
    assert_eq!(p.pending_reserve, r.reserve, "reserve on reduced base");

    assert_eq!(p.pending_affiliate, a, "payout reservation untouched by netting");
    assert_eq!(p.affiliate_unreconciled, 0, "netting tracker fully consumed");

    let with_protocol = protocol_drain(&p);
    let with_lp = g - with_protocol - p.pending_affiliate;
    assert_eq!(with_protocol + a + with_lp, g, "affiliate cycle MUST conserve GGR exactly");

    let lp_cost = base_lp - with_lp;
    let protocol_cost = base_protocol - with_protocol;
    assert_eq!(lp_cost + protocol_cost, a, "affiliate cost splits exactly between LP and protocol");
    assert!(lp_cost > 0 && lp_cost < a, "LP bears a SHARE of affiliate — not 0 (today's earmark) and not 100%");
    assert!(protocol_cost > 0, "protocol bears a share of affiliate (the World-2 shift)");
    assert!(lp_cost > protocol_cost, "LP is the majority bearer (~85% vs ~15%)");

    let lp_pct_bps = (lp_cost as u128) * 10_000 / (a as u128);
    assert!(
        (7_500..=9_500).contains(&(lp_pct_bps as u64)),
        "LP-bearing {lp_pct_bps} bps outside the expected ~75-95% World-2 band",
    );
}

struct RefWaterfall {
    provider_fee: u64,
    dev_fee: u64,
    lp_due: u64,
    protocol_due: u64,
    sovereign: u64,
    yield_b: u64,
    compound: u64,
    reserve: u64,
}

fn reference_waterfall(
    pool: &AssetPool,
    net: u64,
    fee_due: u64,
    phase: u8,
    dev_fee_bps: u16,
) -> RefWaterfall {
    let weighted_lp_bps =
        compute_weighted_lp_bps(pool, phase, pool.founding_banker_lp_tokens_in_window).unwrap();

    let after_provider = net - fee_due;
    let dev_fee = (after_provider as u128 * dev_fee_bps as u128 / 10_000u128) as u64;
    let after_dev = after_provider - dev_fee;
    let lp_due = (after_dev as u128 * weighted_lp_bps as u128 / 10_000u128) as u64;
    let protocol_due = after_dev - lp_due;
    let sovereign = (protocol_due as u128 * SOVEREIGN_CARVE_BPS as u128 / 10_000u128) as u64;
    let tax_remainder = protocol_due - sovereign;
    let (yield_bps, compound_bps, _) = phase_split_bps(phase);
    let yield_b = (tax_remainder as u128 * yield_bps as u128 / 10_000u128) as u64;
    let compound = (tax_remainder as u128 * compound_bps as u128 / 10_000u128) as u64;
    let reserve = tax_remainder - yield_b - compound;

    RefWaterfall {
        provider_fee: fee_due,
        dev_fee,
        lp_due,
        protocol_due,
        sovereign,
        yield_b,
        compound,
        reserve,
    }
}
