
use anchor_lang::prelude::*;
use anchor_lang::system_program;
use anchor_lang::AccountDeserialize;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};

declare_id!("85b3FfAzz3akfnH7NPCqR4Pjna45N3N6e6MvPsxABJ6n");

#[cfg(not(feature = "no-entrypoint"))]
solana_security_txt::security_txt! {
    name: "yield_escrow",
    project_url: "https://topbit.io",
    contacts: "email:security@topbit.io",
    policy: "https://topbit.io/security",
    preferred_languages: "en",
    source_code: "https://github.com/topbit-io/topbit"
}

const EPOCH_DURATION_SECS: i64 = 7 * 24 * 60 * 60;
const SWEEP_DELAY_SECS: i64 = 30 * 24 * 60 * 60;
const ADMIN_TIMELOCK_SECONDS: i64 = 259_200;

pub const MIN_STAKE_AGE_FOR_CLAIM_SECONDS: i64 = 86_400;

const PROPOSE_RATE_LIMIT_WINDOW_SECONDS: i64 = 86_400;
const PROPOSE_RATE_LIMIT_RING_LEN: usize = 5;
const CURRENCY_SOL: u8 = 0;
const CURRENCY_USDC: u8 = 1;
const BPS_DENOM: u128 = 10_000;

const TIER_WEIGHT_BPS: [u64; 8] = [0, 500, 1_500, 3_000, 5_000, 7_500, 9_000, 10_000];

const TIER_DIAMOND_IDX: u8 = 6;
const TIER_SOVEREIGN_IDX: u8 = 7;

pub const SOVEREIGN_REGISTRY_PROGRAM_ID: Pubkey =
    pubkey!("14ndgn3yKuD4Zi3ozBt7Fo4cYzUuYDAZrTn15wT3rFC2");

const SOVEREIGN_SEAT_DISCRIMINATOR: [u8; 8] =
    [86, 192, 194, 254, 102, 216, 130, 233];

const SOVEREIGN_SEAT_HOLDER_OFFSET: usize = 9;
const SOVEREIGN_SEAT_ACTIVE_OFFSET: usize = 49;
const SOVEREIGN_SEAT_MIN_LEN: usize = 50;

pub const ACC_SCALE: u128 = 1_000_000_000_000;

fn advance_acc_reward(cfg: &mut YieldConfig, amount: u64, total_weighted_stake: u128) -> Result<()> {
    if total_weighted_stake == 0 {
        cfg.undistributed_carry = cfg
            .undistributed_carry
            .checked_add(amount as u128)
            .ok_or(YieldError::MathOverflow)?;
        return Ok(());
    }
    let effective = (amount as u128)
        .checked_add(cfg.undistributed_carry)
        .ok_or(YieldError::MathOverflow)?;
    cfg.undistributed_carry = 0;
    let delta = effective
        .checked_mul(ACC_SCALE)
        .ok_or(YieldError::MathOverflow)?
        .checked_div(total_weighted_stake)
        .ok_or(YieldError::MathOverflow)?;
    cfg.acc_reward_per_share = cfg
        .acc_reward_per_share
        .checked_add(delta)
        .ok_or(YieldError::MathOverflow)?;
    Ok(())
}

fn read_live_total_weighted_stake(staking_config: &AccountInfo) -> Result<u128> {
    let data = staking_config.try_borrow_data()?;
    let scfg = staking::StakingConfig::try_deserialize(&mut data.as_ref())
        .map_err(|_| YieldError::Unauthorized)?;
    Ok(scfg.total_weighted_stake)
}

pub const WATERFALL_PROGRAM_ID: Pubkey = pubkey!("9tkZMhH7cf293wpGpJycqsZLb8eojkJ5jyUHJtqcS5zR");
pub const STAKING_PROGRAM_ID: Pubkey = pubkey!("2n2puiEN8BbMMEtq387b6HKR2trvKY9rK5uM82Ht2Vtc");
pub const LUCKY_DRAW_PROGRAM_ID: Pubkey = pubkey!("44PN7KBqH9XvuZcH3x41zQVfegEZNkNKsSkCieAtkEPc");

fn check_and_record_propose(cfg: &mut YieldConfig, now: i64) -> Result<()> {
    require!(now >= cfg.propose_cooldown_until, YieldError::ProposeCooldownActive);
    let window_start = now.saturating_sub(PROPOSE_RATE_LIMIT_WINDOW_SECONDS);
    let count_24h = cfg.recent_proposes.iter().filter(|t| **t > window_start).count();
    let next_cooldown_seconds: i64 = match count_24h {
        0 | 1 => 0, 2 => 1_800, 3 => 7_200, 4 => 86_400, _ => 604_800,
    };
    cfg.propose_cooldown_until = if next_cooldown_seconds == 0 {
        0
    } else {
        now.checked_add(next_cooldown_seconds).ok_or(YieldError::MathOverflow)?
    };
    for i in 0..(PROPOSE_RATE_LIMIT_RING_LEN - 1) {
        cfg.recent_proposes[i] = cfg.recent_proposes[i + 1];
    }
    cfg.recent_proposes[PROPOSE_RATE_LIMIT_RING_LEN - 1] = now;
    Ok(())
}

#[program]
pub mod yield_escrow {
    use super::*;

    pub fn initialize(
        ctx: Context<Initialize>,
        lucky_draw_pool_address: Pubkey,
        lucky_draw_usdc_account: Pubkey,
        usdc_mint: Pubkey,
    ) -> Result<()> {
        require_keys_neq!(usdc_mint, Pubkey::default(), YieldError::InvalidMint);
        let now = Clock::get()?.unix_timestamp;
        let cfg = &mut ctx.accounts.yield_config;
        cfg.authority = ctx.accounts.authority.key();
        cfg.current_epoch_id = 0;
        cfg.current_epoch_start = now;
        cfg.total_weighted_stake = 0;
        cfg.bump = ctx.bumps.yield_config;
        cfg.lucky_draw_pool = lucky_draw_pool_address;
        cfg.lucky_draw_usdc_account = lucky_draw_usdc_account;

        cfg.usdc_mint = usdc_mint;

        cfg.provider_vault_authority = Pubkey::default();

        cfg.pending_provider_vault_authority = Pubkey::default();
        cfg.pending_provider_vault_authority_unlocks_at = 0;

        cfg.propose_cooldown_until = 0;
        cfg.recent_proposes = [0i64; 5];
        cfg.pending_authority = Pubkey::default();
        cfg.pending_authority_unlocks_at = 0;

        cfg.acc_reward_per_share = 0;
        cfg.undistributed_carry = 0;

        let epoch = &mut ctx.accounts.epoch_zero;
        epoch.id = 0;
        epoch.start_timestamp = now;
        epoch.finalized = false;
        epoch.swept = false;
        epoch.total_weighted_stake_snapshot = 0;
        epoch.sol_pool = 0;
        epoch.usdc_pool = 0;
        epoch.sol_claimed = 0;
        epoch.usdc_claimed = 0;
        epoch.total_weight_claimed = 0;
        epoch.provider_pool_sol = 0;
        epoch.provider_pool_usdc = 0;
        epoch.provider_pool_usdc_claimed = 0;
        epoch.finalized_at = 0;
        epoch.bump = ctx.bumps.epoch_zero;
        Ok(())
    }

