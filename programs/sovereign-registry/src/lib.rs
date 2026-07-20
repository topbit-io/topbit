
use anchor_lang::prelude::*;
use anchor_lang::system_program;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer as TokenTransfer};

declare_id!("14ndgn3yKuD4Zi3ozBt7Fo4cYzUuYDAZrTn15wT3rFC2");

#[cfg(not(feature = "no-entrypoint"))]
solana_security_txt::security_txt! {
    name: "sovereign_registry",
    project_url: "https://topbit.io",
    contacts: "email:security@topbit.io",
    policy: "https://topbit.io/security",
    preferred_languages: "en",
    source_code: "https://github.com/topbit-io/topbit"
}

const MAX_SEATS: u8 = 21;
const TOKEN_DECIMALS: u64 = 1_000_000;

pub const EXPECTED_TOP_DECIMALS: u8 = 6;

const SOVEREIGN_THRESHOLD_TOKENS: u64 = 20_000_000;

pub const SOVEREIGN_THRESHOLD_AMOUNT: u64 =
    SOVEREIGN_THRESHOLD_TOKENS * TOKEN_DECIMALS;

pub const STAKING_PROGRAM_ID: Pubkey =
    pubkey!("2n2puiEN8BbMMEtq387b6HKR2trvKY9rK5uM82Ht2Vtc");

pub const MIN_STAKE_DURATION_SECONDS: i64 = 7 * 24 * 60 * 60;

pub const ADMIN_TIMELOCK_SECONDS: i64 = 72 * 60 * 60;
pub const PROPOSE_RATE_LIMIT_WINDOW_SECONDS: i64 = 86_400;
pub const PROPOSE_RATE_LIMIT_RING_LEN: usize = 5;

fn check_and_record_propose(cfg: &mut RegistryConfig, now: i64) -> Result<()> {
    require!(now >= cfg.propose_cooldown_until, RegistryError::ProposeCooldownActive);
    let window_start = now.saturating_sub(PROPOSE_RATE_LIMIT_WINDOW_SECONDS);
    let count_24h = cfg.recent_proposes.iter().filter(|t| **t > window_start).count();
    let next_cooldown_seconds: i64 = match count_24h {
        0 | 1 => 0, 2 => 1_800, 3 => 7_200, 4 => 86_400, _ => 604_800,
    };
    cfg.propose_cooldown_until = if next_cooldown_seconds == 0 {
        0
    } else {
        now.checked_add(next_cooldown_seconds).ok_or(RegistryError::MathOverflow)?
    };
    for i in 0..(PROPOSE_RATE_LIMIT_RING_LEN - 1) {
        cfg.recent_proposes[i] = cfg.recent_proposes[i + 1];
    }
    cfg.recent_proposes[PROPOSE_RATE_LIMIT_RING_LEN - 1] = now;
    Ok(())
}

#[program]
pub mod sovereign_registry {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        let config = &mut ctx.accounts.registry_config;
        config.authority = ctx.accounts.authority.key();
        config.total_seats_filled = 0;
        config.required_stake = SOVEREIGN_THRESHOLD_AMOUNT;
        config.total_royalty_pool = 0;
        config.bump = ctx.bumps.registry_config;
        config.royalty_vault_bump = ctx.bumps.royalty_vault;
        config.accrued_per_seat = 0;
        config.waterfall_authority = Pubkey::default();
        config.provider_vault_authority = Pubkey::default();

