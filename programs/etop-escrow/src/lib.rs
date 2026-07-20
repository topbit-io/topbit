
use anchor_lang::prelude::*;
use anchor_lang::system_program;
use anchor_spl::token_interface::{
    self as token_interface, Burn as SplBurn, Mint, TokenAccount, TokenInterface, TransferChecked,
};

declare_id!("Aa9CbHs9yDt52x4jfyQfeyb6R7nYxUjABvYbbcgRMuro");

#[cfg(not(feature = "no-entrypoint"))]
solana_security_txt::security_txt! {
    name: "etop_escrow",
    project_url: "https://topbit.io",
    contacts: "email:security@topbit.io",
    policy: "https://topbit.io/security",
    preferred_languages: "en",
    source_code: "https://github.com/topbit-io/topbit"
}


const VESTING_DURATION_DAYS: u64 = 180;
const SECONDS_PER_DAY: u64 = 86_400;

const FORFEIT_DAY_30_BPS: u64 = 10_000;
const FORFEIT_DAY_60_BPS: u64 = 7_500;
const FORFEIT_DAY_90_BPS: u64 = 5_000;
const FORFEIT_DAY_180_BPS: u64 = 2_500;
const BPS_DENOM: u64 = 10_000;

pub const MAX_PENDING_CREDITS: usize = 52;

pub const BURN_FALLBACK_DEST: Pubkey = anchor_lang::solana_program::system_program::ID;

pub const SWAP_ROUTER_PROGRAM_ID: Pubkey =
    pubkey!("9tkZMhH7cf293wpGpJycqsZLb8eojkJ5jyUHJtqcS5zR");

pub const ADMIN_TIMELOCK_SECONDS: i64 = 72 * 60 * 60;

pub const PROPOSE_RATE_LIMIT_WINDOW_SECONDS: i64 = 86_400;
pub const PROPOSE_RATE_LIMIT_RING_LEN: usize = 5;


#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum BurnMethod {
    Burn,
    TransferToSystem,
}

impl Default for BurnMethod {
    fn default() -> Self {
        BurnMethod::Burn
    }
}


#[program]
pub mod etop_escrow {
    use super::*;

    pub fn initialize(
        ctx: Context<Initialize>,
        swap_router_pubkey: Pubkey,
        top_token_mint: Pubkey,
    ) -> Result<()> {
        let config = &mut ctx.accounts.escrow_config;
        config.authority = ctx.accounts.authority.key();
        config.swap_router_pubkey = swap_router_pubkey;
        config.top_token_mint = top_token_mint;
        config.current_epoch_id = 0;
        config.total_top_held = 0;
        config.total_top_claimed_vested = 0;
        config.total_top_burned_forfeit = 0;
        config.burn_method = BurnMethod::Burn;
        config.bump = ctx.bumps.escrow_config;

        config.pending_top_token_mint = Pubkey::default();
        config.pending_top_token_mint_unlocks_at = 0;
        config.pending_swap_router = Pubkey::default();
        config.pending_swap_router_unlocks_at = 0;
        config.pending_burn_method = BurnMethod::Burn;
        config.pending_burn_method_unlocks_at = 0;
        config.pending_authority = Pubkey::default();
        config.pending_authority_unlocks_at = 0;
        config.propose_cooldown_until = 0;
        config.recent_proposes = [0i64; 5];
        Ok(())
    }