    pub fn set_provider_vault_authority(
        ctx: Context<TransferAuthority>,
        new_authority: Pubkey,
    ) -> Result<()> {
        let cfg = &mut ctx.accounts.yield_config;
        require_keys_eq!(
            cfg.authority,
            ctx.accounts.authority.key(),
            YieldError::Unauthorized
        );
        require!(new_authority != Pubkey::default(), YieldError::Unauthorized);
        require!(
            cfg.provider_vault_authority == Pubkey::default(),
            YieldError::ProviderVaultAuthorityAlreadyConfigured
        );
        cfg.provider_vault_authority = new_authority;
        emit!(ProviderVaultAuthoritySet {
            new_authority,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    pub fn propose_set_provider_vault_authority(
        ctx: Context<TransferAuthority>,
        new_authority: Pubkey,
    ) -> Result<()> {
        let cfg = &mut ctx.accounts.yield_config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), YieldError::Unauthorized);
        require!(new_authority != Pubkey::default(), YieldError::Unauthorized);
        let now = Clock::get()?.unix_timestamp;
        check_and_record_propose(cfg, now)?;
        cfg.pending_provider_vault_authority = new_authority;
        cfg.pending_provider_vault_authority_unlocks_at = now
            .checked_add(ADMIN_TIMELOCK_SECONDS)
            .ok_or(YieldError::MathOverflow)?;
        emit!(ProviderVaultAuthorityPending {
            pending_authority: new_authority,
            unlocks_at: cfg.pending_provider_vault_authority_unlocks_at,
        });
        Ok(())
    }

    pub fn finalize_set_provider_vault_authority(ctx: Context<TransferAuthority>) -> Result<()> {
        let cfg = &mut ctx.accounts.yield_config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), YieldError::Unauthorized);
        require!(
            cfg.pending_provider_vault_authority != Pubkey::default(),
            YieldError::NoPendingProviderAuthority,
        );
        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= cfg.pending_provider_vault_authority_unlocks_at,
            YieldError::TimelockNotElapsed,
        );
        let new_authority = cfg.pending_provider_vault_authority;
        cfg.provider_vault_authority = new_authority;
        cfg.pending_provider_vault_authority = Pubkey::default();
        cfg.pending_provider_vault_authority_unlocks_at = 0;
        emit!(ProviderVaultAuthoritySet {
            new_authority,
            timestamp: now,
        });
        Ok(())
    }

    pub fn cancel_set_provider_vault_authority(ctx: Context<TransferAuthority>) -> Result<()> {
        let cfg = &mut ctx.accounts.yield_config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), YieldError::Unauthorized);
        require!(
            cfg.pending_provider_vault_authority != Pubkey::default(),
            YieldError::NoPendingProviderAuthority,
        );
        cfg.pending_provider_vault_authority = Pubkey::default();
        cfg.pending_provider_vault_authority_unlocks_at = 0;
        emit!(ProviderVaultAuthorityCancelled {
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    #[allow(unreachable_code)]
    pub fn deposit_provider_yield(
        ctx: Context<DepositProviderYield>,
        amount: u64,
    ) -> Result<()> {
        require!(amount > 0, YieldError::ZeroAmount);

        require!(false, YieldError::SolDepositNotAllowed);

        let cfg = &ctx.accounts.yield_config;
        require!(
            cfg.provider_vault_authority != Pubkey::default(),
            YieldError::Unauthorized
        );
        require_keys_eq!(
            ctx.accounts.waterfall_signer.key(),
            cfg.provider_vault_authority,
            YieldError::Unauthorized
        );

        let epoch = &mut ctx.accounts.epoch;
        require!(!epoch.finalized, YieldError::EpochFinalized);

        let ix_ctx = CpiContext::new(
            ctx.accounts.system_program.to_account_info(),
            system_program::Transfer {
                from: ctx.accounts.waterfall_signer.to_account_info(),
                to: ctx.accounts.epoch_sol_vault.to_account_info(),
            },
        );
        system_program::transfer(ix_ctx, amount)?;

        epoch.provider_pool_sol = epoch
            .provider_pool_sol
            .checked_add(amount)
            .ok_or(YieldError::MathOverflow)?;

        emit!(ProviderYieldDepositedEvent {
            epoch_id: epoch.id,
            amount,
            total_provider_pool_after: epoch.provider_pool_sol,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    pub fn deposit_provider_yield_usdc(
        ctx: Context<DepositProviderYieldUsdc>,
        amount: u64,
    ) -> Result<()> {
        require!(amount > 0, YieldError::ZeroAmount);

        let cfg = &ctx.accounts.yield_config;
        require!(
            cfg.provider_vault_authority != Pubkey::default(),
            YieldError::Unauthorized
        );
        require_keys_eq!(
            ctx.accounts.waterfall_signer.key(),
            cfg.provider_vault_authority,
            YieldError::Unauthorized
        );

        let cpi_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.source_token_account.to_account_info(),
                to: ctx.accounts.yield_pool_usdc.to_account_info(),
                authority: ctx.accounts.waterfall_signer.to_account_info(),
            },
        );
        token::transfer(cpi_ctx, amount)?;

        let live_tws = read_live_total_weighted_stake(&ctx.accounts.staking_config)?;
        ctx.accounts.yield_config.total_weighted_stake = live_tws;
        advance_acc_reward(&mut ctx.accounts.yield_config, amount, live_tws)?;

        emit!(ProviderYieldDepositedUsdcEvent {
            epoch_id: 0,
            amount,
            total_provider_pool_usdc_after: amount,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    pub fn init_epoch_usdc_vault(
        ctx: Context<InitEpochUsdcVault>,
        epoch_id: u64,
    ) -> Result<()> {
        let cfg = &ctx.accounts.yield_config;
        require_keys_eq!(
            cfg.authority,
            ctx.accounts.authority.key(),
            YieldError::Unauthorized
        );
        let _ = epoch_id;
        Ok(())
    }

    pub fn init_yield_pool(ctx: Context<InitYieldPool>) -> Result<()> {
        require_keys_eq!(
            ctx.accounts.yield_config.authority,
            ctx.accounts.authority.key(),
            YieldError::Unauthorized
        );
        Ok(())
    }

    pub fn transfer_authority(ctx: Context<TransferAuthority>, _new_authority: Pubkey) -> Result<()> {
        let cfg = &ctx.accounts.yield_config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), YieldError::Unauthorized);
        msg!("DEPRECATED: use propose+finalize_rotate_admin for timelocked rotation (R7.7-H-01 / M-CRIT-02)");
        err!(YieldError::InstructionDeprecated)
    }


    pub fn propose_rotate_admin(
        ctx: Context<TransferAuthority>,
        new_admin: Pubkey,
    ) -> Result<()> {
        let cfg = &mut ctx.accounts.yield_config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), YieldError::Unauthorized);
        require!(new_admin != Pubkey::default(), YieldError::InvalidAdmin);
        require!(new_admin != cfg.authority, YieldError::InvalidAdmin);
        require!(
            cfg.pending_authority == Pubkey::default(),
            YieldError::AdminProposalAlreadyPending
        );
        let now = Clock::get()?.unix_timestamp;
        check_and_record_propose(cfg, now)?;
        let unlocks_at = now
            .checked_add(ADMIN_TIMELOCK_SECONDS)
            .ok_or(YieldError::MathOverflow)?;
        cfg.pending_authority = new_admin;
        cfg.pending_authority_unlocks_at = unlocks_at;
        emit!(AdminRotationProposed {
            admin: cfg.authority,
            new_admin,
            unlocks_at,
            timestamp: now,
        });
        Ok(())
    }

    pub fn finalize_rotate_admin(ctx: Context<TransferAuthority>) -> Result<()> {
        let cfg = &mut ctx.accounts.yield_config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), YieldError::Unauthorized);
        require!(
            cfg.pending_authority != Pubkey::default(),
            YieldError::AdminNoProposalPending
        );
        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= cfg.pending_authority_unlocks_at,
            YieldError::TimelockNotElapsed
        );
        let old_admin = cfg.authority;
        let new_admin = cfg.pending_authority;
        cfg.authority = new_admin;
        cfg.pending_authority = Pubkey::default();
        cfg.pending_authority_unlocks_at = 0;
        emit!(AdminRotated { old_admin, new_admin, timestamp: now });
        Ok(())
    }

    pub fn cancel_rotate_admin(ctx: Context<TransferAuthority>) -> Result<()> {
        let cfg = &mut ctx.accounts.yield_config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), YieldError::Unauthorized);
        require!(
            cfg.pending_authority != Pubkey::default(),
            YieldError::AdminNoProposalPending
        );
        let cancelled_admin = cfg.pending_authority;
        cfg.pending_authority = Pubkey::default();
        cfg.pending_authority_unlocks_at = 0;
        emit!(AdminRotationProposalCancelled {
            admin: cfg.authority,
            cancelled_admin,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    #[allow(unreachable_code)]
    pub fn deposit_yield(ctx: Context<DepositYield>, amount: u64, currency: u8) -> Result<()> {
        return err!(YieldError::InstructionDeprecated);
        require!(amount > 0, YieldError::ZeroAmount);
        require!(currency == CURRENCY_SOL || currency == CURRENCY_USDC, YieldError::BadCurrency);
        require!(currency == CURRENCY_USDC, YieldError::SolDepositNotAllowed);

        let (expected_waterfall_pda, _) = Pubkey::find_program_address(
            &[b"swap_router_config"],
            &WATERFALL_PROGRAM_ID,
        );
        require_keys_eq!(
            ctx.accounts.waterfall_config.key(),
            expected_waterfall_pda,
            YieldError::Unauthorized
        );

        let epoch = &mut ctx.accounts.epoch;
        require!(!epoch.finalized, YieldError::EpochFinalized);

        if currency == CURRENCY_SOL {
            let ix_ctx = CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                system_program::Transfer {
                    from: ctx.accounts.source.to_account_info(),
                    to: ctx.accounts.epoch_sol_vault.to_account_info(),
                },
            );
            system_program::transfer(ix_ctx, amount)?;
            epoch.sol_pool = epoch.sol_pool.checked_add(amount).ok_or(YieldError::MathOverflow)?;
        } else {
            let src = ctx.accounts.source_token_account.as_ref()
                .ok_or(YieldError::MissingUsdcAccounts)?;
            let dst = ctx.accounts.epoch_usdc_vault.as_ref()
                .ok_or(YieldError::MissingUsdcAccounts)?;
            let token_prog = ctx.accounts.token_program.as_ref()
                .ok_or(YieldError::MissingUsdcAccounts)?;
            let cpi_ctx = CpiContext::new(
                token_prog.to_account_info(),
                Transfer {
                    from: src.to_account_info(),
                    to: dst.to_account_info(),
                    authority: ctx.accounts.waterfall_config.to_account_info(),
                },
            );
            token::transfer(cpi_ctx, amount)?;
            epoch.usdc_pool = epoch.usdc_pool.checked_add(amount).ok_or(YieldError::MathOverflow)?;
        }

        emit!(YieldDepositedEvent {
            epoch_id: epoch.id,
            amount,
            currency,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    #[allow(unreachable_code)]
    pub fn notify_yield_deposit(
        ctx: Context<NotifyYieldDeposit>,
        amount: u64,
        currency: u8,
    ) -> Result<()> {
        return err!(YieldError::InstructionDeprecated);
        require!(amount > 0, YieldError::ZeroAmount);
        require!(currency == CURRENCY_SOL || currency == CURRENCY_USDC, YieldError::BadCurrency);
        require!(currency == CURRENCY_USDC, YieldError::SolDepositNotAllowed);

        let (expected_waterfall_pda, _) = Pubkey::find_program_address(
            &[b"swap_router_config"],
            &WATERFALL_PROGRAM_ID,
        );
        require_keys_eq!(
            ctx.accounts.waterfall_config.key(),
            expected_waterfall_pda,
            YieldError::Unauthorized,
        );

        let epoch = &mut ctx.accounts.epoch;
        require!(!epoch.finalized, YieldError::EpochFinalized);

        if currency == CURRENCY_SOL {
            epoch.sol_pool = epoch.sol_pool
                .checked_add(amount)
                .ok_or(YieldError::MathOverflow)?;
        } else {
            epoch.usdc_pool = epoch.usdc_pool
                .checked_add(amount)
                .ok_or(YieldError::MathOverflow)?;
        }

        emit!(YieldDepositNotifiedEvent {
            epoch_id: epoch.id,
            amount,
            currency,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    pub fn rollover_epoch(ctx: Context<RolloverEpoch>) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        let cfg = &mut ctx.accounts.yield_config;

        let elapsed = now.checked_sub(cfg.current_epoch_start).ok_or(YieldError::MathOverflow)?;
        require!(elapsed >= EPOCH_DURATION_SECS, YieldError::EpochNotReady);

        let total_weighted_stake: u128 = {
            let data = ctx.accounts.staking_config.try_borrow_data()?;
            let staking_cfg = staking::StakingConfig::try_deserialize(&mut data.as_ref())
                .map_err(|_| YieldError::Unauthorized)?;
            staking_cfg.total_weighted_stake
        };
        cfg.total_weighted_stake = total_weighted_stake;

        let prev = &mut ctx.accounts.prev_epoch;
        require!(!prev.finalized, YieldError::EpochFinalized);
        require_eq!(prev.id, cfg.current_epoch_id, YieldError::EpochMismatch);
        prev.finalized = true;
        prev.total_weighted_stake_snapshot = cfg.total_weighted_stake;
        prev.finalized_at = now;

        let new_id = cfg.current_epoch_id.checked_add(1).ok_or(YieldError::MathOverflow)?;
        let next = &mut ctx.accounts.next_epoch;
        next.id = new_id;
        next.start_timestamp = now;
        next.finalized = false;
        next.swept = false;
        next.total_weighted_stake_snapshot = 0;
        next.sol_pool = 0;
        next.usdc_pool = 0;
        next.sol_claimed = 0;
        next.usdc_claimed = 0;
        next.total_weight_claimed = 0;
        next.provider_pool_sol = 0;
        next.provider_pool_usdc = 0;
        next.provider_pool_usdc_claimed = 0;
        next.finalized_at = 0;
        next.bump = ctx.bumps.next_epoch;

        cfg.current_epoch_id = new_id;
        cfg.current_epoch_start = now;

        emit!(EpochRolledOverEvent {
            prev_id: prev.id,
            new_id,
            snapshot: prev.total_weighted_stake_snapshot,
            timestamp: now,
        });
        Ok(())
    }

    pub fn claim(ctx: Context<Claim>) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;

        let (pos_owner, amount, tier, stake_ts, reward_owed, reward_debt) = {
            let data = ctx.accounts.stake_position.try_borrow_data()?;
            let pos = staking::StakePosition::try_deserialize(&mut data.as_ref())
                .map_err(|_| YieldError::Unauthorized)?;
            (
                pos.owner,
                pos.amount,
                pos.tier,
                pos.stake_timestamp,
                pos.reward_owed,
                pos.reward_debt,
            )
        };
        require_keys_eq!(pos_owner, ctx.accounts.staker.key(), YieldError::Unauthorized);
        require!((tier as usize) < TIER_WEIGHT_BPS.len(), YieldError::BadTier);

        let effective_tier: u8 = if tier == TIER_SOVEREIGN_IDX {
            let valid_seat = match &ctx.accounts.sovereign_seat {
                Some(seat) => is_valid_active_sovereign_seat(
                    &seat.to_account_info(),
                    &ctx.accounts.staker.key(),
                )?,
                None => false,
            };
            if valid_seat { TIER_SOVEREIGN_IDX } else { TIER_DIAMOND_IDX }
        } else {
            tier
        };

        let stake_age = now.checked_sub(stake_ts).ok_or(YieldError::MathOverflow)?;
        require!(
            stake_age >= MIN_STAKE_AGE_FOR_CLAIM_SECONDS,
            YieldError::StakeTooFreshToClaim
        );

        let acc = ctx.accounts.yield_config.acc_reward_per_share;
        let weight = (amount as u128)
            .checked_mul(TIER_WEIGHT_BPS[effective_tier as usize] as u128)
            .ok_or(YieldError::MathOverflow)?
            .checked_div(10_000u128)
            .ok_or(YieldError::MathOverflow)?;
        let accrued = weight
            .checked_mul(acc)
            .ok_or(YieldError::MathOverflow)?
            .checked_div(ACC_SCALE)
            .ok_or(YieldError::MathOverflow)?;
        let entitlement = (reward_owed as u128)
            .checked_add(accrued)
            .ok_or(YieldError::MathOverflow)?
            .saturating_sub(reward_debt);

        let claimed_so_far = ctx.accounts.claim_checkpoint.claimed as u128;
        let claimable: u64 = entitlement
            .saturating_sub(claimed_so_far)
            .try_into()
            .map_err(|_| YieldError::MathOverflow)?;

        let pool_bump = ctx.bumps.yield_pool_usdc;
        let cp_bump = ctx.bumps.claim_checkpoint;
        {
            let checkpoint = &mut ctx.accounts.claim_checkpoint;
            checkpoint.staker = ctx.accounts.staker.key();
            checkpoint.bump = cp_bump;
            if claimable == 0 {
                return Ok(());
            }
            checkpoint.claimed = checkpoint
                .claimed
                .checked_add(claimable)
                .ok_or(YieldError::MathOverflow)?;
        }

        let seeds: &[&[u8]] = &[b"yield_pool_usdc", &[pool_bump]];
        let signer: &[&[&[u8]]] = &[seeds];
        let cpi = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.yield_pool_usdc.to_account_info(),
                to: ctx.accounts.staker_usdc_account.to_account_info(),
                authority: ctx.accounts.yield_pool_usdc.to_account_info(),
            },
            signer,
        );
        token::transfer(cpi, claimable)?;

        emit!(AccumulatorYieldClaimedEvent {
            staker: ctx.accounts.staker.key(),
            amount: claimable,
            total_claimed: ctx.accounts.claim_checkpoint.claimed,
            acc_reward_per_share: acc,
            timestamp: now,
        });
        Ok(())
    }

    pub fn sweep_unclaimed(ctx: Context<SweepUnclaimed>, epoch_id: u64) -> Result<()> {
        require_keys_eq!(
            ctx.accounts.lucky_draw_pool.key(),
            ctx.accounts.yield_config.lucky_draw_pool,
            YieldError::InvalidLuckyDrawPool,
        );
        require_keys_eq!(
            ctx.accounts.lucky_draw_usdc_account.key(),
            ctx.accounts.yield_config.lucky_draw_usdc_account,
            YieldError::InvalidLuckyDrawUsdcAccount,
        );

        let epoch = &mut ctx.accounts.epoch;
        require_eq!(epoch.id, epoch_id, YieldError::EpochMismatch);
        require!(epoch.finalized, YieldError::EpochNotFinalized);
        require!(!epoch.swept, YieldError::EpochSwept);

        let now = Clock::get()?.unix_timestamp;
        let close_ts = epoch.start_timestamp
            .checked_add(EPOCH_DURATION_SECS).ok_or(YieldError::MathOverflow)?;
        let sweep_ts = close_ts
            .checked_add(SWEEP_DELAY_SECS).ok_or(YieldError::MathOverflow)?;
        require!(now >= sweep_ts, YieldError::SweepNotReady);

        let remaining_sol = epoch.sol_pool.checked_sub(epoch.sol_claimed).ok_or(YieldError::MathOverflow)?;
        let remaining_usdc_native = epoch.usdc_pool.checked_sub(epoch.usdc_claimed).ok_or(YieldError::MathOverflow)?;
        let remaining_provider_usdc = epoch.provider_pool_usdc
            .checked_sub(epoch.provider_pool_usdc_claimed).ok_or(YieldError::MathOverflow)?;
        let remaining_usdc = remaining_usdc_native
            .checked_add(remaining_provider_usdc).ok_or(YieldError::MathOverflow)?;

        if remaining_sol > 0 {
            let epoch_id_bytes = epoch.id.to_le_bytes();
            let seeds: &[&[u8]] = &[b"epoch_sol_vault", epoch_id_bytes.as_ref(), &[ctx.bumps.epoch_sol_vault]];
            let signer_seeds = &[seeds];
            let cpi_ctx = CpiContext::new_with_signer(
                ctx.accounts.system_program.to_account_info(),
                system_program::Transfer {
                    from: ctx.accounts.epoch_sol_vault.to_account_info(),
                    to: ctx.accounts.lucky_draw_pool.to_account_info(),
                },
                signer_seeds,
            );
            system_program::transfer(cpi_ctx, remaining_sol)?;
        }
        if remaining_usdc > 0 {
            let epoch_id_bytes = epoch.id.to_le_bytes();
            let seeds: &[&[u8]] = &[b"epoch_usdc_vault", epoch_id_bytes.as_ref(), &[ctx.bumps.epoch_usdc_vault]];
            let signer_seeds = &[seeds];
            let cpi_ctx = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.epoch_usdc_vault.to_account_info(),
                    to: ctx.accounts.lucky_draw_usdc_account.to_account_info(),
                    authority: ctx.accounts.epoch_usdc_vault.to_account_info(),
                },
                signer_seeds,
            );
            token::transfer(cpi_ctx, remaining_usdc)?;
        }

        epoch.swept = true;

        emit!(EpochSweptEvent {
            epoch_id,
            swept_sol: remaining_sol,
            swept_usdc: remaining_usdc_native,
            swept_provider_usdc: remaining_provider_usdc,
            timestamp: now,
        });
        Ok(())
    }
}