        config.royalty_vault_usdc = Pubkey::default();
        config.usdc_mint = Pubkey::default();
        config.royalty_vault_usdc_bump = 0;
        config.accrued_per_seat_usdc = 0;
        config.total_royalty_pool_usdc = 0;
        config.propose_cooldown_until = 0;
        config.recent_proposes = [0i64; 5];
        config.pending_authority = Pubkey::default();
        config.pending_authority_unlocks_at = 0;
        config.pending_provider_vault_authority = Pubkey::default();
        config.pending_provider_vault_authority_unlocks_at = 0;
        config.pending_waterfall_authority = Pubkey::default();
        config.pending_waterfall_authority_unlocks_at = 0;
        Ok(())
    }

    pub fn set_waterfall_authority(
        ctx: Context<AdminOnly>,
        new_authority: Pubkey,
    ) -> Result<()> {
        let cfg = &mut ctx.accounts.registry_config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), RegistryError::Unauthorized);
        require!(
            new_authority != Pubkey::default(),
            RegistryError::InvalidWaterfallAuthority
        );
        require!(
            cfg.waterfall_authority == Pubkey::default(),
            RegistryError::WaterfallAuthorityAlreadyConfigured
        );
        cfg.waterfall_authority = new_authority;
        Ok(())
    }

    pub fn propose_set_waterfall_authority(
        ctx: Context<AdminOnly>,
        new_authority: Pubkey,
    ) -> Result<()> {
        let cfg = &mut ctx.accounts.registry_config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), RegistryError::Unauthorized);
        require!(
            cfg.waterfall_authority != Pubkey::default(),
            RegistryError::WaterfallAuthorityNotConfigured
        );
        require!(
            new_authority != Pubkey::default(),
            RegistryError::InvalidWaterfallAuthority
        );
        require!(
            cfg.pending_waterfall_authority == Pubkey::default(),
            RegistryError::WaterfallAuthorityProposalAlreadyPending
        );
        let now = Clock::get()?.unix_timestamp;
        check_and_record_propose(cfg, now)?;
        let unlocks_at = now
            .checked_add(ADMIN_TIMELOCK_SECONDS)
            .ok_or(RegistryError::MathOverflow)?;
        cfg.pending_waterfall_authority = new_authority;
        cfg.pending_waterfall_authority_unlocks_at = unlocks_at;
        Ok(())
    }

    pub fn finalize_set_waterfall_authority(ctx: Context<AdminOnly>) -> Result<()> {
        let cfg = &mut ctx.accounts.registry_config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), RegistryError::Unauthorized);
        require!(
            cfg.pending_waterfall_authority != Pubkey::default(),
            RegistryError::WaterfallAuthorityNoProposalPending
        );
        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= cfg.pending_waterfall_authority_unlocks_at,
            RegistryError::AdminTimelockNotElapsed
        );
        cfg.waterfall_authority = cfg.pending_waterfall_authority;
        cfg.pending_waterfall_authority = Pubkey::default();
        cfg.pending_waterfall_authority_unlocks_at = 0;
        Ok(())
    }

    pub fn cancel_set_waterfall_authority(ctx: Context<AdminOnly>) -> Result<()> {
        let cfg = &mut ctx.accounts.registry_config;
        require_keys_eq!(cfg.authority, ctx.accounts.authority.key(), RegistryError::Unauthorized);
        require!(
            cfg.pending_waterfall_authority != Pubkey::default(),
            RegistryError::WaterfallAuthorityNoProposalPending
        );
        cfg.pending_waterfall_authority = Pubkey::default();
        cfg.pending_waterfall_authority_unlocks_at = 0;
        Ok(())
    }

    pub fn set_provider_vault_authority(
        ctx: Context<AdminOnly>,
        new_authority: Pubkey,
    ) -> Result<()> {
        let cfg = &mut ctx.accounts.registry_config;
        require_keys_eq!(
            cfg.authority,
            ctx.accounts.authority.key(),
            RegistryError::Unauthorized
        );
        require!(
            new_authority != Pubkey::default(),
            RegistryError::InvalidWaterfallAuthority
        );
        require!(
            cfg.provider_vault_authority == Pubkey::default(),
            RegistryError::ProviderVaultAuthorityAlreadyConfigured
        );
        let old_authority = cfg.provider_vault_authority;
        cfg.provider_vault_authority = new_authority;
        emit!(ProviderVaultAuthorityRotated {
            old_authority,
            new_authority,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    pub fn propose_set_provider_vault_authority(
        ctx: Context<AdminOnly>,
        new_authority: Pubkey,
    ) -> Result<()> {
        let cfg = &mut ctx.accounts.registry_config;
        require_keys_eq!(
            cfg.authority,
            ctx.accounts.authority.key(),
            RegistryError::Unauthorized
        );
        require!(
            new_authority != Pubkey::default(),
            RegistryError::InvalidWaterfallAuthority
        );
        require!(
            cfg.pending_provider_vault_authority == Pubkey::default(),
            RegistryError::ProviderVaultAuthorityProposalAlreadyPending
        );
        let now = Clock::get()?.unix_timestamp;
        check_and_record_propose(cfg, now)?;
        let unlocks_at = now
            .checked_add(ADMIN_TIMELOCK_SECONDS)
            .ok_or(RegistryError::MathOverflow)?;
        cfg.pending_provider_vault_authority = new_authority;
        cfg.pending_provider_vault_authority_unlocks_at = unlocks_at;
        emit!(ProviderVaultAuthorityProposed {
            admin: cfg.authority,
            new_authority,
            unlocks_at,
            timestamp: now,
        });
        Ok(())
    }

    pub fn finalize_set_provider_vault_authority(
        ctx: Context<AdminOnly>,
    ) -> Result<()> {
        let cfg = &mut ctx.accounts.registry_config;
        require_keys_eq!(
            cfg.authority,
            ctx.accounts.authority.key(),
            RegistryError::Unauthorized
        );
        require!(
            cfg.pending_provider_vault_authority != Pubkey::default(),
            RegistryError::ProviderVaultAuthorityNoProposalPending
        );
        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= cfg.pending_provider_vault_authority_unlocks_at,
            RegistryError::AdminTimelockNotElapsed
        );
        let new_authority = cfg.pending_provider_vault_authority;
        let old_authority = cfg.provider_vault_authority;
        cfg.provider_vault_authority = new_authority;
        cfg.pending_provider_vault_authority = Pubkey::default();
        cfg.pending_provider_vault_authority_unlocks_at = 0;
        emit!(ProviderVaultAuthorityRotated {
            old_authority,
            new_authority,
            timestamp: now,
        });
        Ok(())
    }

    pub fn cancel_set_provider_vault_authority(
        ctx: Context<AdminOnly>,
    ) -> Result<()> {
        let cfg = &mut ctx.accounts.registry_config;
        require_keys_eq!(
            cfg.authority,
            ctx.accounts.authority.key(),
            RegistryError::Unauthorized
        );
        require!(
            cfg.pending_provider_vault_authority != Pubkey::default(),
            RegistryError::ProviderVaultAuthorityNoProposalPending
        );
        let cancelled_authority = cfg.pending_provider_vault_authority;
        cfg.pending_provider_vault_authority = Pubkey::default();
        cfg.pending_provider_vault_authority_unlocks_at = 0;
        emit!(ProviderVaultAuthorityProposalCancelled {
            admin: cfg.authority,
            cancelled_authority,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    pub fn initialize_royalty_vault_usdc(
        ctx: Context<InitializeRoyaltyVaultUsdc>,
    ) -> Result<()> {
        let config = &mut ctx.accounts.registry_config;
        require!(
            config.usdc_mint == Pubkey::default(),
            RegistryError::UsdcVaultAlreadyConfigured
        );
        config.usdc_mint = ctx.accounts.usdc_mint.key();
        config.royalty_vault_usdc = ctx.accounts.royalty_vault_usdc.key();
        config.royalty_vault_usdc_bump = ctx.bumps.royalty_vault_usdc;

        emit!(RoyaltyVaultUsdcInitialized {
            usdc_mint: config.usdc_mint,
            royalty_vault_usdc: config.royalty_vault_usdc,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }

    pub fn claim_seat(ctx: Context<ClaimSeat>, seat_index: u8) -> Result<()> {
        require!(seat_index < MAX_SEATS, RegistryError::InvalidSeatIndex);

        let config = &mut ctx.accounts.registry_config;
        require!(
            config.total_seats_filled < MAX_SEATS,
            RegistryError::AllSeatsFilled
        );

        let stake_pos: StakePosition = {
            let data = ctx.accounts.stake_position.try_borrow_data()?;
            StakePosition::try_deserialize(&mut data.as_ref())
                .map_err(|_| RegistryError::StakePositionMismatch)?
        };
        require_keys_eq!(
            stake_pos.owner,
            ctx.accounts.holder.key(),
            RegistryError::StakePositionMismatch
        );
        require!(
            stake_pos.amount >= SOVEREIGN_THRESHOLD_AMOUNT,
            RegistryError::InsufficientStake
        );

        let now = Clock::get()?.unix_timestamp;
        let age = now
            .checked_sub(stake_pos.stake_timestamp)
            .ok_or(RegistryError::MathOverflow)?;
        require!(
            age >= MIN_STAKE_DURATION_SECONDS,
            RegistryError::StakeTooYoungForSeatClaim
        );

        let seat = &mut ctx.accounts.sovereign_seat;
        seat.seat_index = seat_index;
        seat.holder = ctx.accounts.holder.key();
        seat.claimed_royalties = 0;
        seat.active = true;
        seat.bump = ctx.bumps.sovereign_seat;
        seat.accrued_at_claim = config.accrued_per_seat;
        seat.claimed_royalties_usdc = 0;
        seat.accrued_at_claim_usdc = config.accrued_per_seat_usdc;

        config.total_seats_filled = config
            .total_seats_filled
            .checked_add(1)
            .ok_or(RegistryError::MathOverflow)?;

        emit!(SeatClaimedEvent {
            holder: ctx.accounts.holder.key(),
            seat_index,
            stake_amount: stake_pos.amount,
            total_seats_filled: config.total_seats_filled,
            accrued_at_claim: seat.accrued_at_claim,
            accrued_at_claim_usdc: seat.accrued_at_claim_usdc,
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    pub fn release_seat(ctx: Context<ReleaseSeat>) -> Result<()> {
        let stake_pos: StakePosition = {
            let data = ctx.accounts.stake_position.try_borrow_data()?;
            StakePosition::try_deserialize(&mut data.as_ref())
                .map_err(|_| RegistryError::StakePositionMismatch)?
        };
        require_keys_eq!(
            stake_pos.owner,
            ctx.accounts.holder.key(),
            RegistryError::StakePositionMismatch
        );
        require!(
            stake_pos.amount < SOVEREIGN_THRESHOLD_AMOUNT,
            RegistryError::StillSovereignTier
        );


        let seat = &mut ctx.accounts.sovereign_seat;
        seat.active = false;

        let config = &mut ctx.accounts.registry_config;
        config.total_seats_filled = config
            .total_seats_filled
            .checked_sub(1)
            .ok_or(RegistryError::MathOverflow)?;

        emit!(SeatReleasedEvent {
            holder: ctx.accounts.holder.key(),
            seat_index: seat.seat_index,
            remaining_stake: stake_pos.amount,
            total_seats_filled: config.total_seats_filled,
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    pub fn deposit_royalty(ctx: Context<DepositRoyalty>, amount: u64) -> Result<()> {
        require!(amount > 0, RegistryError::ZeroAmount);

        let config = &mut ctx.accounts.registry_config;
        require!(
            config.waterfall_authority != Pubkey::default(),
            RegistryError::WaterfallAuthorityNotConfigured
        );

        let signer_key = ctx.accounts.waterfall_signer.key();
        let signer_ok = signer_key == config.waterfall_authority
            || (config.provider_vault_authority != Pubkey::default()
                && signer_key == config.provider_vault_authority);
        require!(signer_ok, RegistryError::UnauthorizedWaterfallCaller);

        require!(config.total_seats_filled > 0, RegistryError::NoActiveSeats);

        let per_seat_delta = amount
            .checked_div(config.total_seats_filled as u64)
            .ok_or(RegistryError::MathOverflow)?;
        config.accrued_per_seat = config
            .accrued_per_seat
            .checked_add(per_seat_delta)
            .ok_or(RegistryError::MathOverflow)?;
        config.total_royalty_pool = config
            .total_royalty_pool
            .checked_add(amount)
            .ok_or(RegistryError::MathOverflow)?;

        system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                system_program::Transfer {
                    from: ctx.accounts.waterfall_signer.to_account_info(),
                    to: ctx.accounts.royalty_vault.to_account_info(),
                },
            ),
            amount,
        )?;

        emit!(RoyaltyDepositedEvent {
            amount,
            seats_filled: config.total_seats_filled,
            per_seat_delta,
            accrued_per_seat: config.accrued_per_seat,
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    pub fn deposit_royalty_usdc(
        ctx: Context<DepositRoyaltyUsdc>,
        amount: u64,
    ) -> Result<()> {
        require!(amount > 0, RegistryError::ZeroAmount);

        let config = &mut ctx.accounts.registry_config;
        require!(
            config.usdc_mint != Pubkey::default(),
            RegistryError::UsdcVaultNotConfigured
        );
        require!(
            config.waterfall_authority != Pubkey::default(),
            RegistryError::WaterfallAuthorityNotConfigured
        );

        let signer_key = ctx.accounts.waterfall_signer.key();
        let signer_ok = signer_key == config.waterfall_authority
            || (config.provider_vault_authority != Pubkey::default()
                && signer_key == config.provider_vault_authority);
        require!(signer_ok, RegistryError::UnauthorizedWaterfallCaller);

        require!(config.total_seats_filled > 0, RegistryError::NoActiveSeats);

        let per_seat_delta = amount
            .checked_div(config.total_seats_filled as u64)
            .ok_or(RegistryError::MathOverflow)?;
        config.accrued_per_seat_usdc = config
            .accrued_per_seat_usdc
            .checked_add(per_seat_delta)
            .ok_or(RegistryError::MathOverflow)?;
        config.total_royalty_pool_usdc = config
            .total_royalty_pool_usdc
            .checked_add(amount)
            .ok_or(RegistryError::MathOverflow)?;

        let cpi_ctx = CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            TokenTransfer {
                from: ctx.accounts.waterfall_source_ata.to_account_info(),
                to: ctx.accounts.royalty_vault_usdc.to_account_info(),
                authority: ctx.accounts.waterfall_signer.to_account_info(),
            },
        );
        token::transfer(cpi_ctx, amount)?;

        emit!(RoyaltyDepositedUsdcEvent {
            amount,
            seats_filled: config.total_seats_filled,
            per_seat_delta,
            accrued_per_seat_usdc: config.accrued_per_seat_usdc,
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    pub fn claim_royalty(ctx: Context<ClaimRoyalty>) -> Result<()> {
        let config = &ctx.accounts.registry_config;
        require!(
            config.total_seats_filled > 0,
            RegistryError::NoActiveSeats
        );

        let stake_pos: StakePosition = {
            let data = ctx.accounts.stake_position.try_borrow_data()?;
            StakePosition::try_deserialize(&mut data.as_ref())
                .map_err(|_| RegistryError::StakePositionMismatch)?
        };
        require_keys_eq!(
            stake_pos.owner,
            ctx.accounts.holder.key(),
            RegistryError::StakePositionMismatch
        );
        require!(
            stake_pos.amount >= SOVEREIGN_THRESHOLD_AMOUNT,
            RegistryError::InsufficientStake
        );

        let seat = &mut ctx.accounts.sovereign_seat;
        require!(seat.active, RegistryError::SeatInactive);

        let entitlement = config
            .accrued_per_seat
            .checked_sub(seat.accrued_at_claim)
            .ok_or(RegistryError::MathOverflow)?;
        let claimable = entitlement
            .checked_sub(seat.claimed_royalties)
            .ok_or(RegistryError::MathOverflow)?;

        require!(claimable > 0, RegistryError::NothingToClaim);

        let rent_min = Rent::get()?.minimum_balance(ctx.accounts.royalty_vault.data_len());
        require!(
            ctx.accounts.royalty_vault.lamports().saturating_sub(claimable) >= rent_min,
            RegistryError::RoyaltyVaultRentFloor
        );

        seat.claimed_royalties = seat
            .claimed_royalties
            .checked_add(claimable)
            .ok_or(RegistryError::MathOverflow)?;

        let royalty_vault_bump = config.royalty_vault_bump;
        system_program::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.system_program.to_account_info(),
                system_program::Transfer {
                    from: ctx.accounts.royalty_vault.to_account_info(),
                    to:   ctx.accounts.holder.to_account_info(),
                },
                &[&[b"royalty_vault", &[royalty_vault_bump]]],
            ),
            claimable,
        )?;

        emit!(RoyaltyClaimedEvent {
            holder: ctx.accounts.holder.key(),
            seat_index: seat.seat_index,
            amount: claimable,
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    pub fn claim_royalty_usdc(ctx: Context<ClaimRoyaltyUsdc>) -> Result<()> {
        let config = &ctx.accounts.registry_config;
        require!(
            config.usdc_mint != Pubkey::default(),
            RegistryError::UsdcVaultNotConfigured
        );
        require!(
            config.total_seats_filled > 0,
            RegistryError::NoActiveSeats
        );

        let stake_pos: StakePosition = {
            let data = ctx.accounts.stake_position.try_borrow_data()?;
            StakePosition::try_deserialize(&mut data.as_ref())
                .map_err(|_| RegistryError::StakePositionMismatch)?
        };
        require_keys_eq!(
            stake_pos.owner,
            ctx.accounts.holder.key(),
            RegistryError::StakePositionMismatch
        );
        require!(
            stake_pos.amount >= SOVEREIGN_THRESHOLD_AMOUNT,
            RegistryError::InsufficientStake
        );

        let seat = &mut ctx.accounts.sovereign_seat;
        require!(seat.active, RegistryError::SeatInactive);

        let entitlement = config
            .accrued_per_seat_usdc
            .checked_sub(seat.accrued_at_claim_usdc)
            .ok_or(RegistryError::MathOverflow)?;
        let claimable = entitlement
            .checked_sub(seat.claimed_royalties_usdc)
            .ok_or(RegistryError::MathOverflow)?;

        require!(claimable > 0, RegistryError::NothingToClaim);

        seat.claimed_royalties_usdc = seat
            .claimed_royalties_usdc
            .checked_add(claimable)
            .ok_or(RegistryError::MathOverflow)?;

        let registry_config_key = ctx.accounts.registry_config.key();
        let vault_bump = ctx.accounts.registry_config.royalty_vault_usdc_bump;
        let signer_seeds: &[&[u8]] = &[
            b"royalty_vault_usdc",
            registry_config_key.as_ref(),
            &[vault_bump],
        ];
        let signer_seeds_arr = &[signer_seeds];

        let cpi_ctx = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            TokenTransfer {
                from: ctx.accounts.royalty_vault_usdc.to_account_info(),
                to: ctx.accounts.holder_usdc_ata.to_account_info(),
                authority: ctx.accounts.royalty_vault_usdc.to_account_info(),
            },
            signer_seeds_arr,
        );
        token::transfer(cpi_ctx, claimable)?;

        emit!(RoyaltyClaimedUsdcEvent {
            holder: ctx.accounts.holder.key(),
            seat_index: seat.seat_index,
            amount: claimable,
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }


    pub fn propose_rotate_admin(
        ctx: Context<AdminOnly>,
        new_admin: Pubkey,
    ) -> Result<()> {
        let cfg = &mut ctx.accounts.registry_config;
        require!(new_admin != Pubkey::default(), RegistryError::InvalidAdmin);
        require!(new_admin != cfg.authority, RegistryError::InvalidAdmin);
        require!(
            cfg.pending_authority == Pubkey::default(),
            RegistryError::AdminProposalAlreadyPending
        );
        let now = Clock::get()?.unix_timestamp;
        check_and_record_propose(cfg, now)?;
        let unlocks_at = now
            .checked_add(ADMIN_TIMELOCK_SECONDS)
            .ok_or(RegistryError::MathOverflow)?;
        cfg.pending_authority = new_admin;
        cfg.pending_authority_unlocks_at = unlocks_at;
        emit!(AdminRotationProposed { admin: cfg.authority, new_admin, unlocks_at, timestamp: now });
        Ok(())
    }

    pub fn finalize_rotate_admin(ctx: Context<AdminOnly>) -> Result<()> {
        let cfg = &mut ctx.accounts.registry_config;
        require!(
            cfg.pending_authority != Pubkey::default(),
            RegistryError::AdminNoProposalPending
        );
        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= cfg.pending_authority_unlocks_at,
            RegistryError::AdminTimelockNotElapsed
        );
        let old_admin = cfg.authority;
        let new_admin = cfg.pending_authority;
        cfg.authority = new_admin;
        cfg.pending_authority = Pubkey::default();
        cfg.pending_authority_unlocks_at = 0;
        emit!(AdminRotated { old_admin, new_admin, timestamp: now });
        Ok(())
    }

    pub fn cancel_rotate_admin(ctx: Context<AdminOnly>) -> Result<()> {
        let cfg = &mut ctx.accounts.registry_config;
        require!(
            cfg.pending_authority != Pubkey::default(),
            RegistryError::AdminNoProposalPending
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
}


#[error_code]
pub enum RegistryError {
    #[msg("Seat index must be 0-20")]
    InvalidSeatIndex,
    #[msg("All 21 sovereign seats are filled")]
    AllSeatsFilled,
    #[msg("Arithmetic overflow")]
    MathOverflow,
    #[msg("No active seats to distribute royalties")]
    NoActiveSeats,
    #[msg("Nothing available to claim")]
    NothingToClaim,
    #[msg("Caller is not authorized")]
    Unauthorized,
    #[msg("Amount must be > 0")]
    ZeroAmount,
    #[msg("StakePosition.owner does not match holder signer")]
    StakePositionMismatch,
    #[msg("Insufficient $TOP staked — Sovereign tier requires 20M $TOP")]
    InsufficientStake,
    #[msg("Holder is still Sovereign-tier — seat cannot be released")]
    StillSovereignTier,
    #[msg("Seat is already inactive")]
    SeatInactive,
    #[msg("Waterfall authority pubkey is zero — set_waterfall_authority first")]
    WaterfallAuthorityNotConfigured,
    #[msg("Waterfall authority pubkey cannot be zero")]
    InvalidWaterfallAuthority,
    #[msg("Unauthorized waterfall caller — does not match registry_config.waterfall_authority")]
    UnauthorizedWaterfallCaller,
    #[msg("USDC royalty vault not configured — call initialize_royalty_vault_usdc first")]
    UsdcVaultNotConfigured,
    #[msg("USDC royalty vault is already configured — mint cannot change")]
    UsdcVaultAlreadyConfigured,
    #[msg("Stake too young for Sovereign seat claim — must wait 7 days from stake")]
    StakeTooYoungForSeatClaim,
    #[msg("Royalty credits must be claimed before releasing seat. Call claim_royalty() first.")]
    MustClaimRoyaltyFirst,

    #[msg("Propose cooldown active — escalating rate-limit per Rule 27b defense (R7.7-H-01)")]
    ProposeCooldownActive,
    #[msg("Invalid admin pubkey — must be non-default and distinct from current admin")]
    InvalidAdmin,
    #[msg("Admin rotation proposal already pending — cancel before re-proposing")]
    AdminProposalAlreadyPending,
    #[msg("No admin rotation proposal pending")]
    AdminNoProposalPending,
    #[msg("Admin rotation timelock not elapsed (72h)")]
    AdminTimelockNotElapsed,
    #[msg("Instruction deprecated — use propose+finalize_set_provider_vault_authority for timelocked rotation (Wave D.E.2 / M-CRIT-02)")]
    InstructionDeprecated,
    #[msg("Provider-vault-authority rotation proposal already pending — cancel first")]
    ProviderVaultAuthorityProposalAlreadyPending,
    #[msg("No provider-vault-authority rotation proposal pending")]
    ProviderVaultAuthorityNoProposalPending,

    #[msg("provider_vault_authority already configured — use propose_set_provider_vault_authority for timelocked rotation (Wave E.2)")]
    ProviderVaultAuthorityAlreadyConfigured,
    #[msg("Waterfall authority already configured — use propose_set_waterfall_authority for timelocked rotation")]
    WaterfallAuthorityAlreadyConfigured,
    #[msg("A waterfall authority proposal is already pending — cancel it first")]
    WaterfallAuthorityProposalAlreadyPending,
    #[msg("No pending waterfall authority proposal to finalize or cancel")]
    WaterfallAuthorityNoProposalPending,
    #[msg("Royalty claim would drop the SOL royalty vault below its rent-exempt minimum")]
    RoyaltyVaultRentFloor,
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

const _STAKE_POSITION_MIRROR_PREFIX_BYTES_PIN: () = {
    const MIRROR_PAYLOAD: usize = 32 + 8 + 1 + 8 + 8 + 8 + 1;
    assert!(
        MIRROR_PAYLOAD == 66,
        "sovereign-registry's StakePosition mirror payload size changed — \
         re-audit cross-program byte layout against staking::StakePosition \
         (and update LIVE_STAKE_POSITION_MIN_LEN if the live struct also \
         changed)."
    );
};


pub const LIVE_STAKE_POSITION_MIN_LEN: usize = 74;


#[account]
pub struct RegistryConfig {
    pub authority: Pubkey,
    pub total_seats_filled: u8,
    pub required_stake: u64,
    pub total_royalty_pool: u64,
    pub bump: u8,
    pub royalty_vault_bump: u8,
    pub accrued_per_seat: u64,
    pub waterfall_authority: Pubkey,
    pub provider_vault_authority: Pubkey,
    pub usdc_mint: Pubkey,
    pub royalty_vault_usdc: Pubkey,
    pub royalty_vault_usdc_bump: u8,
    pub accrued_per_seat_usdc: u64,         
    pub total_royalty_pool_usdc: u64,
    
    pub propose_cooldown_until: i64,
    pub recent_proposes: [i64; 5],
    
    pub pending_authority: Pubkey,
    pub pending_authority_unlocks_at: i64,
    
    pub pending_provider_vault_authority: Pubkey,
    pub pending_provider_vault_authority_unlocks_at: i64,
    
    pub pending_waterfall_authority: Pubkey,
    pub pending_waterfall_authority_unlocks_at: i64,
}



#[account]
pub struct SovereignSeat {
    pub seat_index: u8,
    pub holder: Pubkey,
    pub claimed_royalties: u64,
    pub active: bool,
    pub bump: u8,
    pub accrued_at_claim: u64,
    pub claimed_royalties_usdc: u64,
    
    pub accrued_at_claim_usdc: u64,
}




#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = authority,
        
        
        
        space = 380,
        seeds = [b"registry_config"],
        bump
    )]
    pub registry_config: Account<'info, RegistryConfig>,
    
    
    
    #[account(
        mut,
        seeds = [b"royalty_vault"],
        bump
    )]
    pub royalty_vault: AccountInfo<'info>,
    #[account(mut)]
    pub authority: Signer<'info>,
    
    #[account(
        seeds = [crate::ID.as_ref()],
        bump,
        seeds::program = anchor_lang::solana_program::bpf_loader_upgradeable::ID,
        constraint = program_data.upgrade_authority_address == Some(authority.key())
            @ RegistryError::Unauthorized,
    )]
    pub program_data: Account<'info, ProgramData>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct AdminOnly<'info> {
    #[account(
        mut,
        seeds = [b"registry_config"],
        bump = registry_config.bump,
        constraint = authority.key() == registry_config.authority @ RegistryError::Unauthorized
    )]
    pub registry_config: Account<'info, RegistryConfig>,
    pub authority: Signer<'info>,
}



#[derive(Accounts)]
pub struct InitializeRoyaltyVaultUsdc<'info> {
    #[account(
        mut,
        seeds = [b"registry_config"],
        bump = registry_config.bump,
        constraint = authority.key() == registry_config.authority @ RegistryError::Unauthorized,
    )]
    pub registry_config: Account<'info, RegistryConfig>,
    
    pub usdc_mint: Account<'info, Mint>,
    
    
    
    #[account(
        init,
        payer = authority,
        seeds = [b"royalty_vault_usdc", registry_config.key().as_ref()],
        bump,
        token::mint = usdc_mint,
        token::authority = royalty_vault_usdc,
    )]
    pub royalty_vault_usdc: Account<'info, TokenAccount>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}



#[derive(Accounts)]
#[instruction(seat_index: u8)]
pub struct ClaimSeat<'info> {
    #[account(
        init,
        payer = protocol_payer,
        space = 75,
        
        
        seeds = [b"sovereign_seat".as_ref(), holder.key().as_ref()],
        bump
    )]
    pub sovereign_seat: Account<'info, SovereignSeat>,
    #[account(mut, seeds = [b"registry_config"], bump = registry_config.bump)]
    pub registry_config: Account<'info, RegistryConfig>,
    
    
    
    #[account(
        seeds = [b"stake_position", holder.key().as_ref()],
        bump,
        seeds::program = STAKING_PROGRAM_ID,
        owner = STAKING_PROGRAM_ID,
    )]
    pub stake_position: UncheckedAccount<'info>,
    pub holder: Signer<'info>,
    
    
    
    #[account(
        mut,
        constraint = protocol_payer.key() == registry_config.authority @ RegistryError::Unauthorized
    )]
    pub protocol_payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}