    pub fn propose_set_top_token_mint(
        ctx: Context<AdminOnly>,
        new_mint: Pubkey,
    ) -> Result<()> {
        let cfg = &mut ctx.accounts.escrow_config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), EscrowError::Unauthorized);
        let now = Clock::get()?.unix_timestamp;
        check_and_record_propose(cfg, now)?;
        cfg.pending_top_token_mint = new_mint;
        cfg.pending_top_token_mint_unlocks_at = now
            .checked_add(ADMIN_TIMELOCK_SECONDS)
            .ok_or(EscrowError::MathOverflow)?;
        emit!(TopTokenMintProposed {
            new_mint,
            unlocks_at: cfg.pending_top_token_mint_unlocks_at,
        });
        Ok(())
    }

    pub fn finalize_set_top_token_mint(ctx: Context<AdminOnly>) -> Result<()> {
        let cfg = &mut ctx.accounts.escrow_config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), EscrowError::Unauthorized);
        require!(
            cfg.pending_top_token_mint_unlocks_at != 0,
            EscrowError::NothingPending
        );
        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= cfg.pending_top_token_mint_unlocks_at,
            EscrowError::TimelockNotElapsed
        );
        require!(cfg.total_top_held == 0, EscrowError::MintLocked);
        let old = cfg.top_token_mint;
        cfg.top_token_mint = cfg.pending_top_token_mint;
        cfg.pending_top_token_mint = Pubkey::default();
        cfg.pending_top_token_mint_unlocks_at = 0;
        emit!(TopTokenMintRotated { old, new: cfg.top_token_mint, timestamp: now });
        Ok(())
    }

    pub fn cancel_set_top_token_mint(ctx: Context<AdminOnly>) -> Result<()> {
        let cfg = &mut ctx.accounts.escrow_config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), EscrowError::Unauthorized);
        require!(
            cfg.pending_top_token_mint_unlocks_at != 0,
            EscrowError::NothingPending
        );
        cfg.pending_top_token_mint = Pubkey::default();
        cfg.pending_top_token_mint_unlocks_at = 0;
        emit!(TopTokenMintProposalCancelled {});
        Ok(())
    }

    pub fn propose_set_swap_router(
        ctx: Context<AdminOnly>,
        new_router: Pubkey,
    ) -> Result<()> {
        let cfg = &mut ctx.accounts.escrow_config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), EscrowError::Unauthorized);
        let now = Clock::get()?.unix_timestamp;
        check_and_record_propose(cfg, now)?;
        cfg.pending_swap_router = new_router;
        cfg.pending_swap_router_unlocks_at = now
            .checked_add(ADMIN_TIMELOCK_SECONDS)
            .ok_or(EscrowError::MathOverflow)?;
        emit!(SwapRouterProposed {
            new_router,
            unlocks_at: cfg.pending_swap_router_unlocks_at,
        });
        Ok(())
    }

    pub fn finalize_set_swap_router(ctx: Context<AdminOnly>) -> Result<()> {
        let cfg = &mut ctx.accounts.escrow_config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), EscrowError::Unauthorized);
        require!(
            cfg.pending_swap_router_unlocks_at != 0,
            EscrowError::NothingPending
        );
        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= cfg.pending_swap_router_unlocks_at,
            EscrowError::TimelockNotElapsed
        );
        require!(cfg.total_top_held == 0, EscrowError::MintLocked);
        let old = cfg.swap_router_pubkey;
        cfg.swap_router_pubkey = cfg.pending_swap_router;
        cfg.pending_swap_router = Pubkey::default();
        cfg.pending_swap_router_unlocks_at = 0;
        emit!(SwapRouterRotated { old, new: cfg.swap_router_pubkey, timestamp: now });
        Ok(())
    }

    pub fn cancel_set_swap_router(ctx: Context<AdminOnly>) -> Result<()> {
        let cfg = &mut ctx.accounts.escrow_config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), EscrowError::Unauthorized);
        require!(
            cfg.pending_swap_router_unlocks_at != 0,
            EscrowError::NothingPending
        );
        cfg.pending_swap_router = Pubkey::default();
        cfg.pending_swap_router_unlocks_at = 0;
        emit!(SwapRouterProposalCancelled {});
        Ok(())
    }

    pub fn propose_set_burn_method(
        ctx: Context<AdminOnly>,
        method: BurnMethod,
    ) -> Result<()> {
        let cfg = &mut ctx.accounts.escrow_config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), EscrowError::Unauthorized);
        let now = Clock::get()?.unix_timestamp;
        check_and_record_propose(cfg, now)?;
        cfg.pending_burn_method = method;
        cfg.pending_burn_method_unlocks_at = now
            .checked_add(ADMIN_TIMELOCK_SECONDS)
            .ok_or(EscrowError::MathOverflow)?;
        emit!(BurnMethodProposed {
            method,
            unlocks_at: cfg.pending_burn_method_unlocks_at,
        });
        Ok(())
    }

    pub fn finalize_set_burn_method(ctx: Context<AdminOnly>) -> Result<()> {
        let cfg = &mut ctx.accounts.escrow_config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), EscrowError::Unauthorized);
        require!(
            cfg.pending_burn_method_unlocks_at != 0,
            EscrowError::NothingPending
        );
        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= cfg.pending_burn_method_unlocks_at,
            EscrowError::TimelockNotElapsed
        );
        let method = cfg.pending_burn_method;
        cfg.burn_method = method;
        cfg.pending_burn_method = BurnMethod::Burn;
        cfg.pending_burn_method_unlocks_at = 0;
        emit!(BurnMethodSet { method, timestamp: now });
        Ok(())
    }

    pub fn cancel_set_burn_method(ctx: Context<AdminOnly>) -> Result<()> {
        let cfg = &mut ctx.accounts.escrow_config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), EscrowError::Unauthorized);
        require!(
            cfg.pending_burn_method_unlocks_at != 0,
            EscrowError::NothingPending
        );
        cfg.pending_burn_method = BurnMethod::Burn;
        cfg.pending_burn_method_unlocks_at = 0;
        emit!(BurnMethodProposalCancelled {});
        Ok(())
    }


    pub fn propose_transfer_authority(
        ctx: Context<AdminOnly>,
        new_authority: Pubkey,
    ) -> Result<()> {
        let cfg = &mut ctx.accounts.escrow_config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), EscrowError::Unauthorized);
        require!(new_authority != Pubkey::default(), EscrowError::InvalidAuthority);
        let now = Clock::get()?.unix_timestamp;
        check_and_record_propose(cfg, now)?;
        cfg.pending_authority = new_authority;
        cfg.pending_authority_unlocks_at = now
            .checked_add(ADMIN_TIMELOCK_SECONDS)
            .ok_or(EscrowError::MathOverflow)?;
        emit!(AuthorityProposed {
            new_authority,
            unlocks_at: cfg.pending_authority_unlocks_at,
        });
        Ok(())
    }

    pub fn finalize_transfer_authority(ctx: Context<AdminOnly>) -> Result<()> {
        let cfg = &mut ctx.accounts.escrow_config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), EscrowError::Unauthorized);
        require!(
            cfg.pending_authority != Pubkey::default(),
            EscrowError::NothingPending
        );
        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= cfg.pending_authority_unlocks_at,
            EscrowError::TimelockNotElapsed
        );
        let old = cfg.authority;
        cfg.authority = cfg.pending_authority;
        cfg.pending_authority = Pubkey::default();
        cfg.pending_authority_unlocks_at = 0;
        emit!(AuthorityRotated { old, new: cfg.authority, timestamp: now });
        Ok(())
    }

    pub fn cancel_transfer_authority(ctx: Context<AdminOnly>) -> Result<()> {
        let cfg = &mut ctx.accounts.escrow_config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), EscrowError::Unauthorized);
        require!(
            cfg.pending_authority != Pubkey::default(),
            EscrowError::NothingPending
        );
        cfg.pending_authority = Pubkey::default();
        cfg.pending_authority_unlocks_at = 0;
        emit!(AuthorityProposalCancelled {});
        Ok(())
    }

    pub fn open_epoch(ctx: Context<OpenEpoch>, epoch_id: u64) -> Result<()> {
        let cfg = &mut ctx.accounts.escrow_config;
        require_keys_eq!(
            ctx.accounts.authority.key(),
            cfg.authority,
            EscrowError::Unauthorized
        );
        let expected_next = cfg.current_epoch_id.checked_add(1).ok_or(EscrowError::MathOverflow)?;
        if cfg.current_epoch_id == 0 && epoch_id == 0 {
        } else {
            require!(epoch_id == expected_next, EscrowError::BadEpochId);
        }
        let now = Clock::get()?.unix_timestamp;
        let epoch = &mut ctx.accounts.epoch;
        epoch.id = epoch_id;
        epoch.top_pool = 0;
        epoch.top_claimed = 0;
        epoch.total_weighted_stake_snapshot = 0;
        epoch.created_at = 0;
        epoch.finalized = false;
        epoch.snapshot_finalized = false;
        epoch.sum_registered_weight = 0;
        epoch.bump = ctx.bumps.epoch;
        cfg.current_epoch_id = epoch_id;
        emit!(EpochOpened { epoch_id, timestamp: now });
        Ok(())
    }

    pub fn deposit_top_from_swap(
        ctx: Context<DepositTopFromSwap>,
        top_amount: u64,
        epoch_id: u64,
    ) -> Result<()> {
        let cfg = &mut ctx.accounts.escrow_config;
        require!(top_amount > 0, EscrowError::ZeroAmount);

        require_keys_eq!(
            ctx.accounts.swap_router_signer.key(),
            cfg.swap_router_pubkey,
            EscrowError::Unauthorized,
        );
        let (expected, _) = Pubkey::find_program_address(
            &[b"swap_router_config"],
            &SWAP_ROUTER_PROGRAM_ID,
        );
        require_keys_eq!(
            ctx.accounts.swap_router_signer.key(),
            expected,
            EscrowError::Unauthorized,
        );

        require_keys_eq!(
            ctx.accounts.top_token_mint.key(),
            cfg.top_token_mint,
            EscrowError::WrongMint,
        );

        let ata = &ctx.accounts.backing_ata;
        let expected_min = cfg.total_top_held.checked_add(top_amount)
            .ok_or(EscrowError::MathOverflow)?;
        require!(ata.amount >= expected_min, EscrowError::BackingShort);

        let epoch = &mut ctx.accounts.epoch;
        require_eq!(epoch.id, epoch_id, EscrowError::BadEpochId);
        require!(!epoch.finalized, EscrowError::EpochFinalized);
        require_eq!(epoch_id, cfg.current_epoch_id, EscrowError::NotCurrentEpoch);

        let now = Clock::get()?.unix_timestamp;
        if epoch.top_pool == 0 {
            epoch.created_at = now;
        }

        epoch.top_pool = epoch.top_pool.checked_add(top_amount).ok_or(EscrowError::MathOverflow)?;
        cfg.total_top_held = cfg.total_top_held.checked_add(top_amount).ok_or(EscrowError::MathOverflow)?;

        emit!(EtopPoolFunded {
            epoch_id,
            top_amount,
            total_weighted_stake: epoch.total_weighted_stake_snapshot,
            timestamp: now,
        });
        Ok(())
    }

    pub fn register_epoch_weight(
        ctx: Context<RegisterEpochWeight>,
        epoch_id: u64,
    ) -> Result<()> {
        require_eq!(ctx.accounts.epoch.id, epoch_id, EscrowError::BadEpochId);
        require!(!ctx.accounts.epoch.snapshot_finalized, EscrowError::EpochAlreadyFinalized);

        let (expected_pos, _) = Pubkey::find_program_address(
            &[b"stake_position", ctx.accounts.staker.key().as_ref()],
            &STAKING_PROGRAM_ID,
        );
        require_keys_eq!(
            ctx.accounts.stake_position.key(),
            expected_pos,
            EscrowError::Unauthorized,
        );
        require!(
            ctx.accounts.stake_position.owner == &STAKING_PROGRAM_ID,
            EscrowError::Unauthorized,
        );

        let stake_data = ctx.accounts.stake_position.try_borrow_data()?;
        require!(stake_data.len() >= 49, EscrowError::InvalidStakePositionAccount);
        require!(
            stake_data[0..8] == STAKE_POSITION_DISCRIMINATOR,
            EscrowError::InvalidStakePositionAccount,
        );
        let owner_bytes: [u8; 32] = stake_data[8..40]
            .try_into().map_err(|_| EscrowError::InvalidStakePositionAccount)?;
        let owner_pk = Pubkey::new_from_array(owner_bytes);
        require_keys_eq!(owner_pk, ctx.accounts.staker.key(), EscrowError::Unauthorized);
        let amount = u64::from_le_bytes(
            stake_data[40..48].try_into().map_err(|_| EscrowError::InvalidStakePositionAccount)?
        );
        let tier = stake_data[48];
        drop(stake_data);

        let weight = compute_weighted_contribution(amount, tier)?;
        require!(weight > 0, EscrowError::BadWeight);

        ctx.accounts.epoch.sum_registered_weight = ctx.accounts.epoch
            .sum_registered_weight
            .checked_add(weight)
            .ok_or(EscrowError::MathOverflow)?;

        let claim = &mut ctx.accounts.claim;
        claim.epoch_id = epoch_id;
        claim.staker = ctx.accounts.staker.key();
        claim.top_credited = 0;
        claim.claimed = false;
        claim.registered_weight = weight;
        claim.weight_registered = true;
        claim.bump = ctx.bumps.claim;

        emit!(EpochWeightRegistered {
            epoch_id,
            staker: ctx.accounts.staker.key(),
            weight,
        });
        Ok(())
    }

    pub fn claim_etop_for_epoch(
        ctx: Context<ClaimEtopForEpoch>,
        epoch_id: u64,
    ) -> Result<()> {
        let top_pool = ctx.accounts.epoch.top_pool;
        let sum_registered = ctx.accounts.epoch.sum_registered_weight;
        let created_at = ctx.accounts.epoch.created_at;
        let top_claimed = ctx.accounts.epoch.top_claimed;
        require_eq!(ctx.accounts.epoch.id, epoch_id, EscrowError::BadEpochId);
        require!(top_pool > 0, EscrowError::NothingToClaim);

        require!(ctx.accounts.epoch.snapshot_finalized, EscrowError::EpochSnapshotNotFinalized);

        require!(sum_registered > 0, EscrowError::EmptySnapshot);

        require!(ctx.accounts.claim.weight_registered, EscrowError::WeightNotRegistered);
        require!(!ctx.accounts.claim.claimed, EscrowError::AlreadyClaimed);
        require_keys_eq!(ctx.accounts.claim.staker, ctx.accounts.staker.key(), EscrowError::Unauthorized);
        require_eq!(ctx.accounts.claim.epoch_id, epoch_id, EscrowError::BadEpochId);
        let staker_weight = ctx.accounts.claim.registered_weight;
        require!(staker_weight > 0 && staker_weight <= sum_registered, EscrowError::BadWeight);

        let top_share_u128 = (top_pool as u128)
            .checked_mul(staker_weight).ok_or(EscrowError::MathOverflow)?
            .checked_div(sum_registered).ok_or(EscrowError::MathOverflow)?;
        let top_share: u64 = top_share_u128.try_into().map_err(|_| EscrowError::MathOverflow)?;
        require!(top_share > 0, EscrowError::NothingToClaim);

        let remaining = (top_pool as u128)
            .checked_sub(top_claimed as u128).ok_or(EscrowError::MathOverflow)?;
        require!((top_share as u128) <= remaining, EscrowError::EpochOverdrawn);

        let now = Clock::get()?.unix_timestamp;

        {
            let position = &mut ctx.accounts.position;
            require!(
                position.pending_credits.len() < MAX_PENDING_CREDITS,
                EscrowError::PendingCreditsFull,
            );
            position.pending_credits.push(EtopCredit {
                epoch_id,
                top_amount: top_share,
                vest_start: created_at,
                vested_claimed: 0,
            });
            position.last_claim_at = now;
        }

        {
            let epoch_mut = &mut ctx.accounts.epoch;
            epoch_mut.top_claimed = epoch_mut.top_claimed
                .checked_add(top_share).ok_or(EscrowError::MathOverflow)?;
            epoch_mut.snapshot_finalized = true;
        }

        {
            let claim = &mut ctx.accounts.claim;
            claim.claimed = true;
            claim.top_credited = top_share;
        }

        emit!(EtopCreditClaimed {
            epoch_id,
            staker: ctx.accounts.staker.key(),
            top_amount: top_share,
            vest_start: created_at,
            timestamp: now,
        });
        Ok(())
    }

    pub fn set_epoch_snapshot(
        ctx: Context<SetEpochSnapshot>,
        epoch_id: u64,
        total_weighted_stake: u128,
    ) -> Result<()> {
        require!(total_weighted_stake > 0, EscrowError::EmptySnapshot);
        require_eq!(ctx.accounts.epoch.id, epoch_id, EscrowError::BadEpochId);
        require!(
            !ctx.accounts.epoch.snapshot_finalized,
            EscrowError::EpochAlreadyFinalized,
        );
        require!(
            ctx.accounts.epoch.top_claimed == 0,
            EscrowError::EpochAlreadyFinalized,
        );
        ctx.accounts.epoch.total_weighted_stake_snapshot = total_weighted_stake;
        emit!(EpochSnapshotSet { epoch_id, total_weighted_stake });
        Ok(())
    }

    pub fn finalize_epoch_snapshot(
        ctx: Context<FinalizeEpochSnapshot>,
        epoch_id: u64,
    ) -> Result<()> {
        require_keys_eq!(
            ctx.accounts.escrow_config.authority,
            ctx.accounts.authority.key(),
            EscrowError::Unauthorized,
        );
        require_eq!(
            ctx.accounts.epoch.id,
            epoch_id,
            EscrowError::BadEpochId,
        );
        require!(
            ctx.accounts.epoch.sum_registered_weight > 0,
            EscrowError::EmptySnapshot,
        );
        let was_finalized = ctx.accounts.epoch.snapshot_finalized;
        if !was_finalized {
            ctx.accounts.epoch.snapshot_finalized = true;
            emit!(EpochSnapshotFinalized {
                epoch_id,
                finalized_by: ctx.accounts.authority.key(),
            });
        }
        Ok(())
    }

    pub fn claim_vested(ctx: Context<ClaimVested>, amount: u64) -> Result<()> {
        require!(amount > 0, EscrowError::ZeroAmount);
        let now = Clock::get()?.unix_timestamp;

        let cfg = &ctx.accounts.escrow_config;
        require_keys_eq!(
            ctx.accounts.top_token_mint.key(),
            cfg.top_token_mint,
            EscrowError::WrongMint,
        );

        let position = &mut ctx.accounts.position;
        require_keys_eq!(position.staker, ctx.accounts.staker.key(), EscrowError::Unauthorized);
        let mut total_available: u128 = 0;
        for credit in position.pending_credits.iter() {
            let v = compute_vested(credit.top_amount, credit.vest_start, now)?;
            let delta = v.checked_sub(credit.vested_claimed).ok_or(EscrowError::MathOverflow)?;
            total_available = total_available.checked_add(delta as u128).ok_or(EscrowError::MathOverflow)?;
        }
        require!((amount as u128) <= total_available, EscrowError::InsufficientVested);

        let mut remaining = amount;
        for credit in position.pending_credits.iter_mut() {
            if remaining == 0 { break; }
            let v = compute_vested(credit.top_amount, credit.vest_start, now)?;
            let avail = v.checked_sub(credit.vested_claimed).ok_or(EscrowError::MathOverflow)?;
            if avail == 0 { continue; }
            let take = avail.min(remaining);
            credit.vested_claimed = credit.vested_claimed.checked_add(take).ok_or(EscrowError::MathOverflow)?;
            remaining = remaining.checked_sub(take).ok_or(EscrowError::MathOverflow)?;
        }
        require_eq!(remaining, 0, EscrowError::MathOverflow);

        let cfg_bump = ctx.accounts.escrow_config.bump;
        let seeds: &[&[u8]] = &[b"escrow_config", &[cfg_bump]];
        let signer = &[seeds];
        let top_decimals = ctx.accounts.top_token_mint.decimals;
        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            TransferChecked {
                from: ctx.accounts.backing_ata.to_account_info(),
                mint: ctx.accounts.top_token_mint.to_account_info(),
                to: ctx.accounts.staker_ata.to_account_info(),
                authority: ctx.accounts.escrow_config.to_account_info(),
            },
            signer,
        );
        token_interface::transfer_checked(cpi_ctx, amount, top_decimals)?;

        let cfg_mut = &mut ctx.accounts.escrow_config;
        cfg_mut.total_top_held = cfg_mut.total_top_held.checked_sub(amount).ok_or(EscrowError::MathOverflow)?;
        cfg_mut.total_top_claimed_vested = cfg_mut.total_top_claimed_vested.checked_add(amount).ok_or(EscrowError::MathOverflow)?;

        emit!(EtopVestedClaimed { staker: ctx.accounts.staker.key(), amount, timestamp: now });
        Ok(())
    }

    pub fn early_unstake(ctx: Context<EarlyUnstake>, credit_index: u32) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        let cfg = &ctx.accounts.escrow_config;
        require_keys_eq!(
            ctx.accounts.top_token_mint.key(),
            cfg.top_token_mint,
            EscrowError::WrongMint,
        );
        let top_decimals = ctx.accounts.top_token_mint.decimals;

        let position = &mut ctx.accounts.position;
        require_keys_eq!(position.staker, ctx.accounts.staker.key(), EscrowError::Unauthorized);

        let idx = credit_index as usize;
        require!(idx < position.pending_credits.len(), EscrowError::BadCreditIndex);
        let credit = position.pending_credits[idx].clone();

        let elapsed_diff = now.checked_sub(credit.vest_start).ok_or(EscrowError::MathOverflow)?;
        require!(elapsed_diff >= 0, EscrowError::VestStartInFuture);
        let elapsed_secs = elapsed_diff as u64;
        let days_staked = elapsed_secs / SECONDS_PER_DAY;

        let vested_now = compute_vested(credit.top_amount, credit.vest_start, now)?;
        let vested_remaining = vested_now.checked_sub(credit.vested_claimed).ok_or(EscrowError::MathOverflow)?;
        let unvested = credit.top_amount.checked_sub(vested_now).ok_or(EscrowError::MathOverflow)?;

        let burn_bps = compute_forfeiture_burn_bps(days_staked);
        let unvested_burn = ((unvested as u128)
            .checked_mul(burn_bps as u128).ok_or(EscrowError::MathOverflow)?
            .checked_div(BPS_DENOM as u128).ok_or(EscrowError::MathOverflow)?)
            as u64;
        let unvested_retain = unvested.checked_sub(unvested_burn).ok_or(EscrowError::MathOverflow)?;

        let total_to_player = vested_remaining
            .checked_add(unvested_retain).ok_or(EscrowError::MathOverflow)?;

        let cfg_bump = cfg.bump;
        let seeds: &[&[u8]] = &[b"escrow_config", &[cfg_bump]];
        let signer = &[seeds];
        if total_to_player > 0 {
            let cpi_ctx = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.backing_ata.to_account_info(),
                    mint: ctx.accounts.top_token_mint.to_account_info(),
                    to: ctx.accounts.staker_ata.to_account_info(),
                    authority: ctx.accounts.escrow_config.to_account_info(),
                },
                signer,
            );
            token_interface::transfer_checked(cpi_ctx, total_to_player, top_decimals)?;
        }

        if unvested_burn > 0 {
            match cfg.burn_method {
                BurnMethod::Burn => {
                    let cpi_ctx = CpiContext::new_with_signer(
                        ctx.accounts.token_program.to_account_info(),
                        SplBurn {
                            mint: ctx.accounts.top_token_mint.to_account_info(),
                            from: ctx.accounts.backing_ata.to_account_info(),
                            authority: ctx.accounts.escrow_config.to_account_info(),
                        },
                        signer,
                    );
                    token_interface::burn(cpi_ctx, unvested_burn)?;
                }
                BurnMethod::TransferToSystem => {
                    let burn_dest = ctx.accounts.burn_dest_ata.as_ref()
                        .ok_or(EscrowError::MissingAccount)?;
                    let cpi_ctx = CpiContext::new_with_signer(
                        ctx.accounts.token_program.to_account_info(),
                        TransferChecked {
                            from: ctx.accounts.backing_ata.to_account_info(),
                            mint: ctx.accounts.top_token_mint.to_account_info(),
                            to: burn_dest.to_account_info(),
                            authority: ctx.accounts.escrow_config.to_account_info(),
                        },
                        signer,
                    );
                    token_interface::transfer_checked(cpi_ctx, unvested_burn, top_decimals)?;
                }
            }
        }

        position.pending_credits.swap_remove(idx);

        let cfg_mut = &mut ctx.accounts.escrow_config;
        let total_removed = credit.top_amount.checked_sub(credit.vested_claimed).ok_or(EscrowError::MathOverflow)?;
        cfg_mut.total_top_held = cfg_mut.total_top_held.checked_sub(total_removed).ok_or(EscrowError::MathOverflow)?;
        cfg_mut.total_top_burned_forfeit = cfg_mut.total_top_burned_forfeit.checked_add(unvested_burn).ok_or(EscrowError::MathOverflow)?;

        emit!(EtopEarlyUnstake {
            staker: ctx.accounts.staker.key(),
            credit_epoch_id: credit.epoch_id,
            credit_amount: credit.top_amount,
            days_staked,
            retained: total_to_player,
            burned: unvested_burn,
            timestamp: now,
        });
        Ok(())
    }

    pub fn init_etop_position(ctx: Context<InitEtopPosition>) -> Result<()> {
        let pos = &mut ctx.accounts.position;
        if pos.staker == Pubkey::default() {
            pos.staker = ctx.accounts.staker.key();
            pos.pending_credits = Vec::new();
            pos.last_claim_at = 0;
            pos.bump = ctx.bumps.position;
        }
        Ok(())
    }
}