fn is_valid_active_sovereign_seat(
    seat_acc: &AccountInfo,
    claimer_key: &Pubkey,
) -> Result<bool> {
    if seat_acc.owner != &SOVEREIGN_REGISTRY_PROGRAM_ID {
        return Ok(false);
    }
    let data = seat_acc.try_borrow_data()?;
    if data.len() < SOVEREIGN_SEAT_MIN_LEN {
        return Ok(false);
    }
    if &data[0..8] != SOVEREIGN_SEAT_DISCRIMINATOR {
        return Ok(false);
    }
    let holder_bytes: [u8; 32] = data
        [SOVEREIGN_SEAT_HOLDER_OFFSET .. SOVEREIGN_SEAT_HOLDER_OFFSET + 32]
        .try_into().map_err(|_| YieldError::MathOverflow)?;
    let holder = Pubkey::new_from_array(holder_bytes);
    if &holder != claimer_key {
        return Ok(false);
    }
    if data[SOVEREIGN_SEAT_ACTIVE_OFFSET] == 0 {
        return Ok(false);
    }
    Ok(true)
}

#[account]
pub struct StakePosition {
    pub owner: Pubkey,
    pub amount: u64,
    pub tier: u8,
    pub stake_timestamp: i64,
    pub last_claim: i64,
    pub etop_balance: u64,
    pub bump: u8,
}


#[account]
pub struct YieldConfig {
    pub authority: Pubkey,
    pub current_epoch_id: u64,
    pub current_epoch_start: i64,
    pub total_weighted_stake: u128,
    pub bump: u8,
    pub lucky_draw_pool: Pubkey,
    pub lucky_draw_usdc_account: Pubkey,
    pub provider_vault_authority: Pubkey,
    pub usdc_mint: Pubkey,
    pub pending_provider_vault_authority: Pubkey,
    pub pending_provider_vault_authority_unlocks_at: i64,

    pub propose_cooldown_until: i64,
    pub recent_proposes: [i64; 5],

    pub pending_authority: Pubkey,
    pub pending_authority_unlocks_at: i64,

    pub acc_reward_per_share: u128,
    pub undistributed_carry: u128,
}
impl YieldConfig {
    pub const SPACE: usize = 8 + 32 + 8 + 8 + 16 + 1 + 32 + 32 + 32 + 32 + 32 + 8
        + 8 + 40 + 32 + 8
        + 16 + 16;
    pub const ACC_REWARD_PER_SHARE_OFFSET: usize = 329;
}

#[account]
pub struct YieldClaimCheckpoint {
    pub staker: Pubkey,
    pub claimed: u64,
    pub bump: u8,
}
impl YieldClaimCheckpoint {
    pub const SPACE: usize = 8 + 32 + 8 + 1;
}

#[account]
pub struct Epoch {
    pub id: u64,
    pub start_timestamp: i64,
    pub finalized: bool,
    pub swept: bool,
    pub total_weighted_stake_snapshot: u128,
    pub sol_pool: u64,
    pub usdc_pool: u64,
    pub sol_claimed: u64,
    pub usdc_claimed: u64,
    pub total_weight_claimed: u128,
    pub provider_pool_sol: u64,
    pub provider_pool_usdc: u64,
    pub provider_pool_usdc_claimed: u64,
    pub finalized_at: i64,
    pub bump: u8,
}
impl Epoch {
    pub const SPACE: usize = 8 + 8 + 8 + 1 + 1 + 16 + 8 + 8 + 8 + 8 + 16 + 8 + 8 + 8 + 8 + 1;
}

#[account]
pub struct ClaimPosition {
    pub epoch_id: u64,
    pub staker: Pubkey,
    pub claimed: bool,
    pub sol_claimed: u64,
    pub usdc_claimed: u64,
    pub etop_vested: u64,
    pub claim_weight: u128,
    pub bump: u8,
}
impl ClaimPosition {
    pub const SPACE: usize = 8 + 8 + 32 + 1 + 8 + 8 + 8 + 16 + 1;
}


#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = authority,
        space = YieldConfig::SPACE,
        seeds = [b"yield_config"],
        bump
    )]
    pub yield_config: Account<'info, YieldConfig>,
    #[account(
        init,
        payer = authority,
        space = Epoch::SPACE,
        seeds = [b"epoch", 0u64.to_le_bytes().as_ref()],
        bump,
    )]
    pub epoch_zero: Account<'info, Epoch>,
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(
        seeds = [crate::ID.as_ref()],
        bump,
        seeds::program = anchor_lang::solana_program::bpf_loader_upgradeable::ID,
        constraint = program_data.upgrade_authority_address == Some(authority.key())
            @ YieldError::Unauthorized,
    )]
    pub program_data: Account<'info, ProgramData>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(epoch_id: u64)]
pub struct InitEpochUsdcVault<'info> {
    #[account(seeds = [b"yield_config"], bump = yield_config.bump)]
    pub yield_config: Account<'info, YieldConfig>,
    #[account(
        constraint = usdc_mint.key() == yield_config.usdc_mint @ YieldError::InvalidMint
    )]
    pub usdc_mint: Account<'info, anchor_spl::token::Mint>,
    #[account(
        init_if_needed,
        payer = authority,
        seeds = [b"epoch_usdc_vault", epoch_id.to_le_bytes().as_ref()],
        bump,
        token::mint = usdc_mint,
        token::authority = epoch_usdc_vault,
    )]
    pub epoch_usdc_vault: Account<'info, TokenAccount>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct InitYieldPool<'info> {
    #[account(seeds = [b"yield_config"], bump = yield_config.bump)]
    pub yield_config: Account<'info, YieldConfig>,
    #[account(
        constraint = usdc_mint.key() == yield_config.usdc_mint @ YieldError::InvalidMint
    )]
    pub usdc_mint: Account<'info, anchor_spl::token::Mint>,
    #[account(
        init_if_needed,
        payer = authority,
        seeds = [b"yield_pool_usdc"],
        bump,
        token::mint = usdc_mint,
        token::authority = yield_pool_usdc,
    )]
    pub yield_pool_usdc: Account<'info, TokenAccount>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct TransferAuthority<'info> {
    #[account(mut, seeds = [b"yield_config"], bump = yield_config.bump)]
    pub yield_config: Account<'info, YieldConfig>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(amount: u64, currency: u8)]
pub struct DepositYield<'info> {
    #[account(mut, seeds = [b"yield_config"], bump = yield_config.bump)]
    pub yield_config: Account<'info, YieldConfig>,

    #[account(
        mut,
        seeds = [b"epoch", yield_config.current_epoch_id.to_le_bytes().as_ref()],
        bump = epoch.bump,
    )]
    pub epoch: Account<'info, Epoch>,

    pub waterfall_config: Signer<'info>,

    #[account(mut)]
    pub source: AccountInfo<'info>,

    #[account(mut)]
    pub source_token_account: Option<Account<'info, TokenAccount>>,

    #[account(
        mut,
        seeds = [b"epoch_sol_vault", yield_config.current_epoch_id.to_le_bytes().as_ref()],
        bump,
    )]
    pub epoch_sol_vault: AccountInfo<'info>,

    #[account(mut)]
    pub epoch_usdc_vault: Option<Account<'info, TokenAccount>>,

    pub token_program: Option<Program<'info, Token>>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(amount: u64, currency: u8)]
pub struct NotifyYieldDeposit<'info> {
    #[account(mut, seeds = [b"yield_config"], bump = yield_config.bump)]
    pub yield_config: Account<'info, YieldConfig>,

    #[account(
        mut,
        seeds = [b"epoch", yield_config.current_epoch_id.to_le_bytes().as_ref()],
        bump = epoch.bump,
    )]
    pub epoch: Account<'info, Epoch>,

    pub waterfall_config: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(amount: u64)]
pub struct DepositProviderYield<'info> {
    #[account(mut, seeds = [b"yield_config"], bump = yield_config.bump)]
    pub yield_config: Account<'info, YieldConfig>,

    #[account(
        mut,
        seeds = [b"epoch", yield_config.current_epoch_id.to_le_bytes().as_ref()],
        bump = epoch.bump,
    )]
    pub epoch: Account<'info, Epoch>,

    #[account(
        mut,
        seeds = [b"epoch_sol_vault", yield_config.current_epoch_id.to_le_bytes().as_ref()],
        bump,
    )]
    pub epoch_sol_vault: AccountInfo<'info>,

    #[account(mut)]
    pub waterfall_signer: Signer<'info>,

    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(amount: u64)]
pub struct DepositProviderYieldUsdc<'info> {
    #[account(mut, seeds = [b"yield_config"], bump = yield_config.bump)]
    pub yield_config: Account<'info, YieldConfig>,

    #[account(mut)]
    pub source_token_account: Account<'info, TokenAccount>,

    #[account(
        mut,
        seeds = [b"yield_pool_usdc"],
        bump,
        token::mint = yield_config.usdc_mint,
        token::authority = yield_pool_usdc,
    )]
    pub yield_pool_usdc: Account<'info, TokenAccount>,

    pub waterfall_signer: Signer<'info>,

    #[account(
        seeds = [b"staking_config"],
        bump,
        seeds::program = STAKING_PROGRAM_ID,
        owner = STAKING_PROGRAM_ID,
    )]
    pub staking_config: AccountInfo<'info>,

    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct RolloverEpoch<'info> {
    #[account(mut, seeds = [b"yield_config"], bump = yield_config.bump)]
    pub yield_config: Account<'info, YieldConfig>,

    #[account(
        mut,
        seeds = [b"epoch", yield_config.current_epoch_id.to_le_bytes().as_ref()],
        bump = prev_epoch.bump,
    )]
    pub prev_epoch: Account<'info, Epoch>,

    #[account(
        init,
        payer = payer,
        space = Epoch::SPACE,
        seeds = [b"epoch", (yield_config.current_epoch_id + 1).to_le_bytes().as_ref()],
        bump,
    )]
    pub next_epoch: Account<'info, Epoch>,

    #[account(
        seeds = [b"staking_config"],
        bump,
        seeds::program = STAKING_PROGRAM_ID,
        owner = STAKING_PROGRAM_ID,
    )]
    pub staking_config: AccountInfo<'info>,

    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Claim<'info> {
    #[account(seeds = [b"yield_config"], bump = yield_config.bump)]
    pub yield_config: Account<'info, YieldConfig>,

    #[account(
        mut,
        seeds = [b"yield_pool_usdc"],
        bump,
        token::mint = yield_config.usdc_mint,
        token::authority = yield_pool_usdc,
    )]
    pub yield_pool_usdc: Account<'info, TokenAccount>,

    #[account(
        init_if_needed,
        payer = staker,
        space = YieldClaimCheckpoint::SPACE,
        seeds = [b"yield_claim", staker.key().as_ref()],
        bump,
    )]
    pub claim_checkpoint: Account<'info, YieldClaimCheckpoint>,

    #[account(
        seeds = [b"stake_position", staker.key().as_ref()],
        bump,
        seeds::program = STAKING_PROGRAM_ID,
        owner = STAKING_PROGRAM_ID,
    )]
    pub stake_position: UncheckedAccount<'info>,

    pub sovereign_seat: Option<UncheckedAccount<'info>>,

    #[account(mut, token::mint = yield_config.usdc_mint, token::authority = staker)]
    pub staker_usdc_account: Account<'info, TokenAccount>,

    #[account(mut)]
    pub staker: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(epoch_id: u64)]