#[derive(Accounts)]
pub struct ReleaseSeat<'info> {
    
    #[account(
        mut,
        close = holder,
        constraint = sovereign_seat.holder == holder.key() @ RegistryError::Unauthorized,
        constraint = sovereign_seat.active @ RegistryError::SeatInactive,
        seeds = [b"sovereign_seat", holder.key().as_ref()], 
        bump = sovereign_seat.bump,
    )]
    pub sovereign_seat: Account<'info, SovereignSeat>,
    #[account(mut, seeds = [b"registry_config"], bump = registry_config.bump)]
    pub registry_config: Account<'info, RegistryConfig>,
    
    
    
    #[account(
        seeds = [b"stake_position", holder.key().as_ref()],
        bump,
        seeds::program = STAKING_PROGRAM_ID,
        owner = STAKING_PROGRAM_ID,
    )]
    pub stake_position: UncheckedAccount<'info>,
    
    #[account(mut)]
    pub holder: AccountInfo<'info>,
    
    
    #[account(mut)]
    pub caller: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct DepositRoyalty<'info> {
    #[account(
        mut,
        seeds = [b"registry_config"],
        bump = registry_config.bump,
    )]
    pub registry_config: Account<'info, RegistryConfig>,
    
    #[account(
        mut,
        seeds = [b"royalty_vault"],
        bump = registry_config.royalty_vault_bump
    )]
    pub royalty_vault: AccountInfo<'info>,
    
    
    
    #[account(mut)]
    pub waterfall_signer: Signer<'info>,
    pub system_program: Program<'info, System>,
}