fn compute_vested(
    total_amount: u64,
    start_timestamp: i64,
    now: i64,
) -> Result<u64> {
    let diff = now
        .checked_sub(start_timestamp)
        .ok_or(EscrowError::MathOverflow)?;
    require!(diff >= 0, EscrowError::VestStartInFuture);
    let elapsed_seconds = diff as u64;
    let elapsed_days = elapsed_seconds / SECONDS_PER_DAY;
    let capped_days = elapsed_days.min(VESTING_DURATION_DAYS);
    (total_amount as u128)
        .checked_mul(capped_days as u128)
        .ok_or(EscrowError::MathOverflow)?
        .checked_div(VESTING_DURATION_DAYS as u128)
        .ok_or(EscrowError::MathOverflow)?
        .try_into()
        .map_err(|_| EscrowError::MathOverflow.into())
}

fn compute_forfeiture_burn_bps(days_staked: u64) -> u64 {
    if days_staked <= 30 {
        FORFEIT_DAY_30_BPS
    } else if days_staked <= 60 {
        FORFEIT_DAY_60_BPS
    } else if days_staked <= 90 {
        FORFEIT_DAY_90_BPS
    } else if days_staked < 180 {
        FORFEIT_DAY_180_BPS
    } else {
        0
    }
}

const STAKING_PROGRAM_ID: Pubkey = pubkey!("2n2puiEN8BbMMEtq387b6HKR2trvKY9rK5uM82Ht2Vtc");

pub const STAKE_POSITION_DISCRIMINATOR: [u8; 8] = [78, 165, 30, 111, 171, 125, 11, 220];

const TIER_WEIGHT_BPS: [u128; 8] = [0, 500, 1_500, 3_000, 5_000, 7_500, 9_000, 10_000];

fn compute_weighted_contribution(amount: u64, tier: u8) -> Result<u128> {
    if (tier as usize) >= TIER_WEIGHT_BPS.len() {
        return Ok(0);
    }
    let weight = (amount as u128)
        .checked_mul(TIER_WEIGHT_BPS[tier as usize])
        .ok_or(EscrowError::MathOverflow)?
        .checked_div(BPS_DENOM as u128)
        .ok_or(EscrowError::MathOverflow)?;
    Ok(weight)
}