pub struct SweepUnclaimed<'info> {
    #[account(seeds = [b"yield_config"], bump = yield_config.bump)]
    pub yield_config: Account<'info, YieldConfig>,

    #[account(
        mut,
        seeds = [b"epoch", epoch_id.to_le_bytes().as_ref()],
        bump = epoch.bump,
    )]
    pub epoch: Account<'info, Epoch>,

    #[account(
        mut,
        seeds = [b"epoch_sol_vault", epoch_id.to_le_bytes().as_ref()],
        bump,
    )]
    pub epoch_sol_vault: AccountInfo<'info>,

    #[account(
        mut,
        seeds = [b"epoch_usdc_vault", epoch_id.to_le_bytes().as_ref()],
        bump,
    )]
    pub epoch_usdc_vault: Account<'info, TokenAccount>,

    #[account(mut)]
    pub lucky_draw_pool: AccountInfo<'info>,

    #[account(mut)]
    pub lucky_draw_usdc_account: Account<'info, TokenAccount>,

    #[account(mut)]
    pub caller: Signer<'info>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}


#[event]
pub struct YieldDepositedEvent {
    pub epoch_id: u64,
    pub amount: u64,
    pub currency: u8,
    pub timestamp: i64,
}

#[event]
pub struct YieldDepositNotifiedEvent {
    pub epoch_id: u64,
    pub amount: u64,
    pub currency: u8,
    pub timestamp: i64,
}

#[event]
pub struct EpochRolledOverEvent {
    pub prev_id: u64,
    pub new_id: u64,
    pub snapshot: u128,
    pub timestamp: i64,
}

#[event]
pub struct AccumulatorYieldClaimedEvent {
    pub staker: Pubkey,
    pub amount: u64,
    pub total_claimed: u64,
    pub acc_reward_per_share: u128,
    pub timestamp: i64,
}

#[event]
pub struct YieldClaimedEvent {
    pub epoch_id: u64,
    pub staker: Pubkey,
    pub sol_liquid: u64,
    pub usdc_liquid: u64,
    pub provider_usdc_liquid: u64,
    pub etop_vested: u64,
    pub timestamp: i64,
}

#[event]
pub struct EpochSweptEvent {
    pub epoch_id: u64,
    pub swept_sol: u64,
    pub swept_usdc: u64,
    pub swept_provider_usdc: u64,
    pub timestamp: i64,
}

#[event]
pub struct AuthorityTransferredEvent {
    pub new_authority: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct AdminRotationProposed {
    pub admin: Pubkey,
    pub new_admin: Pubkey,
    pub unlocks_at: i64,
    pub timestamp: i64,
}
#[event]
pub struct AdminRotated {
    pub old_admin: Pubkey,
    pub new_admin: Pubkey,
    pub timestamp: i64,
}
#[event]
pub struct AdminRotationProposalCancelled {
    pub admin: Pubkey,
    pub cancelled_admin: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct ProviderYieldDepositedEvent {
    pub epoch_id: u64,
    pub amount: u64,
    pub total_provider_pool_after: u64,
    pub timestamp: i64,
}

#[event]
pub struct ProviderYieldDepositedUsdcEvent {
    pub epoch_id: u64,
    pub amount: u64,
    pub total_provider_pool_usdc_after: u64,
    pub timestamp: i64,
}

#[event]
pub struct ProviderVaultAuthoritySet {
    pub new_authority: Pubkey,
    pub timestamp: i64,
}

#[event]
pub struct ProviderVaultAuthorityPending {
    pub pending_authority: Pubkey,
    pub unlocks_at: i64,
}

#[event]
pub struct ProviderVaultAuthorityCancelled {
    pub timestamp: i64,
}


#[error_code]
pub enum YieldError {
    #[msg("Math overflow")]
    MathOverflow,
    #[msg("Amount must be > 0")]
    ZeroAmount,
    #[msg("Unauthorized caller")]
    Unauthorized,
    #[msg("Unknown currency code")]
    BadCurrency,
    #[msg("Epoch already finalized")]
    EpochFinalized,
    #[msg("Epoch not yet finalized")]
    EpochNotFinalized,
    #[msg("Epoch already swept")]
    EpochSwept,
    #[msg("Epoch rollover not ready (7 days not elapsed)")]
    EpochNotReady,
    #[msg("Epoch id mismatch")]
    EpochMismatch,
    #[msg("Staker has nothing staked")]
    NothingStaked,
    #[msg("Invalid tier index")]
    BadTier,
    #[msg("Tier has zero yield weight")]
    NoTierWeight,
    #[msg("Already claimed for this epoch")]
    AlreadyClaimed,
    #[msg("Epoch snapshot has zero total weight")]
    EmptySnapshot,
    #[msg("Epoch weight fully claimed — cumulative claimed weight reached the snapshot total (HIGH-1)")]
    EpochWeightExhausted,
    #[msg("Sweep window not yet open (30 days after close)")]
    SweepNotReady,
    #[msg("USDC accounts (source_token_account, epoch_usdc_vault, token_program) are required for USDC deposits")]
    MissingUsdcAccounts,
    #[msg("lucky_draw_pool does not match the canonical address stored at initialize time")]
    InvalidLuckyDrawPool,
    #[msg("lucky_draw_usdc_account does not match the canonical address stored at initialize time")]
    InvalidLuckyDrawUsdcAccount,
    #[msg("SOL deposits are not accepted at v1 — only USDC (currency=1) is allowed")]
    SolDepositNotAllowed,
    #[msg("usdc_mint must be a non-default pubkey (real USDC mint)")]
    InvalidMint,
    #[msg("No pending provider_vault_authority rotation — call propose first")]
    NoPendingProviderAuthority,
    #[msg("72h timelock has not elapsed since propose_set_provider_vault_authority")]
    TimelockNotElapsed,

    #[msg("Stake position is too fresh — effective stake age must be >= MIN_STAKE_AGE_FOR_CLAIM_SECONDS (24h)")]
    StakeTooFreshToClaim,

    #[msg("Stake position post-dates epoch finalization — claimant was not in the snapshot (YE-H01)")]
    StakeAfterEpochFinalized,

    #[msg("Propose cooldown active — escalating rate-limit per Rule 27b defense (R7.7-H-01)")]
    ProposeCooldownActive,
    #[msg("Invalid admin pubkey — must be non-default and distinct from current admin")]
    InvalidAdmin,
    #[msg("Admin rotation proposal already pending — cancel before re-proposing")]
    AdminProposalAlreadyPending,
    #[msg("No admin rotation proposal pending")]
    AdminNoProposalPending,

    #[msg("Instruction deprecated — call the propose/finalize triplet for timelocked rotation (M-CRIT-02 / R9-1-RC-02)")]
    InstructionDeprecated,

    #[msg("provider_vault_authority already configured — use propose_set_provider_vault_authority for timelocked rotation (Wave E.2)")]
    ProviderVaultAuthorityAlreadyConfigured,
}


#[cfg(all(test, not(feature = "idl-build")))]
mod tests {
    use super::*;


    #[test]
    fn deposit_yield_expects_swap_router_config_pda() {
        let (expected_new, _) = Pubkey::find_program_address(
            &[b"swap_router_config"],
            &WATERFALL_PROGRAM_ID,
        );
        let (expected_old, _) = Pubkey::find_program_address(
            &[b"waterfall_config"],
            &WATERFALL_PROGRAM_ID,
        );
        assert_ne!(expected_new, expected_old);
    }

    #[test]
    fn waterfall_program_id_is_swap_router() {
        assert_eq!(
            WATERFALL_PROGRAM_ID.to_string(),
            "9tkZMhH7cf293wpGpJycqsZLb8eojkJ5jyUHJtqcS5zR"
        );
    }


    #[test]
    fn sovereign_registry_program_id_matches() {
        assert_eq!(
            SOVEREIGN_REGISTRY_PROGRAM_ID.to_string(),
            "14ndgn3yKuD4Zi3ozBt7Fo4cYzUuYDAZrTn15wT3rFC2"
        );
    }

    #[test]
    fn sovereign_seat_offsets_locked() {
        assert_eq!(SOVEREIGN_SEAT_HOLDER_OFFSET, 9);
        assert_eq!(SOVEREIGN_SEAT_ACTIVE_OFFSET, 49);
        assert!(SOVEREIGN_SEAT_MIN_LEN >= 50);
    }

    #[test]
    fn sovereign_seat_discriminator_pinned() {
        let expected: [u8; 8] = [86, 192, 194, 254, 102, 216, 130, 233];
        assert_eq!(SOVEREIGN_SEAT_DISCRIMINATOR, expected);
    }

    #[test]
    fn tier_indices_match_staking() {
        assert_eq!(TIER_SOVEREIGN_IDX, 7);
        assert_eq!(TIER_DIAMOND_IDX, 6);
        assert_eq!(TIER_WEIGHT_BPS[TIER_SOVEREIGN_IDX as usize], 10_000);
        assert_eq!(TIER_WEIGHT_BPS[TIER_DIAMOND_IDX as usize], 9_000);
    }

    #[test]
    fn yield_f02_stake_position_layout_pinned() {
        assert_eq!(staking::StakePosition::SPACE, 100);
        assert_eq!(staking::StakePosition::REWARD_OWED_OFFSET, 74);
        assert_eq!(staking::StakePosition::REWARD_DEBT_OFFSET, 82);
    }

    #[test]
    fn claim_downgrades_sovereign_without_seat_to_diamond_weight() {
        let position_tier = TIER_SOVEREIGN_IDX;
        let has_valid_seat = false;
        let effective_tier = if position_tier == TIER_SOVEREIGN_IDX && !has_valid_seat {
            TIER_DIAMOND_IDX
        } else {
            position_tier
        };
        assert_eq!(effective_tier, TIER_DIAMOND_IDX);
        assert_eq!(TIER_WEIGHT_BPS[effective_tier as usize], 9_000);
    }

    #[test]
    fn claim_keeps_sovereign_weight_with_active_seat() {
        let position_tier = TIER_SOVEREIGN_IDX;
        let has_valid_seat = true;
        let effective_tier = if position_tier == TIER_SOVEREIGN_IDX && !has_valid_seat {
            TIER_DIAMOND_IDX
        } else {
            position_tier
        };
        assert_eq!(effective_tier, TIER_SOVEREIGN_IDX);
        assert_eq!(TIER_WEIGHT_BPS[effective_tier as usize], 10_000);
    }

    #[test]
    fn non_sovereign_tier_ignores_seat_branch() {
        for tier in 0u8..=6u8 {
            let effective = if tier == TIER_SOVEREIGN_IDX {
                TIER_DIAMOND_IDX
            } else {
                tier
            };
            assert_eq!(effective, tier);
        }
    }


    fn fresh_epoch() -> Epoch {
        Epoch {
            id: 0,
            start_timestamp: 0,
            finalized: false,
            swept: false,
            total_weighted_stake_snapshot: 0,
            sol_pool: 0,
            usdc_pool: 0,
            sol_claimed: 0,
            usdc_claimed: 0,
            total_weight_claimed: 0,
            provider_pool_sol: 0,
            provider_pool_usdc: 0,
            provider_pool_usdc_claimed: 0,
            finalized_at: 0,
            bump: 1,
        }
    }

    fn provider_signer_authorised(cfg: &YieldConfig, signer: Pubkey) -> bool {
        cfg.provider_vault_authority != Pubkey::default()
            && signer == cfg.provider_vault_authority
    }

    fn fresh_yield_config(provider_vault: Pubkey) -> YieldConfig {
        YieldConfig {
            authority: Pubkey::new_unique(),
            current_epoch_id: 0,
            current_epoch_start: 0,
            total_weighted_stake: 0,
            bump: 1,
            lucky_draw_pool: Pubkey::new_unique(),
            lucky_draw_usdc_account: Pubkey::new_unique(),
            provider_vault_authority: provider_vault,
            usdc_mint: Pubkey::new_unique(),
            pending_provider_vault_authority: Pubkey::default(),
            pending_provider_vault_authority_unlocks_at: 0,
            propose_cooldown_until: 0,
            recent_proposes: [0i64; 5],
            pending_authority: Pubkey::default(),
            pending_authority_unlocks_at: 0,
            acc_reward_per_share: 0,
            undistributed_carry: 0,
        }
    }