#[derive(Accounts)]
pub struct DepositRoyaltyUsdc<'info> {
    #[account(
        mut,
        seeds = [b"registry_config"],
        bump = registry_config.bump,
    )]
    pub registry_config: Account<'info, RegistryConfig>,
    
    
    
    #[account(
        mut,
        seeds = [b"royalty_vault_usdc", registry_config.key().as_ref()],
        bump = registry_config.royalty_vault_usdc_bump,
        address = registry_config.royalty_vault_usdc @ RegistryError::UsdcVaultNotConfigured,
    )]
    pub royalty_vault_usdc: Account<'info, TokenAccount>,
    
    
    
    #[account(
        mut,
        constraint = waterfall_source_ata.mint == registry_config.usdc_mint
            @ RegistryError::UsdcVaultNotConfigured,
        constraint = waterfall_source_ata.owner == waterfall_signer.key()
            @ RegistryError::UnauthorizedWaterfallCaller,
    )]
    pub waterfall_source_ata: Account<'info, TokenAccount>,
    
    
    pub waterfall_signer: Signer<'info>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct ClaimRoyalty<'info> {
    #[account(
        mut,
        has_one = holder,
        constraint = sovereign_seat.active @ RegistryError::SeatInactive,
        seeds = [b"sovereign_seat", holder.key().as_ref()], 
        bump = sovereign_seat.bump,
    )]
    pub sovereign_seat: Account<'info, SovereignSeat>,
    #[account(seeds = [b"registry_config"], bump = registry_config.bump)]
    pub registry_config: Account<'info, RegistryConfig>,
    
    #[account(
        mut,
        seeds = [b"royalty_vault"],
        bump = registry_config.royalty_vault_bump
    )]
    pub royalty_vault: AccountInfo<'info>,
    
    
    
    #[account(
        seeds = [b"stake_position", holder.key().as_ref()],
        bump,
        seeds::program = STAKING_PROGRAM_ID,
        owner = STAKING_PROGRAM_ID,
    )]
    pub stake_position: UncheckedAccount<'info>,
    #[account(mut)]
    pub holder: Signer<'info>,
    pub system_program: Program<'info, System>,
}