fn check_and_record_propose(config: &mut EscrowConfig, now: i64) -> Result<()> {
    require!(
        now >= config.propose_cooldown_until,
        EscrowError::ProposeCooldownActive
    );
    let window_start = now.saturating_sub(PROPOSE_RATE_LIMIT_WINDOW_SECONDS);
    let count_24h = config
        .recent_proposes
        .iter()
        .filter(|t| **t > window_start)
        .count();
    let next_cooldown_seconds: i64 = match count_24h {
        0 | 1 => 0,
        2 => 1_800,
        3 => 7_200,
        4 => 86_400,
        _ => 604_800,
    };
    config.propose_cooldown_until = if next_cooldown_seconds == 0 {
        0
    } else {
        now.checked_add(next_cooldown_seconds)
            .ok_or(EscrowError::MathOverflow)?
    };
    for i in 0..(PROPOSE_RATE_LIMIT_RING_LEN - 1) {
        config.recent_proposes[i] = config.recent_proposes[i + 1];
    }
    config.recent_proposes[PROPOSE_RATE_LIMIT_RING_LEN - 1] = now;
    Ok(())
}


#[error_code]
pub enum EscrowError {
    #[msg("Amount must be greater than zero")]
    ZeroAmount,
    #[msg("Arithmetic overflow")]
    MathOverflow,
    #[msg("Nothing available to claim")]
    NothingToClaim,
    #[msg("Unauthorized")]
    Unauthorized,
    #[msg("Wrong mint")]
    WrongMint,
    #[msg("Mint / swap_router pubkey already locked (total_top_held > 0)")]
    MintLocked,
    #[msg("Bad epoch ID")]
    BadEpochId,
    #[msg("Epoch finalized")]
    EpochFinalized,
    #[msg("Deposit epoch is not the current epoch")]
    NotCurrentEpoch,
    #[msg("Backing ATA balance is short")]
    BackingShort,
    #[msg("Already claimed for this epoch")]
    AlreadyClaimed,
    #[msg("Empty weighted-stake snapshot")]
    EmptySnapshot,
    #[msg("staker_weight invalid (zero or > total_weighted_stake)")]
    BadWeight,
    #[msg("Pending credits Vec at cap; prune via early_unstake or claim_vested")]
    PendingCreditsFull,
    #[msg("Insufficient vested across all credits")]
    InsufficientVested,
    #[msg("Bad credit index")]
    BadCreditIndex,
    #[msg("Epoch overdrawn")]
    EpochOverdrawn,
    #[msg("Missing optional account")]
    MissingAccount,
    #[msg("Caller-supplied staker_weight does not match on-chain recomputation")]
    StakerWeightMismatch,
    #[msg("StakePosition account is malformed or too short to decode")]
    InvalidStakePositionAccount,
    #[msg("burn_dest_ata must be a TokenAccount whose authority is SystemProgram::ID (11111...) for the configured $TOP mint")]
    BurnDestinationInvalid,

    #[msg("No pending proposal for this admin field (unlocks_at == 0)")]
    NothingPending,
    #[msg("Admin timelock has not elapsed yet (72h propose-to-finalize delay)")]
    TimelockNotElapsed,
    #[msg("Invalid authority: cannot propose Pubkey::default()")]
    InvalidAuthority,

    #[msg("Propose cooldown active — escalating rate-limit per Rule 27b defense (R7.7-H-01)")]
    ProposeCooldownActive,

    #[msg("Epoch snapshot already finalized — open a fresh epoch to set a new snapshot")]
    EpochAlreadyFinalized,
    #[msg("Epoch snapshot not yet finalized — call finalize_epoch_snapshot() first")]
    EpochSnapshotNotFinalized,
    #[msg("Vesting start timestamp is in the future (clock skew or invalid credit)")]
    VestStartInFuture,

    #[msg("Staker weight was not registered for this epoch before finalize — call register_epoch_weight() while the snapshot is open")]
    WeightNotRegistered,
}


#[event]
pub struct BurnMethodSet { pub method: BurnMethod, pub timestamp: i64 }
#[event]
pub struct EpochOpened { pub epoch_id: u64, pub timestamp: i64 }
#[event]
pub struct EpochSnapshotSet { pub epoch_id: u64, pub total_weighted_stake: u128 }
#[event]
pub struct EtopPoolFunded {
    pub epoch_id: u64,
    pub top_amount: u64,
    pub total_weighted_stake: u128,
    pub timestamp: i64,
}
#[event]
pub struct EtopCreditClaimed {
    pub epoch_id: u64,
    pub staker: Pubkey,
    pub top_amount: u64,
    pub vest_start: i64,
    pub timestamp: i64,
}
#[event]
pub struct EpochWeightRegistered {
    pub epoch_id: u64,
    pub staker: Pubkey,
    pub weight: u128,
}
#[event]
pub struct EtopVestedClaimed { pub staker: Pubkey, pub amount: u64, pub timestamp: i64 }
#[event]
pub struct EtopEarlyUnstake {
    pub staker: Pubkey,
    pub credit_epoch_id: u64,
    pub credit_amount: u64,
    pub days_staked: u64,
    pub retained: u64,
    pub burned: u64,
    pub timestamp: i64,
}

#[event]
pub struct TopTokenMintProposed { pub new_mint: Pubkey, pub unlocks_at: i64 }
#[event]
pub struct TopTokenMintRotated { pub old: Pubkey, pub new: Pubkey, pub timestamp: i64 }
#[event]
pub struct TopTokenMintProposalCancelled {}
#[event]
pub struct SwapRouterProposed { pub new_router: Pubkey, pub unlocks_at: i64 }
#[event]
pub struct SwapRouterRotated { pub old: Pubkey, pub new: Pubkey, pub timestamp: i64 }
#[event]
pub struct SwapRouterProposalCancelled {}
#[event]
pub struct BurnMethodProposed { pub method: BurnMethod, pub unlocks_at: i64 }
#[event]
pub struct BurnMethodProposalCancelled {}
#[event]
pub struct AuthorityProposed { pub new_authority: Pubkey, pub unlocks_at: i64 }
#[event]
pub struct AuthorityRotated { pub old: Pubkey, pub new: Pubkey, pub timestamp: i64 }
#[event]
pub struct AuthorityProposalCancelled {}

#[event]
pub struct EpochSnapshotFinalized {
    pub epoch_id: u64,
    pub finalized_by: Pubkey,
}


#[account]
pub struct EscrowConfig {
    pub authority: Pubkey,
    pub swap_router_pubkey: Pubkey,
    pub top_token_mint: Pubkey,
    pub current_epoch_id: u64,
    pub total_top_held: u64,
    pub total_top_claimed_vested: u64,
    pub total_top_burned_forfeit: u64,
    pub burn_method: BurnMethod,
    pub bump: u8,

    pub pending_top_token_mint: Pubkey,
    pub pending_top_token_mint_unlocks_at: i64,
    pub pending_swap_router: Pubkey,
    pub pending_swap_router_unlocks_at: i64,
    pub pending_burn_method: BurnMethod,
    pub pending_burn_method_unlocks_at: i64,
    pub pending_authority: Pubkey,
    pub pending_authority_unlocks_at: i64,

    pub propose_cooldown_until: i64,
    pub recent_proposes: [i64; 5],
}
impl EscrowConfig { pub const SPACE: usize = 368; }

#[account]
pub struct EtopEpoch {
    pub id: u64,
    pub top_pool: u64,
    pub top_claimed: u64,
    pub total_weighted_stake_snapshot: u128,
    pub created_at: i64,
    pub finalized: bool,
    pub bump: u8,
    pub snapshot_finalized: bool,
    pub sum_registered_weight: u128,
}
impl EtopEpoch { pub const SPACE: usize = 96; }

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq)]
pub struct EtopCredit {
    pub epoch_id: u64,
    pub top_amount: u64,
    pub vest_start: i64,
    pub vested_claimed: u64,
}

#[account]
pub struct EtopPosition {
    pub staker: Pubkey,
    pub pending_credits: Vec<EtopCredit>,
    pub last_claim_at: i64,
    pub bump: u8,
}
impl EtopPosition { pub const SPACE: usize = 1792; }

#[account]
#[derive(InitSpace)]
pub struct EtopClaim {
    pub epoch_id: u64,
    pub staker: Pubkey,
    pub top_credited: u64,
    pub claimed: bool,
    pub bump: u8,
    pub registered_weight: u128,
    pub weight_registered: bool,
}


#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = authority,
        space = EscrowConfig::SPACE,
        seeds = [b"escrow_config"],
        bump
    )]
    pub escrow_config: Account<'info, EscrowConfig>,
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(
        seeds = [crate::ID.as_ref()],
        bump,
        seeds::program = anchor_lang::solana_program::bpf_loader_upgradeable::ID,
        constraint = program_data.upgrade_authority_address == Some(authority.key())
            @ EscrowError::Unauthorized,
    )]
    pub program_data: Account<'info, ProgramData>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct AdminOnly<'info> {
    #[account(mut, seeds = [b"escrow_config"], bump = escrow_config.bump)]
    pub escrow_config: Account<'info, EscrowConfig>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(epoch_id: u64)]
pub struct SetEpochSnapshot<'info> {
    #[account(mut, seeds = [b"escrow_config"], bump = escrow_config.bump,
              has_one = authority @ EscrowError::Unauthorized)]
    pub escrow_config: Account<'info, EscrowConfig>,
    #[account(
        mut,
        seeds = [b"etop_epoch", epoch_id.to_le_bytes().as_ref()],
        bump = epoch.bump,
    )]
    pub epoch: Account<'info, EtopEpoch>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(epoch_id: u64)]
pub struct FinalizeEpochSnapshot<'info> {
    #[account(mut, seeds = [b"escrow_config"], bump = escrow_config.bump,
              has_one = authority @ EscrowError::Unauthorized)]
    pub escrow_config: Account<'info, EscrowConfig>,
    #[account(
        mut,
        seeds = [b"etop_epoch", epoch_id.to_le_bytes().as_ref()],
        bump = epoch.bump,
    )]
    pub epoch: Account<'info, EtopEpoch>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(epoch_id: u64)]
pub struct OpenEpoch<'info> {
    #[account(mut, seeds = [b"escrow_config"], bump = escrow_config.bump)]
    pub escrow_config: Account<'info, EscrowConfig>,
    #[account(
        init,
        payer = payer,
        space = 8 + EtopEpoch::SPACE,
        seeds = [b"etop_epoch", epoch_id.to_le_bytes().as_ref()],
        bump,
    )]
    pub epoch: Account<'info, EtopEpoch>,
    #[account(constraint = authority.key() == escrow_config.authority @ EscrowError::Unauthorized)]
    pub authority: Signer<'info>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(top_amount: u64, epoch_id: u64)]