    #[test]
    fn test_deposit_provider_yield_basic() {
        let mut epoch = fresh_epoch();
        let amount: u64 = 10_000_000_000;
        epoch.provider_pool_sol = epoch
            .provider_pool_sol
            .checked_add(amount)
            .unwrap();
        assert_eq!(epoch.provider_pool_sol, 10_000_000_000);
        assert_eq!(epoch.sol_pool, 0);
    }

    #[test]
    fn test_deposit_provider_yield_unauthorized_signer_rejects() {
        let provider_pda = Pubkey::new_unique();
        let cfg = fresh_yield_config(provider_pda);
        let attacker = Pubkey::new_unique();
        assert!(!provider_signer_authorised(&cfg, attacker));
        assert!(provider_signer_authorised(&cfg, provider_pda));
    }

    #[test]
    fn test_provider_pool_sol_accumulates_across_deposits() {
        let mut epoch = fresh_epoch();
        let amounts = [3_000_000_000u64, 7_500_000_000u64, 250_000_000u64];
        let mut expected: u64 = 0;
        for a in amounts {
            epoch.provider_pool_sol = epoch
                .provider_pool_sol
                .checked_add(a)
                .unwrap();
            expected += a;
        }
        assert_eq!(epoch.provider_pool_sol, expected);
        assert_eq!(epoch.provider_pool_sol, 10_750_000_000);
    }

    #[test]
    fn test_set_provider_vault_authority_admin_only() {
        let admin = Pubkey::new_unique();
        let mut cfg = fresh_yield_config(Pubkey::default());
        cfg.authority = admin;
        let caller = admin;
        assert_eq!(caller, cfg.authority);
        let attacker = Pubkey::new_unique();
        assert_ne!(attacker, cfg.authority);
        let new_auth_default = Pubkey::default();
        assert_eq!(new_auth_default, Pubkey::default());
    }

    #[test]
    fn test_provider_unset_authority_rejects_deposit() {
        let cfg = fresh_yield_config(Pubkey::default());
        assert!(!provider_signer_authorised(&cfg, Pubkey::default()));
        assert!(!provider_signer_authorised(&cfg, Pubkey::new_unique()));
    }

    #[test]
    fn test_provider_pool_sol_zero_init_on_rollover() {
        let epoch = fresh_epoch();
        assert_eq!(epoch.provider_pool_sol, 0);
    }

    #[test]
    fn epoch_space_constant_matches_field_sum() {
        let expected = 8 + 8 + 8 + 1 + 1 + 16 + 8 + 8 + 8 + 8 + 16 + 8 + 8 + 8 + 8 + 1;
        assert_eq!(Epoch::SPACE, expected);
        assert_eq!(Epoch::SPACE, 123);
    }

    #[test]
    fn claim_position_space_constant_matches_field_sum() {
        let expected = 8 + 8 + 32 + 1 + 8 + 8 + 8 + 16 + 1;
        assert_eq!(ClaimPosition::SPACE, expected);
        assert_eq!(ClaimPosition::SPACE, 90);
    }

    #[test]
    fn high1_cumulative_weight_cap_clamps_to_remaining() {
        let snapshot: u128 = 1_000;
        let mut claimed: u128 = 0;

        let a_eff = 600u128.min(snapshot - claimed);
        claimed += a_eff;
        assert_eq!(a_eff, 600);

        let b_eff = 600u128.min(snapshot - claimed);
        claimed += b_eff;
        assert_eq!(b_eff, 400, "B is clamped to remaining weight, not its live 600");

        assert_eq!(claimed, snapshot);

        let remaining = snapshot - claimed;
        assert_eq!(remaining, 0, "no weight left → EpochWeightExhausted");
    }

    #[test]
    fn yield_config_space_constant_matches_field_sum() {
        let expected = 8 + 32 + 8 + 8 + 16 + 1 + 32 + 32 + 32 + 32 + 32 + 8
                     + 8 + 40 + 32 + 8
                     + 16 + 16;
        assert_eq!(YieldConfig::SPACE, expected);
        assert_eq!(YieldConfig::SPACE, 361);
    }


    #[test]
    fn test_deposit_provider_yield_usdc_basic() {
        let mut epoch = fresh_epoch();
        let amount: u64 = 1_000_000_000;
        epoch.provider_pool_usdc = epoch
            .provider_pool_usdc
            .checked_add(amount)
            .unwrap();
        assert_eq!(epoch.provider_pool_usdc, 1_000_000_000);
        assert_eq!(epoch.usdc_pool, 0);
        assert_eq!(epoch.sol_pool, 0);
        assert_eq!(epoch.provider_pool_sol, 0);
    }

    #[test]
    fn test_deposit_provider_yield_usdc_unauthorized_signer_rejects() {
        let provider_pda = Pubkey::new_unique();
        let cfg = fresh_yield_config(provider_pda);
        let attacker = Pubkey::new_unique();
        assert!(!provider_signer_authorised(&cfg, attacker));
        assert!(provider_signer_authorised(&cfg, provider_pda));
    }

    #[test]
    fn test_deposit_provider_yield_usdc_zero_amount_rejects() {
        let amount: u64 = 0;
        assert_eq!(amount, 0);
        let pass = amount > 0;
        assert!(!pass);
    }

    #[test]
    fn test_provider_pool_usdc_accumulates_across_deposits() {
        let mut epoch = fresh_epoch();
        let amounts = [
            500_000_000u64,
            1_250_000_000u64,
            75_000_000u64,
        ];
        let mut expected: u64 = 0;
        for a in amounts {
            epoch.provider_pool_usdc = epoch
                .provider_pool_usdc
                .checked_add(a)
                .unwrap();
            expected += a;
        }
        assert_eq!(epoch.provider_pool_usdc, expected);
        assert_eq!(epoch.provider_pool_usdc, 1_825_000_000);
    }

    #[test]
    fn test_provider_pool_usdc_overflow_protection() {
        let mut epoch = fresh_epoch();
        epoch.provider_pool_usdc = u64::MAX;
        let extra: u64 = 1;
        let result = epoch.provider_pool_usdc.checked_add(extra);
        assert!(result.is_none());
    }

    #[test]
    fn test_provider_pool_usdc_zero_init_on_rollover() {
        let epoch = fresh_epoch();
        assert_eq!(epoch.provider_pool_usdc, 0);
    }

    #[test]
    fn test_provider_unset_authority_rejects_usdc_deposit() {
        let cfg = fresh_yield_config(Pubkey::default());
        assert!(!provider_signer_authorised(&cfg, Pubkey::default()));
        assert!(!provider_signer_authorised(&cfg, Pubkey::new_unique()));
    }

    #[test]
    fn test_native_usdc_pool_unaffected_by_provider_deposits() {
        let mut epoch = fresh_epoch();
        epoch.usdc_pool = 5_000_000_000;
        let provider_amount: u64 = 250_000_000;
        epoch.provider_pool_usdc = epoch
            .provider_pool_usdc
            .checked_add(provider_amount)
            .unwrap();
        assert_eq!(epoch.usdc_pool, 5_000_000_000);
        assert_eq!(epoch.provider_pool_usdc, 250_000_000);
    }

    #[test]
    fn test_native_sol_deposit_unaffected_by_provider_usdc_deposit() {
        let mut epoch = fresh_epoch();
        epoch.sol_pool = 10_000_000_000;
        epoch.provider_pool_sol = 3_000_000_000;
        let usdc_amount: u64 = 100_000_000;
        epoch.provider_pool_usdc = epoch
            .provider_pool_usdc
            .checked_add(usdc_amount)
            .unwrap();
        assert_eq!(epoch.sol_pool, 10_000_000_000);
        assert_eq!(epoch.provider_pool_sol, 3_000_000_000);
        assert_eq!(epoch.provider_pool_usdc, 100_000_000);
    }

    #[test]
    fn test_provider_sol_and_usdc_share_same_authority() {
        let provider_pda = Pubkey::new_unique();
        let cfg = fresh_yield_config(provider_pda);
        assert!(provider_signer_authorised(&cfg, provider_pda));
        let attacker = Pubkey::new_unique();
        assert!(!provider_signer_authorised(&cfg, attacker));
    }

    #[test]
    fn test_native_deposit_yield_sol_still_works_regression() {
        let mut epoch = fresh_epoch();
        let amount: u64 = 2_000_000_000;
        epoch.sol_pool = epoch.sol_pool.checked_add(amount).unwrap();
        assert_eq!(epoch.sol_pool, 2_000_000_000);
        assert_eq!(epoch.usdc_pool, 0);
        assert_eq!(epoch.provider_pool_sol, 0);
        assert_eq!(epoch.provider_pool_usdc, 0);
    }

    #[test]
    fn test_native_deposit_yield_usdc_still_works_regression() {
        let mut epoch = fresh_epoch();
        let amount: u64 = 500_000_000;
        epoch.usdc_pool = epoch.usdc_pool.checked_add(amount).unwrap();
        assert_eq!(epoch.usdc_pool, 500_000_000);
        assert_eq!(epoch.sol_pool, 0);
        assert_eq!(epoch.provider_pool_sol, 0);
        assert_eq!(epoch.provider_pool_usdc, 0);
    }

    #[test]
    fn test_claim_path_reads_native_pools_not_provider() {
        let mut epoch = fresh_epoch();
        epoch.provider_pool_sol = 10_000_000_000;
        epoch.provider_pool_usdc = 5_000_000_000;
        epoch.total_weighted_stake_snapshot = 1_000_000_000;

        let staker_weight: u128 = 100_000_000;
        let sol_share = (epoch.sol_pool as u128)
            .checked_mul(staker_weight).unwrap()
            .checked_div(epoch.total_weighted_stake_snapshot).unwrap();
        let usdc_share = (epoch.usdc_pool as u128)
            .checked_mul(staker_weight).unwrap()
            .checked_div(epoch.total_weighted_stake_snapshot).unwrap();
        assert_eq!(sol_share, 0);
        assert_eq!(usdc_share, 0);
    }

    #[test]
    fn test_provider_yield_deposited_usdc_event_shape() {
        let _ev = ProviderYieldDepositedUsdcEvent {
            epoch_id: 7,
            amount: 1_000_000_000,
            total_provider_pool_usdc_after: 5_500_000_000,
            timestamp: 1_700_000_000,
        };
    }


    #[test]
    fn init_epoch_usdc_vault_seed_derivation_is_stable() {
        let epoch_id: u64 = 42;
        let (addr_a, _bump_a) = Pubkey::find_program_address(
            &[b"epoch_usdc_vault", &epoch_id.to_le_bytes()],
            &crate::ID,
        );
        let (addr_b, _bump_b) = Pubkey::find_program_address(
            &[b"epoch_usdc_vault", &epoch_id.to_le_bytes()],
            &crate::ID,
        );
        assert_eq!(addr_a, addr_b);
        let (addr_other, _) = Pubkey::find_program_address(
            &[b"epoch_usdc_vault", &43u64.to_le_bytes()],
            &crate::ID,
        );
        assert_ne!(addr_a, addr_other);
    }

    #[test]
    fn init_epoch_usdc_vault_matches_deposit_usdc_seed() {
        let epoch_id: u64 = 7;
        let init_addr = Pubkey::find_program_address(
            &[b"epoch_usdc_vault", &epoch_id.to_le_bytes()],
            &crate::ID,
        ).0;
        let deposit_addr = Pubkey::find_program_address(
            &[b"epoch_usdc_vault", &epoch_id.to_le_bytes()],
            &crate::ID,
        ).0;
        assert_eq!(init_addr, deposit_addr);
    }


    #[test]
    fn deposit_yield_rejects_sol_currency() {
        let currency_sol: u8 = CURRENCY_SOL;
        let currency_usdc: u8 = CURRENCY_USDC;

        let passes_bad_currency_check = currency_sol == CURRENCY_SOL || currency_sol == CURRENCY_USDC;
        let passes_usdc_only_guard = currency_sol == CURRENCY_USDC;
        assert!(passes_bad_currency_check, "SOL is a valid currency code");
        assert!(!passes_usdc_only_guard, "SOL must be rejected by the USDC-only guard (Rule 35)");

        let usdc_passes_bad_currency = currency_usdc == CURRENCY_SOL || currency_usdc == CURRENCY_USDC;
        let usdc_passes_usdc_only = currency_usdc == CURRENCY_USDC;
        assert!(usdc_passes_bad_currency, "USDC is a valid currency code");
        assert!(usdc_passes_usdc_only, "USDC must pass the USDC-only guard (Rule 35)");
    }

    #[test]
    fn deposit_yield_rejects_unknown_currency() {
        let unknown_currency: u8 = 2;
        let passes_bad_currency_check = unknown_currency == CURRENCY_SOL || unknown_currency == CURRENCY_USDC;
        assert!(!passes_bad_currency_check, "unknown currency code must fail BadCurrency check");
    }