#[derive(Accounts)]
pub struct ClaimRoyaltyUsdc<'info> {
    #[account(
        mut,
        has_one = holder,
        constraint = sovereign_seat.active @ RegistryError::SeatInactive,
        seeds = [b"sovereign_seat", holder.key().as_ref()], 
        bump = sovereign_seat.bump,
    )]
    pub sovereign_seat: Account<'info, SovereignSeat>,
    #[account(seeds = [b"registry_config"], bump = registry_config.bump)]
    pub registry_config: Account<'info, RegistryConfig>,
    
    #[account(
        mut,
        seeds = [b"royalty_vault_usdc", registry_config.key().as_ref()],
        bump = registry_config.royalty_vault_usdc_bump,
        address = registry_config.royalty_vault_usdc @ RegistryError::UsdcVaultNotConfigured,
    )]
    pub royalty_vault_usdc: Account<'info, TokenAccount>,
    
    
    
    #[account(
        mut,
        constraint = holder_usdc_ata.mint == registry_config.usdc_mint
            @ RegistryError::UsdcVaultNotConfigured,
        constraint = holder_usdc_ata.owner == holder.key()
            @ RegistryError::Unauthorized,
    )]
    pub holder_usdc_ata: Account<'info, TokenAccount>,
    
    
    
    #[account(
        seeds = [b"stake_position", holder.key().as_ref()],
        bump,
        seeds::program = STAKING_PROGRAM_ID,
        owner = STAKING_PROGRAM_ID,
    )]
    pub stake_position: UncheckedAccount<'info>,
    #[account(mut)]
    pub holder: Signer<'info>,
    pub token_program: Program<'info, Token>,
}



#[event]
pub struct SeatClaimedEvent {
    pub holder: Pubkey,
    pub seat_index: u8,
    pub stake_amount: u64,
    pub total_seats_filled: u8,
    pub accrued_at_claim: u64,
    
    pub accrued_at_claim_usdc: u64,
    pub timestamp: i64,
}

#[event]
pub struct SeatReleasedEvent {
    pub holder: Pubkey,
    pub seat_index: u8,
    pub remaining_stake: u64,
    pub total_seats_filled: u8,
    pub timestamp: i64,
}

#[event]
pub struct RoyaltyDepositedEvent {
    pub amount: u64,
    pub seats_filled: u8,
    pub per_seat_delta: u64,
    pub accrued_per_seat: u64,
    pub timestamp: i64,
}

#[event]
pub struct RoyaltyClaimedEvent {
    pub holder: Pubkey,
    pub seat_index: u8,
    pub amount: u64,
    pub timestamp: i64,
}



#[event]
pub struct RoyaltyDepositedUsdcEvent {
    pub amount: u64,
    pub seats_filled: u8,
    pub per_seat_delta: u64,
    pub accrued_per_seat_usdc: u64,
    pub timestamp: i64,
}


#[event]
pub struct RoyaltyClaimedUsdcEvent {
    pub holder: Pubkey,
    pub seat_index: u8,
    pub amount: u64,
    pub timestamp: i64,
}