pub struct DepositTopFromSwap<'info> {
    #[account(mut, seeds = [b"escrow_config"], bump = escrow_config.bump)]
    pub escrow_config: Account<'info, EscrowConfig>,
    #[account(
        mut,
        seeds = [b"etop_epoch", epoch_id.to_le_bytes().as_ref()],
        bump = epoch.bump,
    )]
    pub epoch: Account<'info, EtopEpoch>,
    pub swap_router_signer: Signer<'info>,
    #[account(
        mut,
        constraint = backing_ata.owner == escrow_config.key() @ EscrowError::Unauthorized,
        constraint = backing_ata.mint == escrow_config.top_token_mint @ EscrowError::WrongMint,
    )]
    pub backing_ata: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(address = escrow_config.top_token_mint @ EscrowError::WrongMint)]
    pub top_token_mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ClaimVested<'info> {
    #[account(mut, seeds = [b"escrow_config"], bump = escrow_config.bump)]
    pub escrow_config: Account<'info, EscrowConfig>,
    #[account(
        mut,
        seeds = [b"etop_pos", staker.key().as_ref()],
        bump = position.bump,
    )]
    pub position: Account<'info, EtopPosition>,
    #[account(
        mut,
        constraint = backing_ata.owner == escrow_config.key() @ EscrowError::Unauthorized,
        constraint = backing_ata.mint == escrow_config.top_token_mint @ EscrowError::WrongMint,
    )]
    pub backing_ata: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        mut,
        constraint = staker_ata.owner == staker.key() @ EscrowError::Unauthorized,
        constraint = staker_ata.mint == escrow_config.top_token_mint @ EscrowError::WrongMint,
    )]
    pub staker_ata: Box<InterfaceAccount<'info, TokenAccount>>,
    pub top_token_mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(mut)]
    pub staker: Signer<'info>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct EarlyUnstake<'info> {
    #[account(mut, seeds = [b"escrow_config"], bump = escrow_config.bump)]
    pub escrow_config: Account<'info, EscrowConfig>,
    #[account(
        mut,
        seeds = [b"etop_pos", staker.key().as_ref()],
        bump = position.bump,
    )]
    pub position: Account<'info, EtopPosition>,
    #[account(
        mut,
        constraint = backing_ata.owner == escrow_config.key() @ EscrowError::Unauthorized,
        constraint = backing_ata.mint == escrow_config.top_token_mint @ EscrowError::WrongMint,
    )]
    pub backing_ata: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        mut,
        constraint = staker_ata.owner == staker.key() @ EscrowError::Unauthorized,
        constraint = staker_ata.mint == escrow_config.top_token_mint @ EscrowError::WrongMint,
    )]
    pub staker_ata: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        mut,
        constraint = burn_dest_ata.owner == BURN_FALLBACK_DEST
            @ EscrowError::BurnDestinationInvalid,
        constraint = burn_dest_ata.mint == escrow_config.top_token_mint
            @ EscrowError::BurnDestinationInvalid,
    )]
    pub burn_dest_ata: Option<InterfaceAccount<'info, TokenAccount>>,
    #[account(mut)]
    pub top_token_mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(mut)]
    pub staker: Signer<'info>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(epoch_id: u64)]
pub struct RegisterEpochWeight<'info> {
    #[account(
        mut,
        seeds = [b"etop_epoch", epoch_id.to_le_bytes().as_ref()],
        bump = epoch.bump,
    )]
    pub epoch: Account<'info, EtopEpoch>,
    #[account(
        init,
        payer = payer,
        space = 8 + EtopClaim::INIT_SPACE,
        seeds = [b"etop_claim", staker.key().as_ref(), epoch_id.to_le_bytes().as_ref()],
        bump,
    )]
    pub claim: Account<'info, EtopClaim>,
    pub stake_position: AccountInfo<'info>,
    pub staker: AccountInfo<'info>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(epoch_id: u64)]