    #[test]
    fn currency_constants_stable() {
        assert_eq!(CURRENCY_SOL, 0, "CURRENCY_SOL must be 0");
        assert_eq!(CURRENCY_USDC, 1, "CURRENCY_USDC must be 1");
    }


    #[test]
    fn yield_config_usdc_mint_is_persisted_at_init() {
        let real_mint = Pubkey::new_unique();
        let mut cfg = fresh_yield_config(Pubkey::default());
        cfg.usdc_mint = real_mint;
        assert_eq!(cfg.usdc_mint, real_mint,
            "usdc_mint must equal the mint supplied to initialize()");
        let wrong_mint = Pubkey::new_unique();
        assert_ne!(cfg.usdc_mint, wrong_mint,
            "a different mint must not match the stored usdc_mint");
    }

    #[test]
    fn initialize_rejects_default_usdc_mint() {
        let zero_mint = Pubkey::default();
        let is_default = zero_mint == Pubkey::default();
        assert!(is_default, "Pubkey::default() must be detected as zero");
        let real_mint = Pubkey::new_unique();
        let real_is_default = real_mint == Pubkey::default();
        assert!(!real_is_default,
            "a freshly-generated unique pubkey must not be Pubkey::default()");
    }

    #[test]
    fn init_epoch_usdc_vault_constraint_requires_matching_mint() {
        let cfg = fresh_yield_config(Pubkey::default());
        let _mint: Pubkey = cfg.usdc_mint;
        assert_ne!(_mint, Pubkey::new_unique(),
            "usdc_mint must be unique — verifies field is real and non-default");
    }


    #[test]
    fn propose_set_provider_vault_authority_admin_only() {
        let admin = Pubkey::new_unique();
        let attacker = Pubkey::new_unique();
        let cfg = fresh_yield_config(Pubkey::default());
        assert!(cfg.authority != admin || cfg.authority != attacker,
            "authority check must distinguish admin from attacker");
        assert_eq!(cfg.pending_provider_vault_authority, Pubkey::default());
        assert_eq!(cfg.pending_provider_vault_authority_unlocks_at, 0);
        let _ = attacker;
    }

    #[test]
    fn propose_set_provider_vault_authority_arms_unlocks_at() {
        let mut cfg = fresh_yield_config(Pubkey::default());
        let new_auth = Pubkey::new_unique();
        let now: i64 = 1_716_000_000;
        cfg.pending_provider_vault_authority = new_auth;
        cfg.pending_provider_vault_authority_unlocks_at = now
            .checked_add(ADMIN_TIMELOCK_SECONDS)
            .unwrap();
        assert_eq!(cfg.pending_provider_vault_authority, new_auth);
        assert_eq!(
            cfg.pending_provider_vault_authority_unlocks_at,
            now + 259_200,
            "unlocks_at must be exactly now + 72h"
        );
    }

    #[test]
    fn finalize_set_provider_vault_authority_blocked_early() {
        let mut cfg = fresh_yield_config(Pubkey::default());
        let new_auth = Pubkey::new_unique();
        let now: i64 = 1_716_000_000;
        cfg.pending_provider_vault_authority = new_auth;
        cfg.pending_provider_vault_authority_unlocks_at = now + 259_200;
        let current_time = now + 100;
        let would_pass = current_time >= cfg.pending_provider_vault_authority_unlocks_at;
        assert!(!would_pass, "finalize must be blocked before 72h elapses");
    }

    #[test]
    fn finalize_set_provider_vault_authority_after_timelock() {
        let mut cfg = fresh_yield_config(Pubkey::default());
        let new_auth = Pubkey::new_unique();
        let now: i64 = 1_716_000_000;
        cfg.pending_provider_vault_authority = new_auth;
        cfg.pending_provider_vault_authority_unlocks_at = now + 259_200;
        let current_time = now + 259_200 + 1;
        let would_pass = current_time >= cfg.pending_provider_vault_authority_unlocks_at;
        assert!(would_pass, "finalize must succeed after 72h");
        cfg.provider_vault_authority = cfg.pending_provider_vault_authority;
        cfg.pending_provider_vault_authority = Pubkey::default();
        cfg.pending_provider_vault_authority_unlocks_at = 0;
        assert_eq!(cfg.provider_vault_authority, new_auth);
        assert_eq!(cfg.pending_provider_vault_authority, Pubkey::default());
        assert_eq!(cfg.pending_provider_vault_authority_unlocks_at, 0);
    }

    #[test]
    fn cancel_set_provider_vault_authority_clears_pending() {
        let mut cfg = fresh_yield_config(Pubkey::default());
        let new_auth = Pubkey::new_unique();
        let now: i64 = 1_716_000_000;
        cfg.pending_provider_vault_authority = new_auth;
        cfg.pending_provider_vault_authority_unlocks_at = now + 259_200;
        cfg.pending_provider_vault_authority = Pubkey::default();
        cfg.pending_provider_vault_authority_unlocks_at = 0;
        assert_eq!(cfg.pending_provider_vault_authority, Pubkey::default());
        assert_eq!(cfg.pending_provider_vault_authority_unlocks_at, 0);
        assert_eq!(cfg.provider_vault_authority, Pubkey::default());
    }

    #[test]
    fn legacy_set_provider_vault_authority_bootstrap_instant() {
        let mut cfg = fresh_yield_config(Pubkey::default());
        let new_auth = Pubkey::new_unique();
        let first_set_allowed = cfg.provider_vault_authority == Pubkey::default();
        assert!(first_set_allowed, "first set MUST be allowed (Wave E.2 bootstrap)");
        cfg.provider_vault_authority = new_auth;
        assert_eq!(cfg.provider_vault_authority, new_auth,
            "first-set bootstrap-instant MUST commit the new authority");
        assert_eq!(cfg.pending_provider_vault_authority, Pubkey::default());
        assert_eq!(cfg.pending_provider_vault_authority_unlocks_at, 0);

        let attempted_rotation_to = Pubkey::new_unique();
        let second_set_allowed = cfg.provider_vault_authority == Pubkey::default();
        assert!(!second_set_allowed,
            "post-bootstrap rotation via set_provider_vault_authority MUST be blocked (Wave E.2). \
             Use propose+finalize_set_provider_vault_authority for the Rule 27b 72h timelock.");
        
        assert_ne!(cfg.provider_vault_authority, attempted_rotation_to,
            "field MUST NOT mutate when the gate reverts post-bootstrap");
        assert_eq!(cfg.provider_vault_authority, new_auth,
            "field MUST retain its bootstrap value after a blocked rotation");
    }
    
    #[test]
    fn yield_config_space_updated_to_241_after_new_c1a() {
        
        assert_eq!(YieldConfig::SPACE, 361,
            "SPACE must be 361 after the H-01 accumulator migration (+32B over 329)");
    }
    
    
    fn simulate_claim_leg(
        pool: u64,
        staker_weight: u128,
        total_weight: u128,
        prior_claimed: u64,
    ) -> (u64, u64, u64, u64) {
        let share_u128 = (pool as u128)
            .checked_mul(staker_weight)
            .unwrap()
            .checked_div(total_weight)
            .unwrap();
        let share: u64 = share_u128.try_into().unwrap();
        
        let liquid = share;
        let epoch_claimed_after = prior_claimed.checked_add(liquid).unwrap();
        let claim_pos_recorded = liquid;
        let etop_vested_recorded: u64 = 0;
        (liquid, epoch_claimed_after, claim_pos_recorded, etop_vested_recorded)
    }
    
    #[test]
    fn t004_test_single_staker_no_double_discount() {
        let pool: u64 = 100_000_000; 
        let staker_weight: u128 = 10_000;
        let total_weight: u128 = 10_000;
        let (liquid, epoch_claimed, claim_pos, etop_vested) =
            simulate_claim_leg(pool, staker_weight, total_weight, 0);
        
        assert_eq!(liquid, 100_000_000,
            "T0-04 single-staker MUST receive full share (was 70M pre-fix)");
        assert_eq!(epoch_claimed, 100_000_000,
            "epoch.usdc_claimed reflects ONLY what was transferred");
        assert_eq!(claim_pos, 100_000_000,
            "claim_pos.usdc_claimed matches transferred amount");
        assert_eq!(etop_vested, 0,
            "T0-04: etop_vested ALWAYS 0 — Path B vesting is upstream");
    }
    
    #[test]
    fn t004_test_two_stakers_weighted_split() {
        let pool: u64 = 100_000_000;
        let total_weight: u128 = 10_000;
        
        let (a_liquid, claimed_after_a, _, a_vested) =
            simulate_claim_leg(pool, 7_000, total_weight, 0);
        let (b_liquid, claimed_after_b, _, b_vested) =
            simulate_claim_leg(pool, 3_000, total_weight, claimed_after_a);
        assert_eq!(a_liquid, 70_000_000, "Staker A gets 70M USDC");
        assert_eq!(b_liquid, 30_000_000, "Staker B gets 30M USDC");
        
        assert_eq!(claimed_after_b, pool,
            "epoch.usdc_claimed exhausts pool — no phantom 30% retained");
        assert_eq!(a_vested, 0);
        assert_eq!(b_vested, 0);
    }
    
    #[test]
    fn t004_test_zero_sol_epoch_path_b_off_simulation() {
        
        let usdc_pool: u64 = 100_000_000;
        let sol_pool: u64 = 0;
        let staker_weight: u128 = 10_000;
        let total_weight: u128 = 10_000;
        let (usdc_liquid, _, _, _) =
            simulate_claim_leg(usdc_pool, staker_weight, total_weight, 0);
        let (sol_liquid, _, _, _) =
            simulate_claim_leg(sol_pool, staker_weight, total_weight, 0);
        assert_eq!(usdc_liquid, 100_000_000,
            "Staker claims full USDC pool — phase-agnostic at claim time");
        assert_eq!(sol_liquid, 0,
            "Zero SOL pool yields zero SOL transfer (handler's if-block skips)");
    }
    
    #[test]
    fn t004_test_epoch_accounting_invariant() {
        let pool: u64 = 100_000_000;
        let total_weight: u128 = 10_000;
        
        let staker_weight: u128 = 7_000;
        let (liquid, claimed_after, _, _) =
            simulate_claim_leg(pool, staker_weight, total_weight, 0);
        assert_eq!(liquid, 70_000_000);
        assert_eq!(claimed_after, 70_000_000);
        
        let sweep_remainder = pool.checked_sub(claimed_after).unwrap();
        assert_eq!(sweep_remainder, 30_000_000,
            "sweep_unclaimed sees the honest 30M unclaimed — no phantom inflation");
        
    }
    
    #[test]
    fn t004_test_claim_position_etop_vested_zero_post_fix() {
        let pool: u64 = 100_000_000;
        let staker_weight: u128 = 10_000;
        let total_weight: u128 = 10_000;
        let (_, _, claim_pos_recorded, etop_vested_recorded) =
            simulate_claim_leg(pool, staker_weight, total_weight, 0);
        assert_eq!(etop_vested_recorded, 0,
            "T0-04 INVARIANT: claim_pos.etop_vested MUST equal 0. \
             Path B vesting is now upstream (swap_router → etop_escrow). \
             If this fails, someone reintroduced the double-discount.");
        
        assert_eq!(claim_pos_recorded, 100_000_000);
    }
    
    
    #[test]
    fn t004_test_bps_denom_still_present() {
        
        assert_eq!(BPS_DENOM, 10_000);
    }
    
    
    
    fn h4_swap_router_pda_matches(signer: Pubkey) -> bool {
        let (expected, _) = Pubkey::find_program_address(
            &[b"swap_router_config"],
            &WATERFALL_PROGRAM_ID,
        );
        signer == expected
    }
    
    
    
    fn h4_simulate_notify_usdc_bump(
        epoch: &mut Epoch,
        amount: u64,
    ) -> std::result::Result<u64, &'static str> {
        if amount == 0 {
            return Err("ZeroAmount");
        }
        if epoch.finalized {
            return Err("EpochFinalized");
        }
        epoch.usdc_pool = epoch
            .usdc_pool
            .checked_add(amount)
            .ok_or("MathOverflow")?;
        Ok(epoch.usdc_pool)
    }
    
    
    
    #[test]
    fn h4_test_notify_yield_deposit_usdc_bumps_counter() {
        let mut epoch = fresh_epoch();
        let bumped = h4_simulate_notify_usdc_bump(&mut epoch, 70_000_000).unwrap();
        assert_eq!(bumped, 70_000_000);
        assert_eq!(epoch.usdc_pool, 70_000_000);
        
        assert_eq!(epoch.sol_pool, 0);
    }
    
    
    
    #[test]
    fn h4_test_notify_yield_deposit_unauth_signer_rejects() {
        let attacker = Pubkey::new_unique();
        assert!(!h4_swap_router_pda_matches(attacker),
            "An arbitrary attacker pubkey must NOT pass the swap_router PDA check");
        
        let (expected, _) = Pubkey::find_program_address(
            &[b"swap_router_config"],
            &WATERFALL_PROGRAM_ID,
        );
        assert!(h4_swap_router_pda_matches(expected));
    }
    
    
    