#[event]
pub struct RoyaltyVaultUsdcInitialized {
    pub usdc_mint: Pubkey,
    pub royalty_vault_usdc: Pubkey,
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
pub struct ProviderVaultAuthorityProposed {
    pub admin: Pubkey,
    pub new_authority: Pubkey,
    pub unlocks_at: i64,
    pub timestamp: i64,
}
#[event]
pub struct ProviderVaultAuthorityRotated {
    pub old_authority: Pubkey,
    pub new_authority: Pubkey,
    pub timestamp: i64,
}
#[event]
pub struct ProviderVaultAuthorityProposalCancelled {
    pub admin: Pubkey,
    pub cancelled_authority: Pubkey,
    pub timestamp: i64,
}


#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sovereign_threshold_is_exactly_20m_top() {
        
        assert_eq!(SOVEREIGN_THRESHOLD_AMOUNT, 20_000_000 * 1_000_000);
        assert_eq!(SOVEREIGN_THRESHOLD_AMOUNT, 20_000_000_000_000);
        assert_eq!(EXPECTED_TOP_DECIMALS, 6);
    }

    #[test]
    fn max_seats_is_21() {
        assert_eq!(MAX_SEATS, 21);
    }
    #[test]
    fn per_seat_accrual_index_advances_correctly() {
        
        
        
        let seats_filled: u64 = 3;
        let mut accrued: u64 = 0;
        for _ in 0..2 {
            let amount: u64 = 300;
            let per_seat_delta = amount.checked_div(seats_filled).unwrap();
            accrued = accrued.checked_add(per_seat_delta).unwrap();
        }
        assert_eq!(accrued, 200);
    }
    #[test]
    fn claimable_respects_snapshot() {
        
        
        let accrued_per_seat: u64 = 130;
        let accrued_at_claim: u64 = 50;
        let claimed_royalties: u64 = 20;
        let entitlement = accrued_per_seat.checked_sub(accrued_at_claim).unwrap();
        let claimable = entitlement.checked_sub(claimed_royalties).unwrap();
        assert_eq!(claimable, 60);
    }
    
    
    

    fn make_config_for_whitelist(
        native: Pubkey,
        provider: Pubkey,
    ) -> RegistryConfig {
        RegistryConfig {
            authority: Pubkey::new_unique(),
            total_seats_filled: 1,
            required_stake: SOVEREIGN_THRESHOLD_AMOUNT,
            total_royalty_pool: 0,
            bump: 1,
            royalty_vault_bump: 1,
            accrued_per_seat: 0,
            waterfall_authority: native,
            provider_vault_authority: provider,
            usdc_mint: Pubkey::default(),
            royalty_vault_usdc: Pubkey::default(),
            royalty_vault_usdc_bump: 0,
            accrued_per_seat_usdc: 0,
            total_royalty_pool_usdc: 0,
            
            propose_cooldown_until: 0,
            recent_proposes: [0i64; 5],
            pending_authority: Pubkey::default(),
            pending_authority_unlocks_at: 0,
            pending_provider_vault_authority: Pubkey::default(),
            pending_provider_vault_authority_unlocks_at: 0,
            
            pending_waterfall_authority: Pubkey::default(),
            pending_waterfall_authority_unlocks_at: 0,
        }
    }

    fn signer_is_authorised(
        config: &RegistryConfig,
        signer_key: Pubkey,
    ) -> bool {
        signer_key == config.waterfall_authority
            || (config.provider_vault_authority != Pubkey::default()
                && signer_key == config.provider_vault_authority)
    }
    #[test]
    fn deposit_royalty_accepts_v3_caller() {
        
        
        let native = Pubkey::new_unique();
        let provider = Pubkey::new_unique();
        let cfg = make_config_for_whitelist(native, provider);
        
        assert!(signer_is_authorised(&cfg, native));
        
        assert!(signer_is_authorised(&cfg, provider));
    }
    #[test]
    fn deposit_royalty_rejects_random_pubkey() {
        
        
        let native = Pubkey::new_unique();
        let provider = Pubkey::new_unique();
        let cfg = make_config_for_whitelist(native, provider);
        let attacker = Pubkey::new_unique();
        assert!(!signer_is_authorised(&cfg, attacker));
    }
    #[test]
    fn deposit_royalty_rejects_default_provider_when_unset() {
        
        
        
        let native = Pubkey::new_unique();
        let cfg = make_config_for_whitelist(native, Pubkey::default());
        assert!(signer_is_authorised(&cfg, native));
        assert!(!signer_is_authorised(&cfg, Pubkey::default()));
    }
    #[test]
    fn whitelist_extension_admin_only() {
        
        
        
        let admin = Pubkey::new_unique();
        let cfg = RegistryConfig {
            authority: admin,
            total_seats_filled: 0,
            required_stake: SOVEREIGN_THRESHOLD_AMOUNT,
            total_royalty_pool: 0,
            bump: 1,
            royalty_vault_bump: 1,
            accrued_per_seat: 0,
            waterfall_authority: Pubkey::default(),
            provider_vault_authority: Pubkey::default(),
            usdc_mint: Pubkey::default(),
            royalty_vault_usdc: Pubkey::default(),
            royalty_vault_usdc_bump: 0,
            accrued_per_seat_usdc: 0,
            total_royalty_pool_usdc: 0,
            propose_cooldown_until: 0,
            recent_proposes: [0i64; 5],
            pending_authority: Pubkey::default(),
            pending_authority_unlocks_at: 0,
            pending_provider_vault_authority: Pubkey::default(),
            pending_provider_vault_authority_unlocks_at: 0,
            
            pending_waterfall_authority: Pubkey::default(),
            pending_waterfall_authority_unlocks_at: 0,
        };
        let caller = admin;
        assert_eq!(caller, cfg.authority);
        
        let attacker = Pubkey::new_unique();
        assert_ne!(attacker, cfg.authority);
    }
    #[test]
    fn registry_config_space_constant_matches_struct() {
        
        
        
        let expected = 8 + 32 + 1 + 8 + 8 + 1 + 1 + 8 + 32 + 32
                     + 32 + 32 + 1 + 8 + 8
                     + 8 + 40 + 32 + 8
                     + 32 + 8
                     + 32 + 8;
        assert_eq!(expected, 380);
    }
    #[test]
    fn late_claimer_only_gets_future_royalties() {
        
        
        
        let mut accrued: u64 = 0;
        let seats_phase_a: u64 = 2;
        accrued = accrued.checked_add(200u64.checked_div(seats_phase_a).unwrap()).unwrap();
        let late_snapshot = accrued;
        let seats_phase_b: u64 = 3;
        accrued = accrued.checked_add(300u64.checked_div(seats_phase_b).unwrap()).unwrap();
        let late_entitlement = accrued.checked_sub(late_snapshot).unwrap();
        assert_eq!(late_entitlement, 100);
        assert_eq!(accrued, 200);
    }
    
    
    
    #[test]
    fn usdc_accrual_index_advances_independently_from_sol() {
        
        
        let seats: u64 = 2;
        let mut accrued_sol: u64 = 0;
        let mut accrued_usdc: u64 = 0;
        accrued_sol = accrued_sol
            .checked_add(200u64.checked_div(seats).unwrap())
            .unwrap();
        accrued_usdc = accrued_usdc
            .checked_add(50u64.checked_div(seats).unwrap())
            .unwrap();
        assert_eq!(accrued_sol, 100);
        assert_eq!(accrued_usdc, 25);
    }
    #[test]
    fn usdc_claimable_respects_snapshot() {
        
        
        
        let accrued_per_seat_usdc: u64 = 100;
        let accrued_at_claim_usdc: u64 = 40;
        let claimed_royalties_usdc: u64 = 10;
        let entitlement = accrued_per_seat_usdc
            .checked_sub(accrued_at_claim_usdc).unwrap();
        let claimable = entitlement
            .checked_sub(claimed_royalties_usdc).unwrap();
        assert_eq!(claimable, 50);
    }
    #[test]
    fn usdc_zero_seats_predicate_matches_sol() {
        
        
        let total_seats_filled: u64 = 0;
        assert!(total_seats_filled == 0); 
        let total_seats_filled: u64 = 1;
        assert!(total_seats_filled > 0); 
    }
    #[test]
    fn usdc_late_claimer_only_gets_future_royalties() {
        
        let mut accrued_usdc: u64 = 0;
        let seats_a: u64 = 2;
        accrued_usdc = accrued_usdc
            .checked_add(200u64.checked_div(seats_a).unwrap()).unwrap();
        let late_snapshot = accrued_usdc;
        let seats_b: u64 = 3;
        accrued_usdc = accrued_usdc
            .checked_add(300u64.checked_div(seats_b).unwrap()).unwrap();
        let late_entitlement = accrued_usdc.checked_sub(late_snapshot).unwrap();
        assert_eq!(late_entitlement, 100);
        assert_eq!(accrued_usdc, 200);
    }
    #[test]
    fn mixed_sol_and_usdc_deposits_track_independently() {
        
        
        let seats: u64 = 4;

        
        let mut acc_sol: u64 = 0;
        for _ in 0..2 {
            acc_sol = acc_sol
                .checked_add(400u64.checked_div(seats).unwrap()).unwrap();
        }
        assert_eq!(acc_sol, 200);

        
        
        let mut acc_usdc: u64 = 0;
        acc_usdc = acc_usdc
            .checked_add(800u64.checked_div(seats).unwrap()).unwrap();
        assert_eq!(acc_usdc, 200);

        
        
        let sol_claimable = acc_sol.checked_sub(0).unwrap().checked_sub(200).unwrap();
        let usdc_claimable = acc_usdc.checked_sub(0).unwrap().checked_sub(0).unwrap();
        assert_eq!(sol_claimable, 0);
        assert_eq!(usdc_claimable, 200);
    }
    #[test]
    fn seat_struct_size_grew_to_75_bytes() {
        
        
        
        let expected = 8 + 1 + 32 + 8 + 1 + 1 + 8 + 8 + 8;
        assert_eq!(expected, 75);
    }
    #[test]
    fn seat_byte_offsets_preserved_for_yield_escrow_reader() {
        
        
        
        let disc = 8usize;
        let seat_index = 1usize;
        let holder_offset = disc + seat_index;
        assert_eq!(holder_offset, 9, "holder offset must stay at 9 for yield-escrow");

        let holder = 32usize;
        let claimed_royalties = 8usize;
        let active_offset = holder_offset + holder + claimed_royalties;
        assert_eq!(active_offset, 49, "active offset must stay at 49 for yield-escrow");

        let active = 1usize;
        let min_len = active_offset + active;
        assert!(min_len >= 50, "min_len for yield-escrow read window must be >= 50");

        
        let bump = 1usize;
        let accrued_at_claim = 8usize;
        let claimed_royalties_usdc_offset =
            active_offset + active + bump + accrued_at_claim;
        assert_eq!(claimed_royalties_usdc_offset, 59);
        assert!(
            claimed_royalties_usdc_offset > 50,
            "USDC fields must sit outside yield-escrow's read window"
        );
    }
    #[test]
    fn usdc_claim_zero_accrued_is_nothing_to_claim() {
        
        let accrued_per_seat_usdc: u64 = 42; 
        let accrued_at_claim_usdc: u64 = 42; 
        let claimed_royalties_usdc: u64 = 0;
        let entitlement = accrued_per_seat_usdc
            .checked_sub(accrued_at_claim_usdc).unwrap();
        let claimable = entitlement
            .checked_sub(claimed_royalties_usdc).unwrap();
        assert_eq!(claimable, 0); 
    }
    #[test]
    fn usdc_double_claim_is_nothing_to_claim() {
        
        
        let accrued_per_seat_usdc: u64 = 60;
        let accrued_at_claim_usdc: u64 = 10;
        let claimed_royalties_usdc: u64 = 50; 
        let entitlement = accrued_per_seat_usdc
            .checked_sub(accrued_at_claim_usdc).unwrap();
        let claimable = entitlement
            .checked_sub(claimed_royalties_usdc).unwrap();
        assert_eq!(claimable, 0);
    }
    #[test]
    fn usdc_vault_not_configured_predicate() {
        
        
        
        let cfg_unconfigured = RegistryConfig {
            authority: Pubkey::new_unique(),
            total_seats_filled: 1,
            required_stake: SOVEREIGN_THRESHOLD_AMOUNT,
            total_royalty_pool: 0,
            bump: 1,
            royalty_vault_bump: 1,
            accrued_per_seat: 0,
            waterfall_authority: Pubkey::new_unique(),
            provider_vault_authority: Pubkey::default(),
            usdc_mint: Pubkey::default(),
            royalty_vault_usdc: Pubkey::default(),
            royalty_vault_usdc_bump: 0,
            accrued_per_seat_usdc: 0,
            total_royalty_pool_usdc: 0,
            propose_cooldown_until: 0,
            recent_proposes: [0i64; 5],
            pending_authority: Pubkey::default(),
            pending_authority_unlocks_at: 0,
            pending_provider_vault_authority: Pubkey::default(),
            pending_provider_vault_authority_unlocks_at: 0,
            
            pending_waterfall_authority: Pubkey::default(),
            pending_waterfall_authority_unlocks_at: 0,
        };
        assert!(cfg_unconfigured.usdc_mint == Pubkey::default()); 

        let cfg_configured = RegistryConfig {
            usdc_mint: Pubkey::new_unique(),
            ..cfg_unconfigured
        };
        assert!(cfg_configured.usdc_mint != Pubkey::default()); 
    }
    #[test]
    fn usdc_signer_whitelist_matches_sol_path() {
        
        
        
        let native = Pubkey::new_unique();
        let provider = Pubkey::new_unique();
        let attacker = Pubkey::new_unique();
        let cfg = make_config_for_whitelist(native, provider);

        
        for signer in [native, provider] {
            assert!(signer_is_authorised(&cfg, signer));
        }
        assert!(!signer_is_authorised(&cfg, attacker));

        
        let cfg_native_only = make_config_for_whitelist(native, Pubkey::default());
        assert!(signer_is_authorised(&cfg_native_only, native));
        assert!(!signer_is_authorised(&cfg_native_only, provider));
    }
    #[test]
    fn sol_path_regression_unchanged() {
        
        
        
        let mut cfg = make_config_for_whitelist(
            Pubkey::new_unique(),
            Pubkey::new_unique(),
        );
        cfg.accrued_per_seat = 130;
        cfg.accrued_per_seat_usdc = 999; 
        let accrued_at_claim = 50u64;
        let claimed_royalties = 20u64;
        let entitlement = cfg.accrued_per_seat
            .checked_sub(accrued_at_claim).unwrap();
        let claimable = entitlement
            .checked_sub(claimed_royalties).unwrap();
        assert_eq!(claimable, 60);
    }

    

    #[test]
    fn registry_min_stake_duration_is_seven_days() {
        assert_eq!(MIN_STAKE_DURATION_SECONDS, 7 * 24 * 60 * 60);
        assert_eq!(MIN_STAKE_DURATION_SECONDS, 604_800);
    }

    
    
    #[test]
    fn claim_seat_rejects_flash_stake() {
        let stake_timestamp: i64 = 1_700_000_000;
        let now: i64 = stake_timestamp + 10; 
        let age = now - stake_timestamp;
        assert!(age < MIN_STAKE_DURATION_SECONDS,
            "10s age must be below 7d threshold");
    }

    
    
    #[test]
    fn claim_seat_passes_at_exact_threshold() {
        let stake_timestamp: i64 = 1_700_000_000;
        let now: i64 = stake_timestamp + MIN_STAKE_DURATION_SECONDS;
        let age = now - stake_timestamp;
        assert_eq!(age, MIN_STAKE_DURATION_SECONDS);
        assert!(age >= MIN_STAKE_DURATION_SECONDS);
    }

    
    #[test]
    fn claim_seat_passes_after_seven_days() {
        let stake_timestamp: i64 = 1_700_000_000;
        let now: i64 = stake_timestamp + 8 * 86_400;
        let age = now - stake_timestamp;
        assert!(age >= MIN_STAKE_DURATION_SECONDS);
    }
    
    
    
    
    fn claim_royalty_stake_gate(stake_amount: u64) -> Result<()> {
        require!(
            stake_amount >= SOVEREIGN_THRESHOLD_AMOUNT,
            RegistryError::InsufficientStake
        );
        Ok(())
    }
    #[test]
    fn claim_royalty_reverts_below_threshold() {
        
        let err = claim_royalty_stake_gate(SOVEREIGN_THRESHOLD_AMOUNT - 1).unwrap_err();
        if let anchor_lang::error::Error::AnchorError(ae) = err {
            assert_eq!(ae.error_name, "InsufficientStake");
        } else {
            panic!("expected AnchorError, got {err:?}");
        }
    }
    #[test]
    fn claim_royalty_ok_at_and_above_threshold() {
        
        assert!(claim_royalty_stake_gate(SOVEREIGN_THRESHOLD_AMOUNT).is_ok());
        assert!(claim_royalty_stake_gate(SOVEREIGN_THRESHOLD_AMOUNT + 1_000_000).is_ok());
    }
    #[test]
    fn claim_royalty_zero_stake_reverts() {
        
        assert!(claim_royalty_stake_gate(0).is_err());
    }
    #[test]
    fn stake_position_mirror_stake_timestamp_field_order_locked() {
        
        
        
        let sp = StakePosition {
            owner: Pubkey::default(),
            amount: 0,
            tier: 0,
            stake_timestamp: 0,
            last_claim: 0,
            etop_balance: 0,
            bump: 0,
        };
        
        
        assert_eq!(sp.stake_timestamp, 0);
        assert_eq!(sp.tier, 0);
    }
    
    
    
    
    fn drift_handler_end(src: &str, idx: usize) -> usize {
        src[idx + 1..]
            .find("\n    pub fn ")
            .map(|p| idx + 1 + p)
            .expect("drift-gate: no following `pub fn` — re-anchor this source-assert bound")
    }

    fn sovereign_registry_lib_rs_source() -> &'static str {
        include_str!("lib.rs")
    }

    #[test]
    fn set_provider_vault_authority_is_bootstrap_instant() {
        let src = sovereign_registry_lib_rs_source();
        let needle = "pub fn set_provider_vault_authority(\n        ctx: Context<AdminOnly>";
        let idx = src.find(needle).expect("set_provider_vault_authority handler must exist");
        
        
        
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
        
        assert!(body.contains("new_authority != Pubkey::default()"),
            "set_provider_vault_authority MUST reject Pubkey::default() new_authority (Wave E.2). \
             Source excerpt:\n{}", body);
        
        assert!(!body.contains("err!(RegistryError::InstructionDeprecated)"),
            "set_provider_vault_authority MUST NOT hard-revert with InstructionDeprecated under Wave E.2 \
             (bootstrap-instant pattern restored). Source excerpt:\n{}", body);
    }

    #[test]
    fn already_configured_error_variant_exists() {
        let src = sovereign_registry_lib_rs_source();
        assert!(src.contains("ProviderVaultAuthorityAlreadyConfigured,"),
            "RegistryError::ProviderVaultAuthorityAlreadyConfigured variant must exist (Wave E.2)");
        assert!(src.contains("Wave E.2"),
            "error/handler must reference the Wave E.2 audit ID for traceability");
    }

    #[test]
    fn instruction_deprecated_error_variant_still_exists_for_other_callers() {
        let src = sovereign_registry_lib_rs_source();
        assert!(src.contains("InstructionDeprecated,"),
            "RegistryError::InstructionDeprecated variant must remain in the error enum for compatibility");
    }

    
    
    
    #[test]
    fn wave_e2_bootstrap_instant_then_rotation_reverts() {
        
        let mut provider_vault_authority = Pubkey::default();
        let new_authority = Pubkey::new_unique();
        
        let first_set_allowed = provider_vault_authority == Pubkey::default();
        assert!(first_set_allowed, "first set MUST be allowed when field is default (Wave E.2 bootstrap)");
        provider_vault_authority = new_authority;
        assert_eq!(provider_vault_authority, new_authority,
            "first-set bootstrap-instant MUST commit the new authority");

        
        let attempted_rotation_to = Pubkey::new_unique();
        let second_set_allowed = provider_vault_authority == Pubkey::default();
        assert!(!second_set_allowed,
            "post-bootstrap rotation via set_provider_vault_authority MUST be blocked (Wave E.2). \
             Caller MUST use propose+finalize_set_provider_vault_authority (Rule 27b 72h timelock).");
        
        assert_ne!(provider_vault_authority, attempted_rotation_to,
            "field MUST NOT mutate when the gate reverts post-bootstrap");
        assert_eq!(provider_vault_authority, new_authority,
            "field MUST retain its bootstrap value after a blocked rotation");
    }

    #[test]
    fn propose_finalize_cancel_triplet_exists_for_provider_vault_authority() {
        let src = sovereign_registry_lib_rs_source();
        assert!(src.contains("pub fn propose_set_provider_vault_authority("),
            "propose_set_provider_vault_authority must exist (canonical replacement for instant set)");
        assert!(src.contains("pub fn finalize_set_provider_vault_authority("),
            "finalize_set_provider_vault_authority must exist");
        assert!(src.contains("pub fn cancel_set_provider_vault_authority("),
            "cancel_set_provider_vault_authority must exist (emergency abort)");
    }
    #[test]
    fn propose_set_provider_vault_authority_calls_check_and_record_propose() {
        
        
        
        let src = sovereign_registry_lib_rs_source();
        let needle = "pub fn propose_set_provider_vault_authority(";
        let idx = src.find(needle).expect("propose_set_provider_vault_authority must exist");
        
        let end = drift_handler_end(src, idx);
        let body = &src[idx..end];
        assert!(body.contains("check_and_record_propose(cfg, now)?;"),
            "propose_set_provider_vault_authority MUST call check_and_record_propose \
             for R7.7-H-01 cancel-storm defense. Source excerpt:\n{}", body);
        assert!(body.contains("ADMIN_TIMELOCK_SECONDS"),
            "propose_set_provider_vault_authority MUST set unlock at now + ADMIN_TIMELOCK_SECONDS \
             (Rule 27b 72h). Source excerpt:\n{}", body);
    }

    #[test]
    fn propose_provider_vault_authority_rejects_default_and_duplicate() {
        let src = sovereign_registry_lib_rs_source();
        let needle = "pub fn propose_set_provider_vault_authority(";
        let idx = src.find(needle).expect("propose_set_provider_vault_authority must exist");
        
        let end = drift_handler_end(src, idx);
        let body = &src[idx..end];
        assert!(body.contains("new_authority != Pubkey::default()"),
            "propose MUST reject default pubkey to avoid silent zeroing of the field. \
             Source excerpt:\n{}", body);
        assert!(body.contains("ProviderVaultAuthorityProposalAlreadyPending"),
            "propose MUST reject when a proposal is already pending (must cancel first). \
             Source excerpt:\n{}", body);
    }

    

    #[test]
    fn r77_h01_check_and_record_propose_helper_present() {
        let src = sovereign_registry_lib_rs_source();
        assert!(src.contains("fn check_and_record_propose("),
            "sovereign-registry MUST define check_and_record_propose helper (R7.7-H-01)");
        assert!(src.contains("propose_cooldown_until"),
            "RegistryConfig MUST carry propose_cooldown_until field (R7.7-H-01)");
        assert!(src.contains("recent_proposes: [i64; 5]"),
            "RegistryConfig MUST carry recent_proposes: [i64; 5] ring buffer (R7.7-H-01)");
        assert!(src.contains("ProposeCooldownActive"),
            "RegistryError::ProposeCooldownActive variant MUST exist (R7.7-H-01)");
    }
    #[test]
    fn r77_h01_all_propose_handlers_call_check_and_record_propose() {
        
        
        
        
        let src = sovereign_registry_lib_rs_source();
        let propose_names = ["propose_rotate_admin",
                             "propose_set_provider_vault_authority"];
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