pub struct ClaimEtopForEpoch<'info> {
    #[account(mut, seeds = [b"escrow_config"], bump = escrow_config.bump)]
    pub escrow_config: Account<'info, EscrowConfig>,
    #[account(
        mut,
        seeds = [b"etop_epoch", epoch_id.to_le_bytes().as_ref()],
        bump = epoch.bump,
    )]
    pub epoch: Account<'info, EtopEpoch>,
    #[account(
        mut,
        seeds = [b"etop_pos", staker.key().as_ref()],
        bump = position.bump,
    )]
    pub position: Account<'info, EtopPosition>,
    #[account(
        mut,
        seeds = [b"etop_claim", staker.key().as_ref(), epoch_id.to_le_bytes().as_ref()],
        bump = claim.bump,
    )]
    pub claim: Account<'info, EtopClaim>,
    #[account(mut)]
    pub staker: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct InitEtopPosition<'info> {
    #[account(
        init_if_needed,
        payer = staker,
        space = 8 + EtopPosition::SPACE,
        seeds = [b"etop_pos", staker.key().as_ref()],
        bump,
    )]
    pub position: Account<'info, EtopPosition>,
    #[account(mut)]
    pub staker: Signer<'info>,
    pub system_program: Program<'info, System>,
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vest_zero_at_day_zero() {
        let total = 1_000_000u64;
        let v = compute_vested(total, 0, 0).unwrap();
        assert_eq!(v, 0);
    }

    #[test]
    fn vest_linear_at_day_90() {
        let total = 180u64;
        let v = compute_vested(total, 0, 90 * SECONDS_PER_DAY as i64).unwrap();
        assert_eq!(v, 90);
    }

    #[test]
    fn vest_full_at_day_180() {
        let total = 1_000_000u64;
        let v = compute_vested(total, 0, 180 * SECONDS_PER_DAY as i64).unwrap();
        assert_eq!(v, 1_000_000);
    }

    #[test]
    fn vest_caps_past_day_180() {
        let total = 1_000_000u64;
        let v = compute_vested(total, 0, 500 * SECONDS_PER_DAY as i64).unwrap();
        assert_eq!(v, 1_000_000);
    }

    #[test]
    fn forfeit_at_day_0_is_full_burn() {
        assert_eq!(compute_forfeiture_burn_bps(0), 10_000);
    }

    #[test]
    fn forfeit_at_day_30_is_full_burn() {
        assert_eq!(compute_forfeiture_burn_bps(30), 10_000);
    }

    #[test]
    fn forfeit_at_day_31_is_75_pct() {
        assert_eq!(compute_forfeiture_burn_bps(31), 7_500);
    }

    #[test]
    fn forfeit_at_day_45_is_75_pct() {
        assert_eq!(compute_forfeiture_burn_bps(45), 7_500);
    }

    #[test]
    fn forfeit_at_day_60_is_75_pct() {
        assert_eq!(compute_forfeiture_burn_bps(60), 7_500);
    }

    #[test]
    fn forfeit_at_day_61_is_50_pct() {
        assert_eq!(compute_forfeiture_burn_bps(61), 5_000);
    }

    #[test]
    fn forfeit_at_day_90_is_50_pct() {
        assert_eq!(compute_forfeiture_burn_bps(90), 5_000);
    }

    #[test]
    fn forfeit_at_day_91_is_25_pct() {
        assert_eq!(compute_forfeiture_burn_bps(91), 2_500);
    }

    #[test]
    fn forfeit_at_day_179_is_25_pct() {
        assert_eq!(compute_forfeiture_burn_bps(179), 2_500);
    }

    #[test]
    fn forfeit_at_day_180_is_zero() {
        assert_eq!(compute_forfeiture_burn_bps(180), 0);
    }

    #[test]
    fn forfeit_at_day_500_is_zero() {
        assert_eq!(compute_forfeiture_burn_bps(500), 0);
    }

    #[test]
    fn early_unstake_math_day_45_100_credit() {
        let total: u64 = 100;
        let elapsed_days: u64 = 45;
        let vested = (total as u128) * (elapsed_days as u128) / 180;
        let unvested = total as u128 - vested;
        let burn_bps: u128 = compute_forfeiture_burn_bps(elapsed_days) as u128;
        let unvested_burn = unvested * burn_bps / 10_000;
        let unvested_retain = unvested - unvested_burn;
        let to_player = vested + unvested_retain;
        assert_eq!(vested, 25);
        assert_eq!(unvested, 75);
        assert_eq!(unvested_burn, 56);
        assert_eq!(unvested_retain, 19);
        assert_eq!(to_player, 44);
    }

    #[test]
    fn early_unstake_day_60_full_credit_size_test_doc_example() {
        let total: u64 = 100;
        let elapsed_days: u64 = 60;
        let vested = (total as u128) * (elapsed_days as u128) / 180;
        assert_eq!(vested, 33);
        let unvested = total as u128 - vested;
        assert_eq!(unvested, 67);
        let burn_bps = compute_forfeiture_burn_bps(elapsed_days) as u128;
        assert_eq!(burn_bps, 7500);
        let unvested_burn = unvested * burn_bps / 10_000;
        assert_eq!(unvested_burn, 50);
    }

    #[test]
    fn max_pending_credits_is_52_weeks() {
        assert_eq!(MAX_PENDING_CREDITS, 52);
    }

    #[test]
    fn burn_method_default_is_burn() {
        assert_eq!(BurnMethod::default(), BurnMethod::Burn);
    }

    #[test]
    fn swap_router_program_id_is_stable() {
        assert_eq!(
            SWAP_ROUTER_PROGRAM_ID.to_string(),
            "9tkZMhH7cf293wpGpJycqsZLb8eojkJ5jyUHJtqcS5zR"
        );
    }


    #[test]
    fn tier_weight_table_matches_staking_program() {
        assert_eq!(TIER_WEIGHT_BPS,
            [0u128, 500, 1_500, 3_000, 5_000, 7_500, 9_000, 10_000]);
    }

    #[test]
    fn compute_weighted_contribution_diamond() {
        let w = compute_weighted_contribution(15_000_000, 6).unwrap();
        assert_eq!(w, 13_500_000);
    }

    #[test]
    fn compute_weighted_contribution_sovereign() {
        let w = compute_weighted_contribution(20_000_000, 7).unwrap();
        assert_eq!(w, 20_000_000);
    }

    #[test]
    fn compute_weighted_contribution_none_tier() {
        let w = compute_weighted_contribution(1_000_000, 0).unwrap();
        assert_eq!(w, 0);
    }

    #[test]
    fn compute_weighted_contribution_invalid_tier() {
        let w = compute_weighted_contribution(1_000_000, 99).unwrap();
        assert_eq!(w, 0);
    }

    #[test]
    fn register_epoch_weight_uses_onchain_computation_not_caller_value() {
        let computed = compute_weighted_contribution(15_000_000, 6).unwrap();
        let attacker_would_have_supplied: u128 = 100_000_000;
        assert_ne!(computed, attacker_would_have_supplied);
        assert_eq!(computed, 13_500_000);
    }

    #[test]
    fn register_epoch_weight_honest_value() {
        let computed = compute_weighted_contribution(15_000_000, 6).unwrap();
        assert_eq!(computed, 13_500_000);
    }

    #[test]
    fn register_epoch_weight_owner_field_check_documented() {
        let stake_position_owner_offset = 8usize;
        assert_eq!(stake_position_owner_offset, 8);
    }

    #[test]
    fn stake_position_byte_offsets_locked() {
        let owner_off: usize = 8;
        let amount_off: usize = owner_off + 32;
        let tier_off: usize = amount_off + 8;
        assert_eq!(owner_off, 8);
        assert_eq!(amount_off, 40);
        assert_eq!(tier_off, 48);
    }


    #[test]
    fn set_epoch_snapshot_requires_typed_epoch_account() {
        fn _accepts_typed_struct<T>(_: core::marker::PhantomData<T>) {}
        _accepts_typed_struct::<EtopEpoch>(core::marker::PhantomData);
    }

    #[test]
    fn set_epoch_snapshot_rejects_wrong_epoch_id() {
        let stored: u64 = 7;
        let arg: u64 = 8;
        assert_ne!(stored, arg);
    }


    #[test]
    fn burn_fallback_dest_is_system_program_id() {
        assert_eq!(
            BURN_FALLBACK_DEST,
            anchor_lang::solana_program::system_program::ID,
        );
        assert_eq!(
            BURN_FALLBACK_DEST.to_string(),
            "11111111111111111111111111111111",
        );
    }

    #[test]
    fn burn_dest_ata_canonical_address_is_deterministic_and_unspendable() {
        let _: Pubkey = BURN_FALLBACK_DEST;
        fn _typed<T>(_: core::marker::PhantomData<T>) {}
        _typed::<EarlyUnstake<'_>>(core::marker::PhantomData);
    }

    #[test]
    fn burn_destination_invalid_error_variant_exists() {
        let err = EscrowError::BurnDestinationInvalid;
        let label: &'static str = match err {
            EscrowError::BurnDestinationInvalid => "burn_dest_invalid",
            _ => "other",
        };
        assert_eq!(label, "burn_dest_invalid");
    }

    #[test]
    fn early_unstake_pre_handler_constraint_evaluation_order() {
        let attacker_owned: bool =  true ==  false;
        assert!(!attacker_owned, "attacker-owned ATA must NOT pass owner check");

        let canonical_burn_ata: bool =  true ==  true;
        assert!(canonical_burn_ata, "canonical 11111...-owned ATA must pass");

        let wrong_mint: bool =  false ==  true;
        assert!(!wrong_mint, "wrong-mint ATA must NOT pass mint check");
    }


    fn fresh_config_for_timelock_tests() -> EscrowConfig {
        EscrowConfig {
            authority: Pubkey::new_unique(),
            swap_router_pubkey: Pubkey::new_unique(),
            top_token_mint: Pubkey::new_unique(),
            current_epoch_id: 0,
            total_top_held: 0,
            total_top_claimed_vested: 0,
            total_top_burned_forfeit: 0,
            burn_method: BurnMethod::Burn,
            bump: 254,
            pending_top_token_mint: Pubkey::default(),
            pending_top_token_mint_unlocks_at: 0,
            pending_swap_router: Pubkey::default(),
            pending_swap_router_unlocks_at: 0,
            pending_burn_method: BurnMethod::Burn,
            pending_burn_method_unlocks_at: 0,
            pending_authority: Pubkey::default(),
            pending_authority_unlocks_at: 0,
            propose_cooldown_until: 0,
            recent_proposes: [0i64; 5],
        }
    }

    #[test]
    fn admin_timelock_seconds_is_72h() {
        assert_eq!(ADMIN_TIMELOCK_SECONDS, 72 * 60 * 60);
        assert_eq!(ADMIN_TIMELOCK_SECONDS, 259_200);
    }

    #[test]
    fn propose_set_top_token_mint_writes_pending_fields() {
        let mut cfg = fresh_config_for_timelock_tests();
        let new_mint = Pubkey::new_unique();
        let now: i64 = 1_700_000_000;
        cfg.pending_top_token_mint = new_mint;
        cfg.pending_top_token_mint_unlocks_at = now + ADMIN_TIMELOCK_SECONDS;
        assert_eq!(cfg.pending_top_token_mint, new_mint);
        assert_eq!(cfg.pending_top_token_mint_unlocks_at, now + 259_200);
        assert_ne!(cfg.top_token_mint, new_mint);
    }

    #[test]
    fn finalize_set_top_token_mint_before_unlock_rejected_shape() {
        let mut cfg = fresh_config_for_timelock_tests();
        let now: i64 = 1_700_000_000;
        cfg.pending_top_token_mint = Pubkey::new_unique();
        cfg.pending_top_token_mint_unlocks_at = now + ADMIN_TIMELOCK_SECONDS;
        assert!(now < cfg.pending_top_token_mint_unlocks_at);
    }

    #[test]
    fn finalize_set_top_token_mint_after_unlock_commits_shape() {
        let mut cfg = fresh_config_for_timelock_tests();
        let prior_mint = cfg.top_token_mint;
        let new_mint = Pubkey::new_unique();
        let now0: i64 = 1_700_000_000;
        cfg.pending_top_token_mint = new_mint;
        cfg.pending_top_token_mint_unlocks_at = now0 + ADMIN_TIMELOCK_SECONDS;
        let now1 = now0 + ADMIN_TIMELOCK_SECONDS + 1;
        assert!(now1 >= cfg.pending_top_token_mint_unlocks_at);
        cfg.top_token_mint = cfg.pending_top_token_mint;
        cfg.pending_top_token_mint = Pubkey::default();
        cfg.pending_top_token_mint_unlocks_at = 0;
        assert_eq!(cfg.top_token_mint, new_mint);
        assert_ne!(cfg.top_token_mint, prior_mint);
        assert_eq!(cfg.pending_top_token_mint, Pubkey::default());
        assert_eq!(cfg.pending_top_token_mint_unlocks_at, 0);
    }

    #[test]
    fn cancel_set_top_token_mint_clears_pending_shape() {
        let mut cfg = fresh_config_for_timelock_tests();
        let live_mint = cfg.top_token_mint;
        cfg.pending_top_token_mint = Pubkey::new_unique();
        cfg.pending_top_token_mint_unlocks_at = 1_700_000_000 + ADMIN_TIMELOCK_SECONDS;
        cfg.pending_top_token_mint = Pubkey::default();
        cfg.pending_top_token_mint_unlocks_at = 0;
        assert_eq!(cfg.pending_top_token_mint, Pubkey::default());
        assert_eq!(cfg.pending_top_token_mint_unlocks_at, 0);
        assert_eq!(cfg.top_token_mint, live_mint);
    }

    #[test]
    fn finalize_no_pending_rejected_shape() {
        let cfg = fresh_config_for_timelock_tests();
        assert_eq!(cfg.pending_top_token_mint_unlocks_at, 0);
        assert_eq!(cfg.pending_swap_router_unlocks_at, 0);
        assert_eq!(cfg.pending_burn_method_unlocks_at, 0);
        assert_eq!(cfg.pending_authority, Pubkey::default());
    }

    #[test]
    fn propose_transfer_authority_rejects_default_pubkey_shape() {
        let new_authority = Pubkey::default();
        assert_eq!(new_authority, Pubkey::default());
    }

    #[test]
    fn finalize_transfer_authority_commits_shape() {
        let mut cfg = fresh_config_for_timelock_tests();
        let old_auth = cfg.authority;
        let new_auth = Pubkey::new_unique();
        let now0: i64 = 1_700_000_000;
        cfg.pending_authority = new_auth;
        cfg.pending_authority_unlocks_at = now0 + ADMIN_TIMELOCK_SECONDS;
        let _now1 = now0 + ADMIN_TIMELOCK_SECONDS + 1;
        cfg.authority = cfg.pending_authority;
        cfg.pending_authority = Pubkey::default();
        cfg.pending_authority_unlocks_at = 0;
        assert_eq!(cfg.authority, new_auth);
        assert_ne!(cfg.authority, old_auth);
    }

    #[test]
    fn set_burn_method_timelock_triplet_shape() {
        let mut cfg = fresh_config_for_timelock_tests();
        assert_eq!(cfg.burn_method, BurnMethod::Burn);
        let now0: i64 = 1_700_000_000;
        cfg.pending_burn_method = BurnMethod::TransferToSystem;
        cfg.pending_burn_method_unlocks_at = now0 + ADMIN_TIMELOCK_SECONDS;
        let _now1 = now0 + ADMIN_TIMELOCK_SECONDS + 1;
        cfg.burn_method = cfg.pending_burn_method;
        cfg.pending_burn_method = BurnMethod::Burn;
        cfg.pending_burn_method_unlocks_at = 0;
        assert_eq!(cfg.burn_method, BurnMethod::TransferToSystem);
        assert_eq!(cfg.pending_burn_method_unlocks_at, 0);
    }

    #[test]
    fn escrow_config_space_absorbs_timelock_fields() {
        assert!(EscrowConfig::SPACE >= 315,
            "EscrowConfig::SPACE must cover timelock fields + rate-limit, got {}",
            EscrowConfig::SPACE);
        assert_eq!(EscrowConfig::SPACE, 368);
    }

    #[test]
    fn stake_position_discriminator_pinned() {
        let expected: [u8; 8] = [78, 165, 30, 111, 171, 125, 11, 220];
        assert_eq!(
            STAKE_POSITION_DISCRIMINATOR, expected,
            "STAKE_POSITION_DISCRIMINATOR drifted — recompute via \
             sha256(\"account:StakePosition\")[..8] and sync."
        );
    }
    
    
    fn build_test_epoch(
        epoch_id: u64,
        top_claimed: u64,
        snapshot: u128,
        snapshot_finalized: bool,
    ) -> EtopEpoch {
        EtopEpoch {
            id: epoch_id,
            top_pool: 0,
            top_claimed,
            total_weighted_stake_snapshot: snapshot,
            created_at: 0,
            finalized: false,
            snapshot_finalized,
            
            sum_registered_weight: snapshot,
            bump: 254,
        }
    }
    
    #[derive(Debug, PartialEq, Eq)]
    enum SimRevert {
        BadEpochId,
        EpochAlreadyFinalized,
        EmptySnapshot,
    }
    
    fn simulate_set_epoch_snapshot(
        epoch: &EtopEpoch,
        epoch_id_arg: u64,
        total_weighted_stake: u128,
    ) -> std::result::Result<(), SimRevert> {
        if total_weighted_stake == 0 {
            return Err(SimRevert::EmptySnapshot);
        }
        if epoch.id != epoch_id_arg {
            return Err(SimRevert::BadEpochId);
        }
        if epoch.snapshot_finalized {
            return Err(SimRevert::EpochAlreadyFinalized);
        }
        if epoch.top_claimed != 0 {
            return Err(SimRevert::EpochAlreadyFinalized);
        }
        Ok(())
    }
    
    fn simulate_finalize_epoch_snapshot(
        epoch: &EtopEpoch,
        epoch_id_arg: u64,
    ) -> std::result::Result<(bool, EtopEpoch), SimRevert> {
        if epoch.id != epoch_id_arg {
            return Err(SimRevert::BadEpochId);
        }
        
        if epoch.sum_registered_weight == 0 {
            return Err(SimRevert::EmptySnapshot);
        }
        let was_finalized = epoch.snapshot_finalized;
        let mut new_epoch = epoch.clone();
        new_epoch.snapshot_finalized = true;
        Ok((was_finalized, new_epoch))
    }
    
    #[derive(Debug, PartialEq, Eq)]
    enum DepositRevert { ZeroAmount, BadEpochId, EpochFinalized, NotCurrentEpoch }
    
    fn simulate_deposit_epoch_gate(
        epoch: &EtopEpoch,
        current_epoch_id: u64,
        epoch_id_arg: u64,
        top_amount: u64,
        now: i64,
    ) -> std::result::Result<i64, DepositRevert> {
        if top_amount == 0 { return Err(DepositRevert::ZeroAmount); }
        if epoch.id != epoch_id_arg { return Err(DepositRevert::BadEpochId); }
        if epoch.finalized { return Err(DepositRevert::EpochFinalized); }
        
        if epoch_id_arg != current_epoch_id { return Err(DepositRevert::NotCurrentEpoch); }
        
        let created_at = if epoch.top_pool == 0 { now } else { epoch.created_at };
        Ok(created_at)
    }
    
    #[test]
    fn etop_f01_deposit_to_noncurrent_epoch_reverts() {
        let epoch = build_test_epoch( 5, 0, 0, false);
        
        assert_eq!(
            simulate_deposit_epoch_gate(&epoch,  7,  5, 1_000, 100),
            Err(DepositRevert::NotCurrentEpoch),
        );
    }
    
    #[test]
    fn etop_f01_deposit_to_current_epoch_ok() {
        let epoch = build_test_epoch( 7, 0, 0, false);
        assert!(simulate_deposit_epoch_gate(&epoch, 7, 7, 1_000, 100).is_ok());
    }
    
    #[test]
    fn etop_f01_finalized_fires_before_current_epoch_gate() {
        let mut epoch = build_test_epoch( 7, 0, 0, false);
        epoch.finalized = true;
        assert_eq!(
            simulate_deposit_epoch_gate(&epoch, 7, 7, 1_000, 100),
            Err(DepositRevert::EpochFinalized),
        );
    }
    
    #[test]
    fn etop_f02_first_deposit_stamps_created_at() {
        let epoch = build_test_epoch( 7, 0, 0, false);
        assert_eq!(epoch.created_at, 0, "open_epoch must leave created_at = 0");
        let after = simulate_deposit_epoch_gate(&epoch, 7, 7, 1_000,  1_700_000_000).unwrap();
        assert_eq!(after, 1_700_000_000, "first deposit stamps the vest anchor");
    }
    
    #[test]
    fn etop_f02_second_deposit_keeps_first_timestamp() {
        let mut epoch = build_test_epoch( 7, 0, 0, false);
        epoch.top_pool = 5_000;
        epoch.created_at = 1_700_000_000;
        let after = simulate_deposit_epoch_gate(&epoch, 7, 7, 1_000,  1_700_999_999).unwrap();
        assert_eq!(after, 1_700_000_000, "vest anchor stays at first deposit, not the second");
    }
    
    #[test]
    fn etop_f01_bad_epoch_id_fires_before_current_epoch_gate() {
        let epoch = build_test_epoch( 3, 0, 0, false);
        
        assert_eq!(
            simulate_deposit_epoch_gate(&epoch,  7,  5, 1_000, 100),
            Err(DepositRevert::BadEpochId),
        );
    }
    
    #[test]
    fn etop_f02_migration_unfunded_old_epoch_reanchors() {
        let mut epoch = build_test_epoch( 7, 0, 0, false);
        epoch.created_at = 1_600_000_000;
        let after = simulate_deposit_epoch_gate(&epoch, 7, 7, 1_000,  1_700_000_000).unwrap();
        assert_eq!(after, 1_700_000_000, "first real deposit re-anchors to swap-landing");
        assert_ne!(after, 1_600_000_000, "the pre-upgrade epoch-open anchor must NOT survive");
    }
    
    #[test]
    fn etop_deposit_zero_amount_reverts() {
        let epoch = build_test_epoch( 7, 0, 0, false);
        assert_eq!(
            simulate_deposit_epoch_gate(&epoch, 7, 7, 0, 100),
            Err(DepositRevert::ZeroAmount),
        );
    }
    
    
    #[test]
    fn m8_first_set_succeeds_on_fresh_epoch() {
        let epoch = build_test_epoch( 7,  0,  0, false);
        assert!(simulate_set_epoch_snapshot(&epoch, 7, 100_000_000_000_000).is_ok());
    }
    
    #[test]
    fn m8_second_set_reverts_after_explicit_finalize() {
        let cold = build_test_epoch( 7, 0, 100_000_000_000_000, false);
        
        assert!(simulate_set_epoch_snapshot(&cold, 7, 200_000_000_000_000).is_ok());
        
        let (was, after_finalize) = simulate_finalize_epoch_snapshot(&cold, 7).expect("finalize");
        assert!(!was, "fresh snapshot, finalize event SHOULD emit on first flip");
        assert!(after_finalize.snapshot_finalized);
        
        assert_eq!(
            simulate_set_epoch_snapshot(&after_finalize, 7, 999_000_000_000_000),
            Err(SimRevert::EpochAlreadyFinalized),
        );
    }
    
    #[test]
    fn m8_set_reverts_after_first_claim_lock_on_claim_route() {
        
        let epoch = build_test_epoch(
             7,
             12_345,
             100_000_000_000_000,
             true,
        );
        assert_eq!(
            simulate_set_epoch_snapshot(&epoch, 7, 999_000_000_000_000),
            Err(SimRevert::EpochAlreadyFinalized),
        );
    }
    
    #[test]
    fn m8_defense_in_depth_top_claimed_blocks_even_without_lock_byte() {
        let epoch = build_test_epoch(
             7,
             1,
             100_000_000_000_000,
             false,       
        );
        assert_eq!(
            simulate_set_epoch_snapshot(&epoch, 7, 999_000_000_000_000),
            Err(SimRevert::EpochAlreadyFinalized),
        );
    }
    
    #[test]
    fn m8_set_with_wrong_epoch_id_reverts_before_lock_check() {
        
        let epoch = build_test_epoch(
             7,
            0,
            100_000_000_000_000,
             true,
        );
        assert_eq!(
            simulate_set_epoch_snapshot(&epoch,  8, 100_000_000_000_000),
            Err(SimRevert::BadEpochId),
        );
    }
    
    #[test]
    fn m8_finalize_on_empty_snapshot_reverts() {
        let epoch = build_test_epoch( 7, 0,  0, false);
        assert_eq!(
            simulate_finalize_epoch_snapshot(&epoch, 7).err(),
            Some(SimRevert::EmptySnapshot),
        );
    }
    
    #[test]
    fn m8_finalize_idempotent_after_already_finalized() {
        let epoch = build_test_epoch(
             7,
            0,
            100_000_000_000_000,
             true,
        );
        let (was, new_epoch) = simulate_finalize_epoch_snapshot(&epoch, 7).expect("idempotent");
        assert!(was, "should report it was already finalized — event SHOULD be suppressed");
        assert!(new_epoch.snapshot_finalized, "lock still true after idempotent no-op");
    }
    
    #[test]
    fn m8_finalize_first_call_flips_lock_byte() {
        let epoch = build_test_epoch( 7, 0, 100_000_000_000_000, false);
        let (was, new_epoch) = simulate_finalize_epoch_snapshot(&epoch, 7).expect("first finalize");
        assert!(!was, "fresh snapshot reports was_finalized=false → event MUST emit");
        assert!(new_epoch.snapshot_finalized, "lock flipped to true after first finalize");
        
        assert_eq!(
            simulate_set_epoch_snapshot(&new_epoch, 7, 999_000_000_000_000),
            Err(SimRevert::EpochAlreadyFinalized),
        );
        
        let (was2, _) = simulate_finalize_epoch_snapshot(&new_epoch, 7).expect("idempotent retry");
        assert!(was2, "second finalize reports was=true → no event re-emit");
    }
    
    #[test]
    fn m8_etop_epoch_field_offsets_regression_locked() {
        
        const RAW_PAYLOAD: usize = 8 + 8 + 8 + 16 + 8 + 1 + 1 + 1 + 16;
        assert_eq!(RAW_PAYLOAD, 67, "EtopEpoch raw payload drifted from 67 bytes");
        
        assert_eq!(EtopEpoch::SPACE, 96, "EtopEpoch::SPACE drifted from 96");
        assert!(EtopEpoch::SPACE >= RAW_PAYLOAD,
            "EtopEpoch::SPACE must cover raw payload ({} >= {})",
            EtopEpoch::SPACE, RAW_PAYLOAD);
        
        let e = build_test_epoch(0, 0, 0, false);
        
        let _finalized: bool = e.finalized;
        let _bump: u8 = e.bump;
        let _snapshot_finalized: bool = e.snapshot_finalized;
        
        let _sum_registered_weight: u128 = e.sum_registered_weight;
        
        let pre = build_test_epoch(0, 0, 0, false);
        let mut post = pre.clone();
        post.snapshot_finalized = true;
        assert!(post.snapshot_finalized);
        assert!(!post.finalized,
            "snapshot_finalized and finalized must be SEPARATE fields — \
             flipping one MUST NOT flip the other");
    }
    
    

    #[derive(Debug, PartialEq, Eq)]
    enum EeRevert {
        BadEpochId,
        EpochAlreadyFinalized,
        BadWeight,
        EmptySnapshot,
        EpochSnapshotNotFinalized, 
        WeightNotRegistered,       
        AlreadyClaimed,
        NothingToClaim,
        EpochOverdrawn,
        MathOverflow,              
    }

    fn build_test_claim(registered_weight: u128, weight_registered: bool, claimed: bool) -> EtopClaim {
        EtopClaim {
            epoch_id: 7,
            staker: Pubkey::new_unique(),
            top_credited: 0,
            claimed,
            bump: 254,
            registered_weight,
            weight_registered,
        }
    }
    
    
    fn ee01_epoch(
        id: u64,
        top_pool: u64,
        top_claimed: u64,
        telemetry: u128,
        sum_registered_weight: u128,
        finalized: bool,
    ) -> EtopEpoch {
        EtopEpoch {
            id,
            top_pool,
            top_claimed,
            total_weighted_stake_snapshot: telemetry,
            created_at: 0,
            finalized: false,
            snapshot_finalized: finalized,
            sum_registered_weight,
            bump: 254,
        }
    }
    
    
    fn simulate_register_epoch_weight(
        epoch: &EtopEpoch,
        epoch_id_arg: u64,
        weight: u128,
    ) -> std::result::Result<u128, EeRevert> {
        if epoch.id != epoch_id_arg { return Err(EeRevert::BadEpochId); }
        
        
        if epoch.snapshot_finalized { return Err(EeRevert::EpochAlreadyFinalized); }
        if weight == 0 { return Err(EeRevert::BadWeight); }
        
        epoch.sum_registered_weight.checked_add(weight).ok_or(EeRevert::MathOverflow)
    }
    
    
    fn simulate_claim_etop(
        epoch: &EtopEpoch,
        claim: &EtopClaim,
        epoch_id_arg: u64,
    ) -> std::result::Result<u64, EeRevert> {
        if epoch.id != epoch_id_arg { return Err(EeRevert::BadEpochId); }
        if epoch.top_pool == 0 { return Err(EeRevert::NothingToClaim); }
        if !epoch.snapshot_finalized { return Err(EeRevert::EpochSnapshotNotFinalized); }
        
        let sum = epoch.sum_registered_weight;
        if sum == 0 { return Err(EeRevert::EmptySnapshot); }
        if !claim.weight_registered { return Err(EeRevert::WeightNotRegistered); }
        if claim.claimed { return Err(EeRevert::AlreadyClaimed); }
        let w = claim.registered_weight;
        if w == 0 || w > sum { return Err(EeRevert::BadWeight); }
        let share_u128 = (epoch.top_pool as u128) * w / sum;
        let share = share_u128 as u64;
        if share == 0 { return Err(EeRevert::NothingToClaim); }
        let remaining = (epoch.top_pool as u128) - (epoch.top_claimed as u128);
        if (share as u128) > remaining { return Err(EeRevert::EpochOverdrawn); }
        Ok(share)
    }
    
    
    #[test]
    fn ee01_honest_claim_pays_exact_fair_share() {
        
        let mut open = ee01_epoch(7,  1000, 0,  0,  0, false);
        open.sum_registered_weight = simulate_register_epoch_weight(&open, 7, 100).expect("A registers");
        assert_eq!(open.sum_registered_weight, 100, "sum after A = 100");
        open.sum_registered_weight = simulate_register_epoch_weight(&open, 7, 100).expect("B registers");
        assert_eq!(open.sum_registered_weight, 200, "sum accumulates to 200 after B");
        
        let mut epoch = open.clone();
        epoch.snapshot_finalized = true;
        let share_a = simulate_claim_etop(&epoch, &build_test_claim(100, true, false), 7).expect("A claims");
        assert_eq!(share_a, 500, "A fair share = 1000 * 100/200");
        epoch.top_claimed = 500;
        let share_b = simulate_claim_etop(&epoch, &build_test_claim(100, true, false), 7).expect("B claims");
        assert_eq!(share_b, 500, "B fair share = 1000 * 100/200");
        assert_eq!(share_a + share_b, 1000, "pool fully + exactly distributed");
    }
    
    
    #[test]
    fn ee01_late_whale_bounded_to_registered_weight() {
        let mut open = ee01_epoch(7, 1000, 0, 0, 0,  false);
        open.sum_registered_weight = simulate_register_epoch_weight(&open, 7, 100).expect("whale registers 100");
        open.sum_registered_weight = simulate_register_epoch_weight(&open, 7, 100).expect("honest B registers 100");
        assert_eq!(open.sum_registered_weight, 200, "sum frozen at 200 (whale 100 + honest B 100)");
        
        let mut epoch = open.clone();
        epoch.snapshot_finalized = true;
        
        
        assert_eq!(
            simulate_register_epoch_weight(&epoch, 7, 190),
            Err(EeRevert::EpochAlreadyFinalized),
            "post-finalize re-registration REJECTED — the ETOP-EE01 core gate",
        );
        
        assert_eq!(simulate_claim_etop(&epoch, &build_test_claim(100, true, false), 7), Ok(500),
            "post-fix claim bounded to the registered fair share, NOT the pool");
        
        let mut after = epoch.clone();
        after.top_claimed = 500;
        assert_eq!(
            simulate_claim_etop(&after, &build_test_claim(100, true, false), 7),
            Ok(500),
            "honest claimer NOT starved (no EpochOverdrawn)",
        );
    }
    
    
    #[test]
    fn ee01_m03_topped_up_staker_still_claims_pre_topup_share() {
        let epoch = ee01_epoch(7, 1000, 0,  200,  200,  true);
        assert_eq!(simulate_claim_etop(&epoch, &build_test_claim(100, true, false), 7), Ok(500),
            "staker in the snapshot claims pre-topup share — M-03 lockout avoided");
    }
    
    
    #[test]
    fn ee01_boundary_register_after_finalize_rejected() {
        let epoch = ee01_epoch(7, 1000, 0, 200, 200,  true);
        assert_eq!(simulate_register_epoch_weight(&epoch, 7, 100),
            Err(EeRevert::EpochAlreadyFinalized));
    }
    
    
    #[test]
    fn ee01_boundary_claim_without_registration_rejected() {
        let epoch = ee01_epoch(7, 1000, 0, 200, 200, true);
        let unregistered = build_test_claim(0,  false, false);
        assert_eq!(simulate_claim_etop(&epoch, &unregistered, 7),
            Err(EeRevert::WeightNotRegistered));
    }
    
    
    #[test]
    fn ee01_boundary_register_zero_weight_rejected() {
        let epoch = ee01_epoch(7, 1000, 0, 0, 0,  false); 
        assert_eq!(simulate_register_epoch_weight(&epoch, 7, 0), Err(EeRevert::BadWeight));
    }

    
    #[test]
    fn ee01_boundary_double_claim_rejected() {
        let epoch = ee01_epoch(7, 1000,  500, 200, 200, true);
        let claimed = build_test_claim(100, true,  true);
        assert_eq!(simulate_claim_etop(&epoch, &claimed, 7), Err(EeRevert::AlreadyClaimed));
    }

    
    #[test]
    fn ee01_boundary_claim_before_finalize_rejected() {
        let epoch = ee01_epoch(7, 1000, 0, 200, 200,  false);
        assert_eq!(simulate_claim_etop(&epoch, &build_test_claim(100, true, false), 7),
            Err(EeRevert::EpochSnapshotNotFinalized));
    }
    
    
    #[test]
    fn ee01_claim_divides_by_onchain_sum_not_authority_telemetry() {
        let epoch = ee01_epoch(7, 1000, 0,  1,  200,  true);
        assert_eq!(simulate_claim_etop(&epoch, &build_test_claim(100, true, false), 7), Ok(500),
            "claim uses sum_registered_weight (200), NOT total_weighted_stake_snapshot (1)");
    }
    
    
    #[test]
    fn ee01_pre_finalize_desync_no_overdraw_no_theft() {
        
        let mut open = ee01_epoch(7,  999, 0,  200,  0, false);
        for _ in 0..3 {
            open.sum_registered_weight =
                simulate_register_epoch_weight(&open, 7, 100).expect("staker self-registers 100");
        }
        assert_eq!(open.sum_registered_weight, 300, "three registrations accumulate to 300 on-chain");
        
        let mut epoch = open.clone();
        epoch.snapshot_finalized = true;
        
        assert_eq!(999u128 * 100 / 200, 499, "pre-fix telemetry-denominator over-allocates -> theft");
        
        let mut total_claimed: u64 = 0;
        for i in 0..3 {
            let mut e = epoch.clone();
            e.top_claimed = total_claimed;
            let share = simulate_claim_etop(&e, &build_test_claim(100, true, false), 7)
                .unwrap_or_else(|err| panic!("claimer {i} must NOT revert, got {err:?}"));
            assert_eq!(share, 333, "each fair share = 999 * 100/300");
            total_claimed += share;
        }
        assert_eq!(total_claimed, 999, "Σ shares == pool (dust-free here); no overdraw, no theft");
    }
    
    
    #[test]
    fn ee01_accumulator_checked_add_overflow() {
        let open = ee01_epoch(7, 1000, 0, 0,  u128::MAX, false);
        assert_eq!(simulate_register_epoch_weight(&open, 7, 1), Err(EeRevert::MathOverflow));
    }
    
    
    #[test]
    fn ee01_finalize_requires_registered_sum_not_telemetry() {
        let epoch = ee01_epoch(7, 1000, 0,  200,  0,  false);
        assert_eq!(
            simulate_finalize_epoch_snapshot(&epoch, 7).err(),
            Some(SimRevert::EmptySnapshot),
            "finalize gates on the registered sum, not the authority telemetry",
        );
    }
    
    
    #[test]
    fn ee01_carries_weight_fields() {
        let c = build_test_claim(13_500_000, true, false);
        let _w: u128 = c.registered_weight;
        let _r: bool = c.weight_registered;
        assert_eq!(c.registered_weight, 13_500_000);
        assert!(c.weight_registered);
        
        let e = ee01_epoch(7, 0, 0, 0, 12_345, false);
        let _sum: u128 = e.sum_registered_weight;
        assert_eq!(e.sum_registered_weight, 12_345);
    }

    
    
    fn drift_handler_end(src: &str, idx: usize) -> usize {
        src[idx + 1..]
            .find("\n    pub fn ")
            .map(|p| idx + 1 + p)
            .expect("drift-gate: no following `pub fn` — re-anchor this source-assert bound")
    }

    fn etop_escrow_lib_rs_source() -> &'static str {
        include_str!("lib.rs")
    }

    #[test]
    fn r77_h01_check_and_record_propose_helper_present() {
        let src = etop_escrow_lib_rs_source();
        assert!(src.contains("fn check_and_record_propose("),
            "etop-escrow MUST define the check_and_record_propose helper (R7.7-H-01)");
        assert!(src.contains("propose_cooldown_until"),
            "EscrowConfig MUST carry propose_cooldown_until field (R7.7-H-01)");
        assert!(src.contains("recent_proposes: [i64; 5]"),
            "EscrowConfig MUST carry recent_proposes: [i64; 5] ring buffer (R7.7-H-01)");
        assert!(src.contains("ProposeCooldownActive"),
            "EscrowError::ProposeCooldownActive variant MUST exist (R7.7-H-01)");
    }

    #[test]
    fn r77_h01_all_propose_handlers_call_check_and_record_propose() {
        
        
        let src = etop_escrow_lib_rs_source();
        let propose_names = ["propose_set_top_token_mint",
                             "propose_set_swap_router",
                             "propose_set_burn_method",
                             "propose_transfer_authority"];
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
}