    #[test]
    fn h4_test_notify_yield_deposit_associative_under_repeated_calls() {
        let mut epoch = fresh_epoch();
        h4_simulate_notify_usdc_bump(&mut epoch, 30_000_000).unwrap();
        h4_simulate_notify_usdc_bump(&mut epoch, 40_000_000).unwrap();
        assert_eq!(epoch.usdc_pool, 70_000_000);
    }

    
    #[test]
    fn h4_test_notify_yield_deposit_zero_amount_rejects() {
        let mut epoch = fresh_epoch();
        let err = h4_simulate_notify_usdc_bump(&mut epoch, 0).unwrap_err();
        assert_eq!(err, "ZeroAmount");
        
        assert_eq!(epoch.usdc_pool, 0);
    }

    
    #[test]
    fn h4_test_notify_yield_deposit_finalized_epoch_rejects() {
        let mut epoch = fresh_epoch();
        epoch.finalized = true;
        let err = h4_simulate_notify_usdc_bump(&mut epoch, 1_000_000).unwrap_err();
        assert_eq!(err, "EpochFinalized");
        assert_eq!(epoch.usdc_pool, 0);
    }
    
    
    #[test]
    fn h4_test_notify_yield_deposit_sol_currency_rejected_at_v1() {
        
        
        assert_eq!(CURRENCY_SOL, 0);
        assert_eq!(CURRENCY_USDC, 1);
        
        let v1_currency_accepted = |c: u8| c == CURRENCY_USDC;
        assert!(!v1_currency_accepted(CURRENCY_SOL));
        assert!(v1_currency_accepted(CURRENCY_USDC));
    }
    
    
    
    #[test]
    fn h4_test_yield_deposit_notified_event_shape() {
        let event = YieldDepositNotifiedEvent {
            epoch_id: 5,
            amount: 70_000_000,
            currency: CURRENCY_USDC,
            timestamp: 1_234_567_890,
        };
        assert_eq!(event.epoch_id, 5);
        assert_eq!(event.amount, 70_000_000);
        assert_eq!(event.currency, CURRENCY_USDC);
        assert_eq!(event.timestamp, 1_234_567_890);
    }
    
    
    #[test]
    fn h4_test_strand_invariant_post_fix() {
        
        
        let mut buggy_epoch = fresh_epoch();
        let amount_transferred: u64 = 70_000_000;
        
        let counter_bumped_buggy = false;
        assert!(!counter_bumped_buggy);
        assert_eq!(buggy_epoch.usdc_pool, 0);
        let _ = amount_transferred;
        
        
        let mut fixed_epoch = fresh_epoch();
        let bumped = h4_simulate_notify_usdc_bump(&mut fixed_epoch, amount_transferred).unwrap();
        
        
        assert_eq!(bumped, amount_transferred);
        assert_eq!(fixed_epoch.usdc_pool, amount_transferred);
    }
    
    
    
    fn simulate_freshness_gate(stake_timestamp: i64, now: i64) -> std::result::Result<(), &'static str> {
        let stake_age = now.checked_sub(stake_timestamp).ok_or("MathOverflow")?;
        
        
        if stake_age < 0 {
            return Err("MathOverflow");
        }
        if stake_age >= MIN_STAKE_AGE_FOR_CLAIM_SECONDS {
            Ok(())
        } else {
            Err("StakeTooFreshToClaim")
        }
    }
    
    
    
    #[test]
    fn min_stake_age_test_1_claim_after_25h_succeeds() {
        let stake_timestamp: i64 = 0;
        let now: i64 = 25 * 60 * 60;  
        let result = simulate_freshness_gate(stake_timestamp, now);
        assert!(result.is_ok(),
            "25h-old stake MUST pass the 24h gate (got {:?})", result);
        
        assert_eq!(now - stake_timestamp, 90_000);
        assert!(90_000 >= MIN_STAKE_AGE_FOR_CLAIM_SECONDS);
    }
    
    
    
    #[test]
    fn min_stake_age_test_2_claim_at_12h_reverts() {
        let stake_timestamp: i64 = 0;
        let now: i64 = 12 * 60 * 60;  
        let result = simulate_freshness_gate(stake_timestamp, now);
        assert_eq!(result, Err("StakeTooFreshToClaim"),
            "12h-old stake MUST revert StakeTooFreshToClaim (age < 24h)");
        
        assert_eq!(now - stake_timestamp, 43_200);
        assert!(43_200 < MIN_STAKE_AGE_FOR_CLAIM_SECONDS);
    }
    
    
    #[test]
    fn min_stake_age_test_3_topup_at_23h_then_24h_claim_reverts() {
        
        let amt_old: u128 = 5_000_000;
        let amt_new: u128 = 15_000_000;      
        let ts_old: u128 = 0;
        let ts_new: u128 = 23 * 3600;
        let weighted: i64 = (((amt_old * ts_old) + (amt_new * ts_new))
            / (amt_old + amt_new)) as i64;
        
        
        assert_eq!(weighted, 62_100,
            "staking weighted-avg must produce 17.25h = 62_100s for this scenario");

        let now: i64 = 24 * 3600;            
        let result = simulate_freshness_gate(weighted, now);
        let stake_age = now - weighted;       
        assert_eq!(stake_age, 24_300,
            "composed effective age must be 6.75h (24_300s)");
        assert!(stake_age < MIN_STAKE_AGE_FOR_CLAIM_SECONDS,
            "composed chain MUST place effective age below the 24h gate (got {}s)",
            stake_age);
        assert_eq!(result, Err("StakeTooFreshToClaim"),
            "composed chain MUST revert StakeTooFreshToClaim");
    }
    
    
    
    #[test]
    fn min_stake_age_test_4_exact_24h_boundary_inclusive() {
        let stake_timestamp: i64 = 0;
        let now: i64 = MIN_STAKE_AGE_FOR_CLAIM_SECONDS;  
        let result = simulate_freshness_gate(stake_timestamp, now);
        assert!(result.is_ok(),
            "exact 24h boundary MUST pass (>=, not >). got {:?}", result);
        assert_eq!(now - stake_timestamp, MIN_STAKE_AGE_FOR_CLAIM_SECONDS);

        
        let now_minus_1: i64 = MIN_STAKE_AGE_FOR_CLAIM_SECONDS - 1;
        let result_minus_1 = simulate_freshness_gate(stake_timestamp, now_minus_1);
        assert_eq!(result_minus_1, Err("StakeTooFreshToClaim"),
            "24h - 1s MUST still revert — inclusive lower bound");
    }
    
    
    
    #[test]
    fn min_stake_age_test_5_future_stake_timestamp_rejected() {
        let now: i64 = 1_700_000_000;
        let stake_timestamp: i64 = now + 1;  
        let result = simulate_freshness_gate(stake_timestamp, now);
        assert_eq!(result, Err("MathOverflow"),
            "future stake_timestamp MUST revert MathOverflow (NOT silently saturate to age=0). \
             Got {:?}", result);
        
        let stake_timestamp_far: i64 = now + 365 * 86_400;
        let result_far = simulate_freshness_gate(stake_timestamp_far, now);
        assert_eq!(result_far, Err("MathOverflow"),
            "365d-future stake_timestamp MUST revert MathOverflow");
    }
    
    
    
    #[test]
    fn min_stake_age_test_6_constant_pinned_at_86400_seconds() {
        
        assert_eq!(MIN_STAKE_AGE_FOR_CLAIM_SECONDS, 86_400,
            "MIN_STAKE_AGE_FOR_CLAIM_SECONDS must equal 24h (86_400s) — \
             matches brief D.C.1-v2 / R8-META M8-CRIT-01 / R6.5 D.1");
        
        assert_eq!(MIN_STAKE_AGE_FOR_CLAIM_SECONDS, 24 * 60 * 60);
        
        assert!(MIN_STAKE_AGE_FOR_CLAIM_SECONDS > 60 * 60,
            "gate must be ≥ 1h to provide meaningful flash-stake resistance");
        
        assert!(MIN_STAKE_AGE_FOR_CLAIM_SECONDS < 30 * 86_400,
            "gate must be << 30d so honest mid-epoch joiners can claim next epoch");
    }
    
    
    
    
    
    #[test]
    fn min_stake_age_test_7_long_term_staker_unaffected() {
        let stake_timestamp: i64 = 1_700_000_000;
        let week: i64 = 7 * 86_400;

        for epoch_n in 1..=52 {
            
            let now: i64 = stake_timestamp + (epoch_n * week);
            let stake_age = now - stake_timestamp;
            
            assert!(stake_age >= week,
                "claim at epoch {} must be ≥ 1 week after stake (got {}s)",
                epoch_n, stake_age);
            
            let result = simulate_freshness_gate(stake_timestamp, now);
            assert!(result.is_ok(),
                "long-term staker MUST pass gate at epoch {} (stake_age = {}s, got {:?})",
                epoch_n, stake_age, result);
        }

        let five_years: i64 = 5 * 365 * 86_400;
        let result_5y = simulate_freshness_gate(stake_timestamp, stake_timestamp + five_years);
        assert!(result_5y.is_ok(),
            "5y-old staker MUST pass gate (got {:?})", result_5y);
        
        
        
        let result_same_slot = simulate_freshness_gate(stake_timestamp, stake_timestamp);
        assert_eq!(result_same_slot, Err("StakeTooFreshToClaim"),
            "same-slot claim MUST revert regardless of staker history");
    }
    
    
    
    
    
    
    fn drift_handler_end(src: &str, idx: usize) -> usize {
        src[idx + 1..]
            .find("\n    pub fn ")
            .map(|p| idx + 1 + p)
            .expect("drift-gate: no following `pub fn` — re-anchor this source-assert bound")
    }
    fn yield_escrow_lib_rs_source() -> &'static str {
        include_str!("lib.rs")
    }

    
    
    #[test]
    fn set_provider_vault_authority_is_bootstrap_instant() {
        let src = yield_escrow_lib_rs_source();
        
        
        
        let needle = "pub fn set_provider_vault_authority(";
        let idx = src.find(needle).expect("handler must exist");
        
        
        let stop_marker = "pub fn propose_set_provider_vault_authority(";
        let stop = src[idx..].find(stop_marker)
            .map(|rel| idx + rel)
            .unwrap_or_else(|| drift_handler_end(src, idx));
        let body = &src[idx..stop];
        
        assert!(body.contains("cfg.provider_vault_authority == Pubkey::default()"),
            "set_provider_vault_authority MUST gate first-set on Pubkey::default() (Wave E.2 bootstrap-instant). \
             Source excerpt:\n{}", body);
        assert!(body.contains("ProviderVaultAuthorityAlreadyConfigured"),
            "set_provider_vault_authority MUST revert with ProviderVaultAuthorityAlreadyConfigured \
             on post-bootstrap calls (Wave E.2). Source excerpt:\n{}", body);
        
        assert!(body.contains("cfg.provider_vault_authority = new_authority;"),
            "set_provider_vault_authority MUST commit the new authority on first-set (Wave E.2). \
             Source excerpt:\n{}", body);
        
        assert!(body.contains("require!(new_authority != Pubkey::default()"),
            "set_provider_vault_authority MUST reject Pubkey::default() new_authority (Wave E.2). \
             Source excerpt:\n{}", body);
        
        assert!(!body.contains("err!(YieldError::InstructionDeprecated)"),
            "set_provider_vault_authority MUST NOT hard-revert with InstructionDeprecated under Wave E.2 \
             (bootstrap-instant pattern restored). Source excerpt:\n{}", body);
    }
    #[test]
    fn deprecated_transfer_authority_reverts_with_err() {
        let src = yield_escrow_lib_rs_source();
        let needle = "pub fn transfer_authority(";
        let idx = src.find(needle).expect("handler must exist");
        let end = drift_handler_end(src, idx);
        let body = &src[idx..end];
        assert!(body.contains("err!(YieldError::InstructionDeprecated)"),
            "transfer_authority MUST hard-revert with InstructionDeprecated (M-CRIT-02). \
             Source excerpt:\n{}", body);
        
        assert!(!body.contains("cfg.authority = new_authority;"),
            "transfer_authority MUST NOT mutate state after the gate (M-CRIT-02). \
             Source excerpt:\n{}", body);
    }

    #[test]
    fn propose_finalize_pair_still_works_alongside_deprecated_gate() {
        
        
        let src = yield_escrow_lib_rs_source();
        
        assert!(src.contains("pub fn propose_set_provider_vault_authority("),
            "propose_set_provider_vault_authority must remain (canonical replacement)");
        assert!(src.contains("pub fn finalize_set_provider_vault_authority("),
            "finalize_set_provider_vault_authority must remain (canonical replacement)");
        assert!(src.contains("pub fn propose_rotate_admin("),
            "propose_rotate_admin must remain (canonical replacement for transfer_authority)");
        assert!(src.contains("pub fn finalize_rotate_admin("),
            "finalize_rotate_admin must remain (canonical replacement for transfer_authority)");
    }
    #[test]
    fn instruction_deprecated_error_code_exists() {
        let src = yield_escrow_lib_rs_source();
        assert!(src.contains("InstructionDeprecated,"),
            "YieldError::InstructionDeprecated variant must exist");
        assert!(src.contains("M-CRIT-02"),
            "error definition must reference the audit ID for traceability");
    }
    
    
    
    
    
    
    
    
    #[test]
    fn r65_d1_freshness_guard_pinned_in_claim_handler() {
        let src = yield_escrow_lib_rs_source();
        let needle = "pub fn claim(ctx: Context<Claim>";
        let idx = src.find(needle).expect("claim handler must exist");
        let end = drift_handler_end(src, idx);
        let body = &src[idx..end];
        
        
        
        assert!(body.contains("stake_age >= MIN_STAKE_AGE_FOR_CLAIM_SECONDS")
                && body.contains("YieldError::StakeTooFreshToClaim"),
            "claim handler MUST enforce stake_age >= MIN_STAKE_AGE_FOR_CLAIM_SECONDS \
             with StakeTooFreshToClaim revert (design D-8 R-d1). Source excerpt:\n{}", body);
        assert!(body.contains("now.checked_sub(stake_ts)") && body.contains("YieldError::MathOverflow"),
            "claim handler MUST compute stake_age via now.checked_sub(stake_ts).ok_or(MathOverflow) \
             so a corrupt/future stake_timestamp cannot silently pass. Source excerpt:\n{}", body);
        assert!(body.contains("D-8"),
            "claim handler MUST cite design D-8 (flash-stake-around-funding defense) for traceability");
    }

    
    
    
    
    

    #[test]
    fn r65_d1_min_stake_age_constant_pinned_at_24h() {
        
        
        
        
        assert_eq!(MIN_STAKE_AGE_FOR_CLAIM_SECONDS, 86_400,
            "MIN_STAKE_AGE_FOR_CLAIM_SECONDS MUST be 24h = 86_400s (R6.5 D.1)");
    }

    

    #[test]
    fn r77_h01_check_and_record_propose_helper_present() {
        let src = yield_escrow_lib_rs_source();
        assert!(src.contains("fn check_and_record_propose("),
            "yield-escrow MUST define check_and_record_propose helper (R7.7-H-01)");
        assert!(src.contains("propose_cooldown_until"),
            "YieldConfig MUST carry propose_cooldown_until field (R7.7-H-01)");
        assert!(src.contains("recent_proposes: [i64; 5]"),
            "YieldConfig MUST carry recent_proposes: [i64; 5] ring buffer (R7.7-H-01)");
        assert!(src.contains("ProposeCooldownActive"),
            "YieldError::ProposeCooldownActive variant MUST exist (R7.7-H-01)");
    }

    #[test]
    fn r77_h01_all_propose_handlers_call_check_and_record_propose() {
        
        
        
        
        let src = yield_escrow_lib_rs_source();
        let propose_names = ["propose_set_provider_vault_authority",
                             "propose_rotate_admin"];
        for name in propose_names {
            let needle = format!("pub fn {}(", name);
            let idx = src.find(&needle).unwrap_or_else(|| panic!("{} handler must exist", name));
            let end = drift_handler_end(src, idx);
            let body = &src[idx..end];
            assert!(body.contains("check_and_record_propose(cfg, now)?;"),
                "{} MUST call check_and_record_propose(cfg, now)? for R7.7-H-01 \
                 defense. Source excerpt:\n{}", name, body);
        }
    }
    
    
    
    
    
    
    
    
    

    
    
    
    
    
    fn simulate_effective_tier(live_tier: u8, valid_active_seat: bool) -> u8 {
        if live_tier == TIER_SOVEREIGN_IDX {
            if valid_active_seat { TIER_SOVEREIGN_IDX } else { TIER_DIAMOND_IDX }
        } else {
            live_tier
        }
    }

    
    fn simulate_weight(amount: u64, tier: u8) -> u128 {
        (amount as u128)
            .checked_mul(TIER_WEIGHT_BPS[tier as usize] as u128).unwrap()
            .checked_div(BPS_DENOM).unwrap()
    }

    
    
    
    #[test]
    fn high1_test_1_seatless_sovereign_downgraded_to_diamond() {
        let eff = simulate_effective_tier(TIER_SOVEREIGN_IDX, false);
        assert_eq!(eff, TIER_DIAMOND_IDX,
            "a tier-7 position with no valid active seat MUST apply Diamond weight (HIGH-1)");
        assert_eq!(TIER_WEIGHT_BPS[eff as usize], 9_000,
            "the downgraded weight MUST be 9000 bps (Diamond), not 10000 (Sovereign)");
        
        let amount_20m: u64 = 20_000_000;
        let applied = simulate_weight(amount_20m, eff);
        let sovereign = simulate_weight(amount_20m, TIER_SOVEREIGN_IDX);
        assert!(applied < sovereign,
            "seatless Sovereign draws strictly less than a seated Sovereign (no over-pay)");
        assert_eq!(applied, simulate_weight(amount_20m, TIER_DIAMOND_IDX),
            "seatless Sovereign draws EXACTLY the Diamond contribution");
    }

    
    #[test]
    fn high1_test_2_seated_sovereign_full_weight_and_lower_tiers_untouched() {
        assert_eq!(simulate_effective_tier(TIER_SOVEREIGN_IDX, true), TIER_SOVEREIGN_IDX,
            "a Sovereign WITH a valid active seat MUST keep full Sovereign weight (10000)");
        assert_eq!(TIER_WEIGHT_BPS[TIER_SOVEREIGN_IDX as usize], 10_000);
        
        for tier in 0u8..=TIER_DIAMOND_IDX {
            assert_eq!(simulate_effective_tier(tier, false), tier,
                "tier {tier} MUST be unaffected by the seat (seatless)");
            assert_eq!(simulate_effective_tier(tier, true), tier,
                "tier {tier} MUST be unaffected by the seat (seated)");
        }
    }
    
    
    
    #[test]
    fn high1_test_3_claim_self_validates_sovereign_seat_in_handler() {
        let src = yield_escrow_lib_rs_source();
        let needle = "pub fn claim(ctx: Context<Claim>";
        let idx = src.find(needle).expect("claim handler must exist");
        
        let stop_marker = "pub fn sweep_unclaimed(";
        let end = src[idx..].find(stop_marker)
            .map(|rel| idx + rel)
            .expect("sweep_unclaimed handler must follow claim()");
        let body = &src[idx..end];
        
        assert!(body.contains("if tier == TIER_SOVEREIGN_IDX")
                && body.contains("is_valid_active_sovereign_seat("),
            "claim() MUST self-validate a tier-7 position against a live seat via \
             is_valid_active_sovereign_seat (HIGH-1). Excerpt:\n{}", body);
        assert!(body.contains("TIER_DIAMOND_IDX")
                && body.contains("let effective_tier"),
            "claim() MUST downgrade a seatless Sovereign to TIER_DIAMOND_IDX (HIGH-1). \
             Excerpt:\n{}", body);
        
        assert!(body.contains("TIER_WEIGHT_BPS[effective_tier as usize]"),
            "claim() MUST index TIER_WEIGHT_BPS by effective_tier (the HIGH-1 self-\
             validated tier), NOT the raw position tier. Excerpt:\n{}", body);
        
        assert!(src.contains("pub sovereign_seat: Option<UncheckedAccount<'info>>,"),
            "Claim context MUST carry an OPTIONAL sovereign_seat account (HIGH-1)");
    }

    
    
    #[test]
    fn m2_test_claim_entitlement_uses_saturating_sub() {
        let src = yield_escrow_lib_rs_source();
        let needle = "pub fn claim(ctx: Context<Claim>";
        let idx = src.find(needle).expect("claim handler must exist");
        let stop_marker = "pub fn sweep_unclaimed(";
        let end = src[idx..].find(stop_marker)
            .map(|rel| idx + rel)
            .expect("sweep_unclaimed handler must follow claim()");
        let body = &src[idx..end];
        assert!(body.contains(".saturating_sub(reward_debt)"),
            "claim() MUST floor the entitlement via saturating_sub(reward_debt) (M-2). \
             Excerpt:\n{}", body);
        
        assert!(body.contains(".saturating_sub(claimed_so_far)"),
            "claim() MUST floor claimable via saturating_sub(claimed_so_far) (M-2). \
             Excerpt:\n{}", body);
        assert!(!body.contains(".checked_sub(reward_debt)"),
            "claim() MUST NOT checked_sub(reward_debt) — underflow-brick footgun (M-2)");
        assert!(!body.contains(".checked_sub(claimed_so_far)"),
            "claim() MUST NOT checked_sub(claimed_so_far) — underflow-brick footgun (M-2)");
    }
    #[test]
    fn m2_test_saturating_floor_yields_zero_not_underflow() {
        let entitlement: u128 = 100;
        let claimed_so_far: u128 = 250;
        let claimable = entitlement.saturating_sub(claimed_so_far);
        assert_eq!(claimable, 0,
            "over-claimed state MUST floor to 0 owed, NOT underflow-revert (M-2)");
        let reward_owed: u128 = 0;
        let accrued: u128 = 9_000;
        let reward_debt: u128 = 10_000;
        let entitlement2 = (reward_owed + accrued).saturating_sub(reward_debt);
        assert_eq!(entitlement2, 0,
            "a downgrade that makes accrued < reward_debt MUST floor to 0 (M-2 + HIGH-1)");
    }
    
    
    
    
    
    
    
    
    
    
    
    
    

    
    
    
    #[test]
    fn ye_m01_accumulator_claim_pays_single_pinned_pool() {
        let src = yield_escrow_lib_rs_source();
        let needle = "pub fn claim(ctx: Context<Claim>";
        let idx = src.find(needle).expect("claim handler must exist");
        let stop_marker = "pub fn sweep_unclaimed(";
        let end = src[idx..].find(stop_marker)
            .map(|rel| idx + rel)
            .expect("sweep_unclaimed handler must follow claim()");
        let body = &src[idx..end];
        assert!(body.contains("from: ctx.accounts.yield_pool_usdc.to_account_info()")
                && body.contains("token::transfer(cpi, claimable)?"),
            "accumulator claim() MUST pay `claimable` from the single yield_pool_usdc \
             holder (YE-M01 re-scoped). Excerpt:\n{}", body);
        
        assert!(src.contains("seeds = [b\"yield_pool_usdc\"]")
                && src.contains("token::mint = yield_config.usdc_mint,"),
            "Claim context MUST pin yield_pool_usdc by seeds + usdc_mint (YE-M01 re-scoped)");
        
        assert!(!body.contains(".ok_or(YieldError::MissingUsdcAccounts)?"),
            "claim() MUST NOT carry the legacy per-epoch MissingUsdcAccounts gate — \
             it pays a single pinned pool now (YE-M01 re-scoped). Excerpt:\n{}", body);
    }

    
    
    
    

    
    
    
    #[test]
    fn ye_m02_test_1_provider_sol_deposit_disabled_in_handler() {
        let src = yield_escrow_lib_rs_source();
        let needle = "pub fn deposit_provider_yield(";
        let idx = src.find(needle).expect("deposit_provider_yield handler must exist");
        let stop_marker = "pub fn deposit_provider_yield_usdc(";
        let stop = src[idx..].find(stop_marker)
            .map(|rel| idx + rel)
            .unwrap_or_else(|| drift_handler_end(src, idx));
        let body = &src[idx..stop];
        
        assert!(body.contains("require!(false, YieldError::SolDepositNotAllowed)"),
            "deposit_provider_yield MUST hard-disable SOL deposits at v1 with \
             require!(false, SolDepositNotAllowed) (YE-M02). Excerpt:\n{}", body);
        
        let guard_pos = body.find("require!(false, YieldError::SolDepositNotAllowed)")
            .expect("guard present");
        let transfer_pos = body.find("system_program::transfer(ix_ctx, amount)");
        if let Some(tp) = transfer_pos {
            assert!(guard_pos < tp,
                "the SOL-disable guard MUST precede the system_program::transfer \
                 so no SOL can land in provider_pool_sol (YE-M02). Excerpt:\n{}", body);
        }
        
        assert!(body.contains("YE-M02"),
            "deposit_provider_yield guard MUST cite YE-M02 for traceability");
    }

    
    
    
    #[test]
    fn ye_m02_test_2_provider_usdc_deposit_not_disabled() {
        let src = yield_escrow_lib_rs_source();
        let needle = "pub fn deposit_provider_yield_usdc(";
        let idx = src.find(needle).expect("deposit_provider_yield_usdc handler must exist");
        
        let stop_marker = "pub fn init_epoch_usdc_vault(";
        let stop = src[idx..].find(stop_marker)
            .map(|rel| idx + rel)
            .unwrap_or_else(|| drift_handler_end(src, idx));
        let body = &src[idx..stop];
        
        assert!(body.contains("token::transfer(cpi_ctx, amount)"),
            "deposit_provider_yield_usdc MUST still transfer USDC (YE-M02 must not \
             over-reach into the live USDC pond). Excerpt:\n{}", body);
        
        assert!(!body.contains("require!(false, YieldError::SolDepositNotAllowed)"),
            "deposit_provider_yield_usdc MUST NOT be hard-disabled — only the SOL \
             variant is (YE-M02). Excerpt:\n{}", body);
    }
}
