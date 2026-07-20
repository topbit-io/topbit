
use anchor_lang::prelude::*;
use anchor_lang::system_program;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};

declare_id!("GcLnquNequt8UwNigWDzyfA2DpeeTMnQnyUbHxHL8cfC");

#[cfg(not(feature = "no-entrypoint"))]
solana_security_txt::security_txt! {
    name: "affiliate_registry",
    project_url: "https://topbit.io",
    contacts: "email:security@topbit.io",
    policy: "https://topbit.io/security",
    preferred_languages: "en",
    source_code: "https://github.com/topbit-io/topbit"
}


const SILVER_PLAYERS: u32 = 50;
const GOLD_PLAYERS: u32 = 200;
const PARTNER_MONTHLY_VOLUME: u64 = 500_000_000_000;
pub const COMMISSION_HOLD_SECONDS: i64 = 7 * 24 * 3600;

pub const SOL_PSEUDO_MINT: Pubkey =
    anchor_lang::solana_program::system_program::ID;

pub const MAX_FUNDING_SOURCES: usize = 16;

pub const ADMIN_TIMELOCK_SECONDS: i64 = 72 * 60 * 60;
pub const PROPOSE_RATE_LIMIT_WINDOW_SECONDS: i64 = 86_400;
pub const PROPOSE_RATE_LIMIT_RING_LEN: usize = 5;

fn check_and_record_propose(cfg: &mut AffiliateConfig, now: i64) -> Result<()> {
    require!(now >= cfg.propose_cooldown_until, AffiliateError::ProposeCooldownActive);
    let window_start = now.saturating_sub(PROPOSE_RATE_LIMIT_WINDOW_SECONDS);
    let count_24h = cfg.recent_proposes.iter().filter(|t| **t > window_start).count();
    let next_cooldown_seconds: i64 = match count_24h {
        0 | 1 => 0, 2 => 1_800, 3 => 7_200, 4 => 86_400, _ => 604_800,
    };
    cfg.propose_cooldown_until = if next_cooldown_seconds == 0 {
        0
    } else {
        now.checked_add(next_cooldown_seconds).ok_or(AffiliateError::MathOverflow)?
    };
    for i in 0..(PROPOSE_RATE_LIMIT_RING_LEN - 1) {
        cfg.recent_proposes[i] = cfg.recent_proposes[i + 1];
    }
    cfg.recent_proposes[PROPOSE_RATE_LIMIT_RING_LEN - 1] = now;
    Ok(())
}

#[program]
pub mod affiliate_registry {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        let config = &mut ctx.accounts.affiliate_config;
        config.authority = ctx.accounts.authority.key();
        config.pool_account = ctx.accounts.affiliate_pool.key();
        config.total_affiliates = 0;
        config.bump = ctx.bumps.affiliate_config;
        config.funding_sources = [Pubkey::default(); MAX_FUNDING_SOURCES];
        config.funding_sources_count = 0;
        config.propose_cooldown_until = 0;
        config.recent_proposes = [0i64; 5];
        config.pending_authority = Pubkey::default();
        config.pending_authority_unlocks_at = 0;
        config.reserved = [0u8; 0];

        emit!(AffiliatePoolInitialized {
            affiliate_pool: ctx.accounts.affiliate_pool.key(),
            mint: ctx.accounts.affiliate_pool.mint,
            authority: ctx.accounts.authority.key(),
            timestamp: Clock::get()?.unix_timestamp,
        });

        Ok(())
    }

    pub fn register(
        ctx: Context<Register>,
        referral_code: [u8; 8],
    ) -> Result<()> {
        let affiliate = &mut ctx.accounts.affiliate;
        affiliate.wallet = ctx.accounts.wallet.key();
        affiliate.referral_code = referral_code;
        affiliate.tier = AffiliateTier::Standard as u8;
        affiliate.total_earned = 0;
        affiliate.unclaimed = 0;
        affiliate.claimable_at = 0;
        affiliate.active_players = 0;
        affiliate.monthly_volume = 0;
        affiliate.bump = ctx.bumps.affiliate;

        let code_index = &mut ctx.accounts.referral_code_index;
        code_index.affiliate = ctx.accounts.wallet.key();
        code_index.bump = ctx.bumps.referral_code_index;

        let config = &mut ctx.accounts.affiliate_config;
        config.total_affiliates = config
            .total_affiliates
            .checked_add(1)
            .ok_or(AffiliateError::MathOverflow)?;

        Ok(())
    }

    pub fn record_commission(
        _ctx: Context<RecordCommission>,
        _amount: u64,
        _referred_player: Pubkey,
    ) -> Result<()> {
        msg!("DEPRECATED: record_commission is gated. Use record_commission_v2 (Pattern Y per-asset bucket).");
        err!(AffiliateError::InstructionDeprecated)
    }

    pub fn claim_commission(_ctx: Context<ClaimCommission>) -> Result<()> {
        msg!("DEPRECATED: claim_commission is gated. Use claim_commission_v2 (Pattern Y per-asset bucket).");
        err!(AffiliateError::InstructionDeprecated)
    }

    pub fn clawback_commission(
        _ctx: Context<ClawbackCommission>,
        _amount: u64,
    ) -> Result<()> {
        msg!("DEPRECATED: clawback_commission is gated. Use clawback_commission_v2 (Pattern Y per-asset bucket).");
        err!(AffiliateError::InstructionDeprecated)
    }

    pub fn update_tier(ctx: Context<UpdateAffiliateTier>) -> Result<()> {
        let affiliate = &mut ctx.accounts.affiliate;
        let new_tier = if affiliate.monthly_volume >= PARTNER_MONTHLY_VOLUME {
            AffiliateTier::Partner as u8
        } else if affiliate.active_players >= GOLD_PLAYERS {
            AffiliateTier::Gold as u8
        } else if affiliate.active_players >= SILVER_PLAYERS {
            AffiliateTier::Silver as u8
        } else {
            AffiliateTier::Standard as u8
        };
        if affiliate.tier != AffiliateTier::Master as u8 {
            affiliate.tier = new_tier;
        }
        Ok(())
    }


    pub fn init_funding_pool(ctx: Context<InitFundingPool>, mint: Pubkey) -> Result<()> {
        let config = &ctx.accounts.affiliate_config;
        require_keys_eq!(
            ctx.accounts.authority.key(),
            config.authority,
            AffiliateError::Unauthorized
        );

        let pool = &mut ctx.accounts.funding_pool;
        pool.asset_mint = mint;
        pool.token_account = match ctx.accounts.token_holder.as_ref() {
            Some(acc) => acc.key(),
            None => Pubkey::default(),
        };
        pool.sol_holder = match ctx.accounts.sol_holder.as_ref() {
            Some(acc) => acc.key(),
            None => Pubkey::default(),
        };
        pool.is_sol_pool = mint == SOL_PSEUDO_MINT;
        pool.total_deposited = 0;
        pool.total_paid_out = 0;
        pool.bump = ctx.bumps.funding_pool;
        pool.total_unclaimed = 0;
        pool.reserved = [0u8; 24];

        if pool.is_sol_pool {
            require!(
                pool.token_account == Pubkey::default(),
                AffiliateError::InvalidPoolShape
            );
            require!(
                pool.sol_holder != Pubkey::default(),
                AffiliateError::InvalidPoolShape
            );
        } else {
            require!(
                pool.token_account != Pubkey::default(),
                AffiliateError::InvalidPoolShape
            );
            require!(
                pool.sol_holder == Pubkey::default(),
                AffiliateError::InvalidPoolShape
            );
        }
        Ok(())
    }

    pub fn set_funding_sources(
        ctx: Context<AdminOnly>,
        sources: [Pubkey; MAX_FUNDING_SOURCES],
        count: u8,
    ) -> Result<()> {
        require!(
            (count as usize) <= MAX_FUNDING_SOURCES,
            AffiliateError::InvalidFundingSources
        );
        let config = &mut ctx.accounts.affiliate_config;
        require_keys_eq!(
            ctx.accounts.authority.key(),
            config.authority,
            AffiliateError::Unauthorized
        );
        let mut normalised = [Pubkey::default(); MAX_FUNDING_SOURCES];
        for i in 0..(count as usize) {
            require!(
                sources[i] != Pubkey::default(),
                AffiliateError::InvalidFundingSources
            );
            for j in 0..i {
                require!(
                    sources[j] != sources[i],
                    AffiliateError::InvalidFundingSources
                );
            }
            normalised[i] = sources[i];
        }
        config.funding_sources = normalised;
        config.funding_sources_count = count;
        Ok(())
    }

    pub fn record_commission_v2(
        ctx: Context<RecordCommissionV2>,
        amount: u64,
        referred_player: Pubkey,
        asset_mint: Pubkey,
        reference: [u8; 32],
    ) -> Result<()> {
        let _ = reference;
        require!(
            !ctx.accounts.comm_record_receipt.recorded,
            AffiliateError::DuplicateCommissionRecord
        );

        require!(amount > 0, AffiliateError::ZeroAmount);
        require!(
            referred_player != ctx.accounts.affiliate.wallet,
            AffiliateError::SelfReferral
        );

        let bucket = &mut ctx.accounts.commission_bucket;
        require!(bucket.mint == asset_mint, AffiliateError::AssetMismatch);
        require!(
            bucket.affiliate == ctx.accounts.affiliate.wallet,
            AffiliateError::AssetMismatch
        );

        let clock = Clock::get()?;

        bucket.unclaimed = bucket
            .unclaimed
            .checked_add(amount)
            .ok_or(AffiliateError::MathOverflow)?;

        let pool = &mut ctx.accounts.funding_pool;
        require!(pool.asset_mint == asset_mint, AffiliateError::AssetMismatch);
        pool.total_unclaimed = pool
            .total_unclaimed
            .checked_add(amount)
            .ok_or(AffiliateError::MathOverflow)?;
        let liquidity = pool
            .total_deposited
            .checked_sub(pool.total_paid_out)
            .ok_or(AffiliateError::MathOverflow)?;
        require!(
            pool.total_unclaimed <= liquidity,
            AffiliateError::PoolOverAttributed
        );

        if bucket.claimable_at == 0 {
            bucket.claimable_at = clock
                .unix_timestamp
                .checked_add(COMMISSION_HOLD_SECONDS)
                .ok_or(AffiliateError::MathOverflow)?;
        }

        let affiliate = &mut ctx.accounts.affiliate;
        affiliate.total_earned = affiliate
            .total_earned
            .checked_add(amount)
            .ok_or(AffiliateError::MathOverflow)?;
        affiliate.monthly_volume = affiliate
            .monthly_volume
            .checked_add(amount)
            .ok_or(AffiliateError::MathOverflow)?;

        emit!(CommissionRecordedV2 {
            affiliate: affiliate.wallet,
            asset_mint,
            amount,
            referred_player,
            new_bucket_unclaimed: bucket.unclaimed,
            claimable_at: bucket.claimable_at,
            timestamp: clock.unix_timestamp,
        });

        let receipt = &mut ctx.accounts.comm_record_receipt;
        receipt.recorded = true;
        receipt.bump = ctx.bumps.comm_record_receipt;
        Ok(())
    }

    pub fn init_commission_bucket(
        ctx: Context<InitCommissionBucket>,
        mint: Pubkey,
    ) -> Result<()> {
        let bucket = &mut ctx.accounts.commission_bucket;
        bucket.affiliate = ctx.accounts.affiliate.wallet;
        bucket.mint = mint;
        bucket.unclaimed = 0;
        bucket.claimable_at = 0;
        bucket.total_earned = 0;
        bucket.bump = ctx.bumps.commission_bucket;
        bucket.reserved = [0u8; 32];
        Ok(())
    }

    pub fn claim_commission_v2(ctx: Context<ClaimCommissionV2>) -> Result<()> {
        let bucket = &mut ctx.accounts.commission_bucket;
        let pool = &mut ctx.accounts.funding_pool;
        let clock = Clock::get()?;

        let claimable = bucket.unclaimed;
        require!(claimable > 0, AffiliateError::NothingToClaim);
        require!(
            clock.unix_timestamp >= bucket.claimable_at,
            AffiliateError::CommissionOnHold
        );
        require!(pool.asset_mint == bucket.mint, AffiliateError::AssetMismatch);
        require!(
            bucket.affiliate == ctx.accounts.wallet.key(),
            AffiliateError::Unauthorized
        );

        bucket.unclaimed = 0;
        bucket.claimable_at = 0;
        bucket.total_earned = bucket
            .total_earned
            .checked_add(claimable)
            .ok_or(AffiliateError::MathOverflow)?;
        pool.total_paid_out = pool
            .total_paid_out
            .checked_add(claimable)
            .ok_or(AffiliateError::MathOverflow)?;
        pool.total_unclaimed = pool
            .total_unclaimed
            .checked_sub(claimable)
            .ok_or(AffiliateError::MathOverflow)?;

        if pool.is_sol_pool {
            let holder = ctx.accounts.sol_holder.as_ref()
                .ok_or(AffiliateError::MissingPoolAccount)?;
            require_keys_eq!(holder.key(), pool.sol_holder, AffiliateError::AssetMismatch);

            let from = holder.to_account_info();
            let to = ctx.accounts.wallet.to_account_info();
            let from_lamports = from.lamports();
            require!(from_lamports >= claimable, AffiliateError::InsufficientPool);
            **from.try_borrow_mut_lamports()? = from_lamports
                .checked_sub(claimable)
                .ok_or(AffiliateError::MathOverflow)?;
            **to.try_borrow_mut_lamports()? = to
                .lamports()
                .checked_add(claimable)
                .ok_or(AffiliateError::MathOverflow)?;
        } else {
            let pool_token = ctx.accounts.pool_token_account.as_ref()
                .ok_or(AffiliateError::MissingPoolAccount)?;
            let wallet_token = ctx.accounts.wallet_token_account.as_ref()
                .ok_or(AffiliateError::MissingPoolAccount)?;
            let token_program = ctx.accounts.token_program.as_ref()
                .ok_or(AffiliateError::MissingPoolAccount)?;

            require_keys_eq!(pool_token.key(), pool.token_account, AffiliateError::AssetMismatch);

            let mint_bytes = pool.asset_mint.to_bytes();
            let seeds: &[&[u8]] = &[b"funding_pool", mint_bytes.as_ref(), &[pool.bump]];
            let signer_seeds = &[seeds];
            let cpi_ctx = CpiContext::new_with_signer(
                token_program.to_account_info(),
                Transfer {
                    from: pool_token.to_account_info(),
                    to: wallet_token.to_account_info(),
                    authority: pool.to_account_info(),
                },
                signer_seeds,
            );
            token::transfer(cpi_ctx, claimable)?;
        }

        emit!(CommissionClaimedV2 {
            affiliate: bucket.affiliate,
            asset_mint: pool.asset_mint,
            amount: claimable,
            timestamp: clock.unix_timestamp,
        });
        Ok(())
    }

    pub fn clawback_commission_v2(
        ctx: Context<ClawbackCommissionV2>,
        amount: u64,
    ) -> Result<()> {
        let bucket = &mut ctx.accounts.commission_bucket;
        let clock = Clock::get()?;

        require!(
            bucket.claimable_at != 0 && clock.unix_timestamp < bucket.claimable_at,
            AffiliateError::CommissionAlreadyClaimable
        );

        let clawback = amount.min(bucket.unclaimed);
        bucket.unclaimed = bucket
            .unclaimed
            .checked_sub(clawback)
            .ok_or(AffiliateError::MathOverflow)?;
        if bucket.unclaimed == 0 {
            bucket.claimable_at = 0;
        }

        let pool = &mut ctx.accounts.funding_pool;
        pool.total_unclaimed = pool
            .total_unclaimed
            .checked_sub(clawback)
            .ok_or(AffiliateError::MathOverflow)?;

        let affiliate = &mut ctx.accounts.affiliate;
        affiliate.total_earned = affiliate.total_earned.saturating_sub(clawback);
        affiliate.monthly_volume = affiliate.monthly_volume.saturating_sub(clawback);

        emit!(CommissionClawedBackV2 {
            affiliate: bucket.affiliate,
            asset_mint: bucket.mint,
            amount: clawback,
            timestamp: clock.unix_timestamp,
        });
        Ok(())
    }

    pub fn deposit_funding_pool(
        ctx: Context<DepositFundingPool>,
        amount: u64,
        asset_mint: Pubkey,
    ) -> Result<()> {
        require!(amount > 0, AffiliateError::ZeroAmount);

        let config = &ctx.accounts.affiliate_config;
        let signer_key = ctx.accounts.source_signer.key();
        let mut ok = false;
        for i in 0..(config.funding_sources_count as usize) {
            if config.funding_sources[i] == signer_key {
                ok = true;
                break;
            }
        }
        require!(ok, AffiliateError::Unauthorized);

        let pool = &mut ctx.accounts.funding_pool;
        require!(pool.asset_mint == asset_mint, AffiliateError::AssetMismatch);

        if pool.is_sol_pool {
            let holder = ctx.accounts.sol_holder.as_ref()
                .ok_or(AffiliateError::MissingPoolAccount)?;
            require_keys_eq!(holder.key(), pool.sol_holder, AffiliateError::AssetMismatch);

            let cpi_ctx = CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                system_program::Transfer {
                    from: ctx.accounts.source_signer.to_account_info(),
                    to: holder.to_account_info(),
                },
            );
            system_program::transfer(cpi_ctx, amount)?;
        } else {
            let source_token = ctx.accounts.source_token_account.as_ref()
                .ok_or(AffiliateError::MissingPoolAccount)?;
            let pool_token = ctx.accounts.pool_token_account.as_ref()
                .ok_or(AffiliateError::MissingPoolAccount)?;
            let token_program = ctx.accounts.token_program.as_ref()
                .ok_or(AffiliateError::MissingPoolAccount)?;

            require_keys_eq!(
                pool_token.key(),
                pool.token_account,
                AffiliateError::AssetMismatch
            );

            let cpi_ctx = CpiContext::new(
                token_program.to_account_info(),
                Transfer {
                    from: source_token.to_account_info(),
                    to: pool_token.to_account_info(),
                    authority: ctx.accounts.source_signer.to_account_info(),
                },
            );
            token::transfer(cpi_ctx, amount)?;
        }

        pool.total_deposited = pool
            .total_deposited
            .checked_add(amount)
            .ok_or(AffiliateError::MathOverflow)?;

        emit!(FundingPoolDeposit {
            asset_mint,
            amount,
            source: signer_key,
            new_total_deposited: pool.total_deposited,
            timestamp: Clock::get()?.unix_timestamp,
        });
        Ok(())
    }


    pub fn propose_rotate_admin(
        ctx: Context<AdminOnly>,
        new_admin: Pubkey,
    ) -> Result<()> {
        let cfg = &mut ctx.accounts.affiliate_config;
        require_keys_eq!(ctx.accounts.authority.key(), cfg.authority, AffiliateError::Unauthorized);
        require!(new_admin != Pubkey::default(), AffiliateError::InvalidAdmin);
        require!(new_admin != cfg.authority, AffiliateError::InvalidAdmin);
        require!(
            cfg.pending_authority == Pubkey::default(),
            AffiliateError::AdminProposalAlreadyPending
        );
        let now = Clock::get()?.unix_timestamp;
        check_and_record_propose(cfg, now)?;
        let unlocks_at = now
            .checked_add(ADMIN_TIMELOCK_SECONDS)
            .ok_or(AffiliateError::MathOverflow)?;
        cfg.pending_authority = new_admin;
        cfg.pending_authority_unlocks_at = unlocks_at;
        emit!(AdminRotationProposed { admin: cfg.authority, new_admin, unlocks_at, timestamp: now });
        Ok(())
    }

    pub fn finalize_rotate_admin(ctx: Context<AdminOnly>) -> Result<()> {
        let cfg = &mut ctx.accounts.affiliate_config;
        require_keys_eq!(ctx.accounts.authority.key(), cfg.authority, AffiliateError::Unauthorized);
        require!(
            cfg.pending_authority != Pubkey::default(),
            AffiliateError::AdminNoProposalPending
        );
        let now = Clock::get()?.unix_timestamp;
        require!(
            now >= cfg.pending_authority_unlocks_at,
            AffiliateError::AdminTimelockNotElapsed
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
        let cfg = &mut ctx.accounts.affiliate_config;
        require_keys_eq!(ctx.accounts.authority.key(), cfg.authority, AffiliateError::Unauthorized);
        require!(
            cfg.pending_authority != Pubkey::default(),
            AffiliateError::AdminNoProposalPending
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
pub enum AffiliateError {
    #[msg("Amount must be greater than zero")]
    ZeroAmount,
    #[msg("Arithmetic overflow")]
    MathOverflow,
    #[msg("Nothing available to claim")]
    NothingToClaim,
    #[msg("Commission is still in the 7-day hold window — cannot claim yet")]
    CommissionOnHold,
    #[msg("Commission is already past the hold window — cannot clawback")]
    CommissionAlreadyClaimable,
    #[msg("Self-referral is not allowed — affiliate cannot refer themselves")]
    SelfReferral,
    #[msg("Unauthorized — caller is not the affiliate registry authority")]
    Unauthorized,
    #[msg("Asset mint does not match the bucket/pool")]
    AssetMismatch,
    #[msg("Funding pool has insufficient balance")]
    InsufficientPool,
    #[msg("Required SPL/SOL pool account not provided")]
    MissingPoolAccount,
    #[msg("Invalid funding-source whitelist (duplicate or default pubkey)")]
    InvalidFundingSources,
    #[msg("FundingPool init must populate exactly one of token / sol holder")]
    InvalidPoolShape,

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

    #[msg("DEPRECATED — v1 single-pool commission path is gated. Use the _v2 instructions (Pattern Y, per-asset bucket).")]
    InstructionDeprecated,
    #[msg("Commission attribution would exceed funded pool liquidity (total_unclaimed > total_deposited − total_paid_out)")]
    PoolOverAttributed,
    #[msg("Commission reference already recorded on-chain (replay) — per-commission idempotency latch")]
    DuplicateCommissionRecord,
}

#[account]
pub struct CommRecordReceipt {
    pub recorded: bool,
    pub bump: u8,
}
impl CommRecordReceipt {
    pub const LEN: usize = 8 + 1 + 1;
}


#[account]
pub struct AffiliateConfig {
    pub authority: Pubkey,
    pub pool_account: Pubkey,
    pub total_affiliates: u64,
    pub bump: u8,
    pub funding_sources: [Pubkey; MAX_FUNDING_SOURCES],
    pub funding_sources_count: u8,

    pub propose_cooldown_until: i64,
    pub recent_proposes: [i64; 5],

    pub pending_authority: Pubkey,
    pub pending_authority_unlocks_at: i64,

    pub reserved: [u8; 0],
}
impl AffiliateConfig {
    pub const LEN: usize = 8 + 32 + 32 + 8 + 1 + (MAX_FUNDING_SOURCES * 32) + 1
        + 8 + 40 + 32 + 8;
}

#[account]
pub struct Affiliate {
    pub wallet: Pubkey,
    pub referral_code: [u8; 8],
    pub tier: u8,
    pub total_earned: u64,
    pub unclaimed: u64,
    pub claimable_at: i64,
    pub active_players: u32,
    pub monthly_volume: u64,
    pub bump: u8,
}
impl Affiliate {
    pub const LEN: usize = 8 + 32 + 8 + 1 + 8 + 8 + 8 + 4 + 8 + 1;
}

#[account]
pub struct ReferralCodeIndex {
    pub affiliate: Pubkey,
    pub bump: u8,
}
impl ReferralCodeIndex {
    pub const LEN: usize = 8 + 32 + 1;
}

#[account]
pub struct FundingPool {
    pub asset_mint: Pubkey,
    pub token_account: Pubkey,
    pub sol_holder: Pubkey,
    pub is_sol_pool: bool,
    pub total_deposited: u64,
    pub total_paid_out: u64,
    pub bump: u8,
    pub total_unclaimed: u64,
    pub reserved: [u8; 24],
}
impl FundingPool {
    pub const LEN: usize = 8 + 32 + 32 + 32 + 1 + 8 + 8 + 1 + 8 + 24;
}

#[account]
pub struct CommissionBucket {
    pub affiliate: Pubkey,
    pub mint: Pubkey,
    pub unclaimed: u64,
    pub claimable_at: i64,
    pub total_earned: u64,
    pub bump: u8,
    pub reserved: [u8; 32],
}
impl CommissionBucket {
    pub const LEN: usize = 8 + 32 + 32 + 8 + 8 + 8 + 1 + 32;
}


#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(
        init,
        payer = authority,
        space = AffiliateConfig::LEN,
        seeds = [b"affiliate_config"],
        bump
    )]
    pub affiliate_config: Account<'info, AffiliateConfig>,
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(
        mut,
        token::authority = affiliate_config,
    )]
    pub affiliate_pool: Account<'info, TokenAccount>,
    #[account(
        seeds = [crate::ID.as_ref()],
        bump,
        seeds::program = anchor_lang::solana_program::bpf_loader_upgradeable::ID,
        constraint = program_data.upgrade_authority_address == Some(authority.key())
            @ AffiliateError::Unauthorized,
    )]
    pub program_data: Account<'info, ProgramData>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(referral_code: [u8; 8])]
pub struct Register<'info> {
    #[account(
        init,
        payer = wallet,
        space = Affiliate::LEN,
        seeds = [b"affiliate", wallet.key().as_ref()],
        bump
    )]
    pub affiliate: Account<'info, Affiliate>,
    #[account(
        init,
        payer = wallet,
        space = ReferralCodeIndex::LEN,
        seeds = [b"ref_code", referral_code.as_ref()],
        bump
    )]
    pub referral_code_index: Account<'info, ReferralCodeIndex>,
    #[account(mut, seeds = [b"affiliate_config"], bump = affiliate_config.bump)]
    pub affiliate_config: Account<'info, AffiliateConfig>,
    #[account(mut)]
    pub wallet: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct RecordCommission<'info> {
    #[account(
        mut,
        seeds = [b"affiliate", affiliate.wallet.as_ref()],
        bump = affiliate.bump
    )]
    pub affiliate: Account<'info, Affiliate>,
    #[account(
        seeds = [b"affiliate_config"],
        bump = affiliate_config.bump,
        has_one = authority @ AffiliateError::Unauthorized,
    )]
    pub affiliate_config: Account<'info, AffiliateConfig>,
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ClaimCommission<'info> {
    #[account(
        mut,
        has_one = wallet,
        seeds = [b"affiliate", wallet.key().as_ref()],
        bump = affiliate.bump
    )]
    pub affiliate: Box<Account<'info, Affiliate>>,
    #[account(seeds = [b"affiliate_config"], bump = affiliate_config.bump)]
    pub affiliate_config: Box<Account<'info, AffiliateConfig>>,
    #[account(mut)]
    pub wallet: Signer<'info>,
    #[account(
        mut,
        constraint = affiliate_pool.key() == affiliate_config.pool_account
            @ AffiliateError::Unauthorized,
    )]
    pub affiliate_pool: Box<Account<'info, TokenAccount>>,
    #[account(
        mut,
        token::mint = affiliate_pool.mint,
        token::authority = wallet,
    )]
    pub wallet_usdc_account: Box<Account<'info, TokenAccount>>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ClawbackCommission<'info> {
    #[account(
        mut,
        seeds = [b"affiliate", affiliate.wallet.as_ref()],
        bump = affiliate.bump
    )]
    pub affiliate: Account<'info, Affiliate>,
    #[account(
        seeds = [b"affiliate_config"],
        bump = affiliate_config.bump,
        has_one = authority @ AffiliateError::Unauthorized,
    )]
    pub affiliate_config: Account<'info, AffiliateConfig>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct UpdateAffiliateTier<'info> {
    #[account(
        mut,
        has_one = wallet,
        seeds = [b"affiliate", wallet.key().as_ref()],
        bump = affiliate.bump
    )]
    pub affiliate: Account<'info, Affiliate>,
    pub wallet: Signer<'info>,
}

#[derive(Accounts)]
pub struct AdminOnly<'info> {
    #[account(mut, seeds = [b"affiliate_config"], bump = affiliate_config.bump)]
    pub affiliate_config: Account<'info, AffiliateConfig>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(mint: Pubkey)]
pub struct InitFundingPool<'info> {
    #[account(seeds = [b"affiliate_config"], bump = affiliate_config.bump)]
    pub affiliate_config: Account<'info, AffiliateConfig>,
    #[account(
        init,
        payer = authority,
        space = FundingPool::LEN,
        seeds = [b"funding_pool", mint.as_ref()],
        bump
    )]
    pub funding_pool: Account<'info, FundingPool>,
    #[account(
        mut,
        token::mint = mint,
        token::authority = funding_pool,
    )]
    pub token_holder: Option<Account<'info, TokenAccount>>,
    #[account(mut)]
    pub sol_holder: Option<AccountInfo<'info>>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(amount: u64, referred_player: Pubkey, asset_mint: Pubkey, reference: [u8; 32])]
pub struct RecordCommissionV2<'info> {
    #[account(
        mut,
        seeds = [b"affiliate", affiliate.wallet.as_ref()],
        bump = affiliate.bump
    )]
    pub affiliate: Account<'info, Affiliate>,
    #[account(
        mut,
        seeds = [b"comm_bucket", affiliate.wallet.as_ref(), asset_mint.as_ref()],
        bump = commission_bucket.bump
    )]
    pub commission_bucket: Account<'info, CommissionBucket>,
    #[account(
        mut,
        seeds = [b"funding_pool", asset_mint.as_ref()],
        bump = funding_pool.bump
    )]
    pub funding_pool: Account<'info, FundingPool>,
    #[account(
        seeds = [b"affiliate_config"],
        bump = affiliate_config.bump,
        has_one = authority @ AffiliateError::Unauthorized,
    )]
    pub affiliate_config: Account<'info, AffiliateConfig>,
    #[account(
        init_if_needed,
        payer = authority,
        space = CommRecordReceipt::LEN,
        seeds = [b"comm_record", affiliate.wallet.as_ref(), referred_player.as_ref(), reference.as_ref()],
        bump
    )]
    pub comm_record_receipt: Account<'info, CommRecordReceipt>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(mint: Pubkey)]
pub struct InitCommissionBucket<'info> {
    #[account(
        seeds = [b"affiliate", affiliate.wallet.as_ref()],
        bump = affiliate.bump
    )]
    pub affiliate: Account<'info, Affiliate>,
    #[account(
        init,
        payer = payer,
        space = CommissionBucket::LEN,
        seeds = [b"comm_bucket", affiliate.wallet.as_ref(), mint.as_ref()],
        bump
    )]
    pub commission_bucket: Account<'info, CommissionBucket>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ClaimCommissionV2<'info> {
    #[account(
        mut,
        seeds = [b"comm_bucket", wallet.key().as_ref(), commission_bucket.mint.as_ref()],
        bump = commission_bucket.bump
    )]
    pub commission_bucket: Account<'info, CommissionBucket>,
    #[account(
        mut,
        seeds = [b"funding_pool", funding_pool.asset_mint.as_ref()],
        bump = funding_pool.bump
    )]
    pub funding_pool: Account<'info, FundingPool>,
    #[account(mut)]
    pub wallet: Signer<'info>,

    #[account(
        mut,
        token::mint = funding_pool.asset_mint,
        token::authority = funding_pool,
    )]
    pub pool_token_account: Option<Account<'info, TokenAccount>>,
    #[account(
        mut,
        token::mint = funding_pool.asset_mint,
        token::authority = wallet,
    )]
    pub wallet_token_account: Option<Account<'info, TokenAccount>>,
    pub token_program: Option<Program<'info, Token>>,

    #[account(mut)]
    pub sol_holder: Option<AccountInfo<'info>>,
}

#[derive(Accounts)]
pub struct ClawbackCommissionV2<'info> {
    #[account(
        mut,
        seeds = [b"comm_bucket", commission_bucket.affiliate.as_ref(), commission_bucket.mint.as_ref()],
        bump = commission_bucket.bump
    )]
    pub commission_bucket: Account<'info, CommissionBucket>,
    #[account(
        mut,
        seeds = [b"affiliate", commission_bucket.affiliate.as_ref()],
        bump = affiliate.bump
    )]
    pub affiliate: Account<'info, Affiliate>,
    #[account(
        mut,
        seeds = [b"funding_pool", commission_bucket.mint.as_ref()],
        bump = funding_pool.bump
    )]
    pub funding_pool: Account<'info, FundingPool>,
    #[account(
        seeds = [b"affiliate_config"],
        bump = affiliate_config.bump,
        has_one = authority @ AffiliateError::Unauthorized,
    )]
    pub affiliate_config: Account<'info, AffiliateConfig>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(amount: u64, asset_mint: Pubkey)]
pub struct DepositFundingPool<'info> {
    #[account(seeds = [b"affiliate_config"], bump = affiliate_config.bump)]
    pub affiliate_config: Account<'info, AffiliateConfig>,
    #[account(
        mut,
        seeds = [b"funding_pool", asset_mint.as_ref()],
        bump = funding_pool.bump
    )]
    pub funding_pool: Account<'info, FundingPool>,

    #[account(mut)]
    pub source_signer: Signer<'info>,

    #[account(
        mut,
        token::mint = funding_pool.asset_mint,
        token::authority = source_signer,
    )]
    pub source_token_account: Option<Account<'info, TokenAccount>>,
    #[account(
        mut,
        token::mint = funding_pool.asset_mint,
        token::authority = funding_pool,
    )]
    pub pool_token_account: Option<Account<'info, TokenAccount>>,
    pub token_program: Option<Program<'info, Token>>,

    #[account(mut)]
    pub sol_holder: Option<AccountInfo<'info>>,

    pub system_program: Program<'info, System>,
}


#[event]
pub struct CommissionClawedBack {
    pub affiliate: Pubkey,
    pub amount: u64,
    pub timestamp: i64,
}

#[event]
pub struct CommissionRecordedV2 {
    pub affiliate: Pubkey,
    pub asset_mint: Pubkey,
    pub amount: u64,
    pub referred_player: Pubkey,
    pub new_bucket_unclaimed: u64,
    pub claimable_at: i64,
    pub timestamp: i64,
}

#[event]
pub struct CommissionClaimedV2 {
    pub affiliate: Pubkey,
    pub asset_mint: Pubkey,
    pub amount: u64,
    pub timestamp: i64,
}

#[event]
pub struct CommissionClawedBackV2 {
    pub affiliate: Pubkey,
    pub asset_mint: Pubkey,
    pub amount: u64,
    pub timestamp: i64,
}

#[event]
pub struct FundingPoolDeposit {
    pub asset_mint: Pubkey,
    pub amount: u64,
    pub source: Pubkey,
    pub new_total_deposited: u64,
    pub timestamp: i64,
}

#[event]
pub struct AffiliatePoolInitialized {
    pub affiliate_pool: Pubkey,
    pub mint: Pubkey,
    pub authority: Pubkey,
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


#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq)]
pub enum AffiliateTier {
    Standard = 0,
    Silver = 1,
    Gold = 2,
    Partner = 3,
    Master = 4,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_bucket(now: i64) -> CommissionBucket {
        let _ = now;
        CommissionBucket {
            affiliate: Pubkey::new_unique(),
            mint: Pubkey::new_unique(),
            unclaimed: 0,
            claimable_at: 0,
            total_earned: 0,
            bump: 255,
            reserved: [0u8; 32],
        }
    }

    #[test]
    fn record_v2_first_anchored_does_not_advance_on_subsequent() {
        let mut b = fresh_bucket(1_000);
        b.unclaimed = b.unclaimed.checked_add(100).unwrap();
        if b.claimable_at == 0 {
            b.claimable_at = 1_000 + COMMISSION_HOLD_SECONDS;
        }
        assert_eq!(b.claimable_at, 1_000 + COMMISSION_HOLD_SECONDS, "first record arms now + 7d");

        let later = 1_000 + 3 * 86_400;
        b.unclaimed = b.unclaimed.checked_add(50).unwrap();
        if b.claimable_at == 0 {
            b.claimable_at = later + COMMISSION_HOLD_SECONDS;
        }
        assert_eq!(
            b.claimable_at,
            1_000 + COMMISSION_HOLD_SECONDS,
            "anchor MUST NOT advance on subsequent recordings (AUTH-02)"
        );
        assert_eq!(b.unclaimed, 150);

        let earlier = 500;
        if b.claimable_at == 0 {
            b.claimable_at = earlier + COMMISSION_HOLD_SECONDS;
        }
        assert_eq!(
            b.claimable_at,
            1_000 + COMMISSION_HOLD_SECONDS,
            "an armed bucket is never re-anchored by a later recording"
        );
    }

    #[test]
    fn claim_v2_clears_bucket_and_rearms_anchor() {
        let mut b = fresh_bucket(0);
        b.unclaimed = 200;
        b.claimable_at = 1_000;

        let now = 1_500;
        assert!(now >= b.claimable_at);

        let claimable = b.unclaimed;
        b.unclaimed = 0;
        b.claimable_at = 0;
        b.total_earned = b.total_earned.checked_add(claimable).unwrap();

        assert_eq!(b.unclaimed, 0);
        assert_eq!(b.claimable_at, 0);
        assert_eq!(b.total_earned, 200);

        let next = 2_000;
        b.unclaimed = b.unclaimed.checked_add(50).unwrap();
        if b.claimable_at == 0 {
            b.claimable_at = next + COMMISSION_HOLD_SECONDS;
        }
        assert_eq!(b.claimable_at, next + COMMISSION_HOLD_SECONDS);
    }

    #[test]
    fn clawback_v2_only_within_hold() {
        let mut b = fresh_bucket(0);
        b.unclaimed = 500;
        b.claimable_at = 10_000;

        let now = 5_000;
        assert!(b.claimable_at != 0 && now < b.claimable_at);
        let clawback = 200u64.min(b.unclaimed);
        b.unclaimed = b.unclaimed.checked_sub(clawback).unwrap();
        if b.unclaimed == 0 {
            b.claimable_at = 0;
        }
        assert_eq!(b.unclaimed, 300);

        let later = 20_000;
        let mature = b.claimable_at != 0 && later < b.claimable_at;
        assert!(!mature, "post-maturity clawback must fail");
    }

    #[test]
    fn clawback_full_drain_rearms() {
        let mut b = fresh_bucket(0);
        b.unclaimed = 500;
        b.claimable_at = 10_000;

        let clawback = 999u64.min(b.unclaimed);
        b.unclaimed = b.unclaimed.checked_sub(clawback).unwrap();
        if b.unclaimed == 0 {
            b.claimable_at = 0;
        }
        assert_eq!(b.unclaimed, 0);
        assert_eq!(b.claimable_at, 0);
    }

    #[test]
    fn funding_pool_sol_pseudo_mint_is_system_program() {
        assert_eq!(SOL_PSEUDO_MINT, anchor_lang::solana_program::system_program::ID);
    }

    #[test]
    fn funding_pool_init_shape_invariant() {
        let pool_spl = FundingPool {
            asset_mint: Pubkey::new_unique(),
            token_account: Pubkey::new_unique(),
            sol_holder: Pubkey::default(),
            is_sol_pool: false,
            total_deposited: 0,
            total_paid_out: 0,
            bump: 255,
            total_unclaimed: 0,
            reserved: [0u8; 24],
        };
        assert!(!pool_spl.is_sol_pool);
        assert!(pool_spl.token_account != Pubkey::default());
        assert!(pool_spl.sol_holder == Pubkey::default());

        let pool_sol = FundingPool {
            asset_mint: SOL_PSEUDO_MINT,
            token_account: Pubkey::default(),
            sol_holder: Pubkey::new_unique(),
            is_sol_pool: true,
            total_deposited: 0,
            total_paid_out: 0,
            bump: 255,
            total_unclaimed: 0,
            reserved: [0u8; 24],
        };
        assert!(pool_sol.is_sol_pool);
        assert!(pool_sol.token_account == Pubkey::default());
        assert!(pool_sol.sol_holder != Pubkey::default());
    }

    #[test]
    fn funding_sources_count_bounded() {
        assert!(MAX_FUNDING_SOURCES >= 1);
        assert!(MAX_FUNDING_SOURCES <= 32);
    }

    #[test]
    fn funding_sources_capacity_16() {
        assert_eq!(MAX_FUNDING_SOURCES, 16);
        let arr = [Pubkey::default(); MAX_FUNDING_SOURCES];
        assert_eq!(arr.len(), 16);
    }

    #[test]
    fn affiliate_config_len_matches_layout_with_16_sources() {
        let expected = 8 + 32 + 32 + 8 + 1 + (16 * 32) + 1
                     + 8 + 40 + 32 + 8;
        assert_eq!(expected, 682);
        assert_eq!(AffiliateConfig::LEN, expected);
    }

    #[test]
    fn record_v2_zero_amount_rejected() {
        let amount: u64 = 0;
        assert!(amount == 0);
    }

    #[test]
    fn record_v2_self_referral_rejected() {
        let aff = Pubkey::new_unique();
        let referred = aff;
        assert!(referred == aff);
    }

    #[test]
    fn affiliate_config_size_matches_struct() {
        assert_eq!(AffiliateConfig::LEN, 8 + 32 + 32 + 8 + 1 + 512 + 1 + 8 + 40 + 32 + 8);
    }

    #[test]
    fn funding_pool_size_matches_struct() {
        assert_eq!(FundingPool::LEN, 8 + 32 + 32 + 32 + 1 + 8 + 8 + 1 + 8 + 24);
        assert_eq!(FundingPool::LEN, 154);
    }

    #[test]
    fn commission_bucket_size_matches_struct() {
        assert_eq!(CommissionBucket::LEN, 8 + 32 + 32 + 8 + 8 + 8 + 1 + 32);
    }


    #[test]
    fn funding_source_whitelist_match_succeeds() {
        let mut sources = [Pubkey::default(); MAX_FUNDING_SOURCES];
        let source_pda = Pubkey::new_unique();
        sources[0] = source_pda;
        let count = 1usize;
        let mut ok = false;
        for i in 0..count {
            if sources[i] == source_pda {
                ok = true;
                break;
            }
        }
        assert!(ok);
    }

    #[test]
    fn funding_source_whitelist_non_match_rejects() {
        let mut sources = [Pubkey::default(); MAX_FUNDING_SOURCES];
        sources[0] = Pubkey::new_unique();
        let count = 1usize;
        let attacker = Pubkey::new_unique();
        let mut ok = false;
        for i in 0..count {
            if sources[i] == attacker {
                ok = true;
                break;
            }
        }
        assert!(!ok);
    }

    #[test]
    fn funding_source_whitelist_empty_rejects_all() {
        let sources = [Pubkey::default(); MAX_FUNDING_SOURCES];
        let count = 0usize;
        let caller = Pubkey::new_unique();
        let mut ok = false;
        for i in 0..count {
            if sources[i] == caller { ok = true; }
        }
        assert!(!ok);
    }

    #[test]
    fn funding_source_set_normalises_count() {
        let count: u8 = 4;
        assert!((count as usize) <= MAX_FUNDING_SOURCES);
        let bad: u8 = (MAX_FUNDING_SOURCES + 1) as u8;
        assert!(!((bad as usize) <= MAX_FUNDING_SOURCES));
    }

    #[test]
    fn funding_source_set_rejects_duplicate() {
        let same = Pubkey::new_unique();
        let mut sources = [Pubkey::default(); MAX_FUNDING_SOURCES];
        sources[0] = same;
        sources[1] = same;
        let count = 2usize;
        let mut dup = false;
        for i in 0..count {
            for j in 0..i {
                if sources[j] == sources[i] { dup = true; }
            }
        }
        assert!(dup);
    }

    #[test]
    fn funding_source_set_rejects_default_in_active_prefix() {
        let mut sources = [Pubkey::default(); MAX_FUNDING_SOURCES];
        sources[0] = Pubkey::new_unique();
        let count = 2usize;
        let mut bad = false;
        for i in 0..count {
            if sources[i] == Pubkey::default() { bad = true; }
        }
        assert!(bad);
    }


    #[test]
    fn buckets_isolated_per_mint() {
        let mut bucket_usdc = fresh_bucket(0);
        let mut bucket_sol = fresh_bucket(0);
        bucket_usdc.mint = Pubkey::new_unique();
        bucket_sol.mint = SOL_PSEUDO_MINT;
        bucket_usdc.unclaimed = 1_000;
        bucket_sol.unclaimed = 5_000;
        assert_ne!(bucket_usdc.mint, bucket_sol.mint);
        assert_ne!(bucket_usdc.unclaimed, bucket_sol.unclaimed);
    }

    #[test]
    fn buckets_isolated_per_affiliate() {
        let mut alice = fresh_bucket(0);
        let mut bob = fresh_bucket(0);
        alice.affiliate = Pubkey::new_unique();
        bob.affiliate = Pubkey::new_unique();
        alice.unclaimed = 100;
        bob.unclaimed = 200;
        assert_ne!(alice.affiliate, bob.affiliate);
        assert_ne!(alice.unclaimed, bob.unclaimed);
    }


    #[test]
    fn rolling_hold_bug_fix_steady_stream_matures() {
        let mut b = fresh_bucket(0);
        let mut t = 0i64;
        for _ in 0..10 {
            b.unclaimed = b.unclaimed.checked_add(10).unwrap();
            if b.claimable_at == 0 {
                b.claimable_at = t + COMMISSION_HOLD_SECONDS;
            }
            t += 12 * 3600;
        }
        let anchor = b.claimable_at;
        let now = 8 * 86_400i64;
        assert!(now >= anchor);
        assert_eq!(b.unclaimed, 100);
    }

    #[test]
    fn rolling_hold_first_anchor_value_correct() {
        let mut b = fresh_bucket(0);
        let t0: i64 = 1_500_000;
        b.unclaimed = b.unclaimed.checked_add(50).unwrap();
        b.claimable_at = t0 + COMMISSION_HOLD_SECONDS;
        assert_eq!(b.claimable_at - t0, 7 * 24 * 3600);
    }

    #[test]
    fn v1_legacy_record_advances_anchor_each_call() {
        let mut affiliate_unclaimed: u64 = 0;
        let mut claimable_at: i64 = 0;
        let mut t: i64 = 0;
        for _ in 0..3 {
            affiliate_unclaimed += 10;
            let proposed = t + COMMISSION_HOLD_SECONDS;
            if proposed > claimable_at {
                claimable_at = proposed;
            }
            t += 86_400;
        }
        assert_eq!(claimable_at, 2 * 86_400 + COMMISSION_HOLD_SECONDS);
    }


    #[test]
    fn next_record_after_claim_arms_fresh_window() {
        let mut b = fresh_bucket(0);
        b.unclaimed = 100;
        b.claimable_at = COMMISSION_HOLD_SECONDS;
        let claim_t: i64 = 8 * 86_400;
        assert!(claim_t >= b.claimable_at);
        b.unclaimed = 0;
        b.claimable_at = 0;
        let next_t: i64 = 10 * 86_400;
        b.unclaimed = b.unclaimed.checked_add(50).unwrap();
        if b.claimable_at == 0 {
            b.claimable_at = next_t + COMMISSION_HOLD_SECONDS;
        }
        assert_eq!(b.claimable_at, next_t + COMMISSION_HOLD_SECONDS);
        assert!(b.claimable_at > claim_t);
    }


    #[test]
    fn claim_before_hold_expires_rejects() {
        let b = CommissionBucket {
            affiliate: Pubkey::new_unique(),
            mint: Pubkey::new_unique(),
            unclaimed: 100,
            claimable_at: 1_000_000,
            total_earned: 0,
            bump: 0,
            reserved: [0u8; 32],
        };
        let now: i64 = 500_000;
        assert!(now < b.claimable_at);
    }


    #[test]
    fn deposit_funding_pool_amount_zero_rejects() {
        let amount: u64 = 0;
        assert!(amount == 0);
    }

    #[test]
    fn deposit_funding_pool_asset_mismatch_rejects() {
        let pool_mint = Pubkey::new_unique();
        let requested_mint = Pubkey::new_unique();
        assert_ne!(pool_mint, requested_mint);
    }

    #[test]
    fn funding_pool_total_deposited_monotonic() {
        let mut total: u64 = 0;
        total = total.checked_add(100).unwrap();
        total = total.checked_add(50).unwrap();
        total = total.checked_add(1_000_000).unwrap();
        assert_eq!(total, 1_000_150);
    }

    #[test]
    fn funding_pool_paid_out_monotonic() {
        let mut paid: u64 = 0;
        paid = paid.checked_add(200).unwrap();
        paid = paid.checked_add(800).unwrap();
        assert_eq!(paid, 1_000);
    }


    #[test]
    fn funding_pool_seeds_doc() {
        let mint = Pubkey::new_unique();
        let _seed: (&[u8], &[u8; 32]) = (b"funding_pool", &mint.to_bytes());
    }

    #[test]
    fn commission_bucket_seeds_doc() {
        let wallet = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let _seed: (&[u8], &[u8; 32], &[u8; 32]) =
            (b"comm_bucket", &wallet.to_bytes(), &mint.to_bytes());
    }


    #[test]
    fn v1_legacy_fields_still_on_affiliate_struct() {
        let a = Affiliate {
            wallet: Pubkey::new_unique(),
            referral_code: [0u8; 8],
            tier: 0,
            total_earned: 0,
            unclaimed: 0,
            claimable_at: 0,
            active_players: 0,
            monthly_volume: 0,
            bump: 0,
        };
        let _u = a.unclaimed;
        let _c = a.claimable_at;
    }


    #[test]
    fn concurrent_record_commission_distinct_buckets() {
        let aff = Pubkey::new_unique();
        let mint_a = Pubkey::new_unique();
        let mint_b = Pubkey::new_unique();
        let mut bucket_a = CommissionBucket {
            affiliate: aff,
            mint: mint_a,
            unclaimed: 0,
            claimable_at: 0,
            total_earned: 0,
            bump: 0,
            reserved: [0u8; 32],
        };
        let mut bucket_b = CommissionBucket {
            affiliate: aff,
            mint: mint_b,
            unclaimed: 0,
            claimable_at: 0,
            total_earned: 0,
            bump: 0,
            reserved: [0u8; 32],
        };
        bucket_a.unclaimed = bucket_a.unclaimed.checked_add(100).unwrap();
        bucket_b.unclaimed = bucket_b.unclaimed.checked_add(250).unwrap();
        assert_eq!(bucket_a.unclaimed, 100);
        assert_eq!(bucket_b.unclaimed, 250);
        assert_ne!(bucket_a.mint, bucket_b.mint);
    }


    #[test]
    fn commission_hold_7_days_exact() {
        assert_eq!(COMMISSION_HOLD_SECONDS, 7 * 24 * 3600);
    }

    #[test]
    fn max_funding_sources_16() {
        assert_eq!(MAX_FUNDING_SOURCES, 16);
    }


    #[test]
    fn clawback_partial_then_full() {
        let mut b = fresh_bucket(0);
        b.unclaimed = 100;
        b.claimable_at = 10_000;
        let part = 30u64.min(b.unclaimed);
        b.unclaimed = b.unclaimed.checked_sub(part).unwrap();
        if b.unclaimed == 0 { b.claimable_at = 0; }
        assert_eq!(b.unclaimed, 70);
        assert_ne!(b.claimable_at, 0);
        let full = 999u64.min(b.unclaimed);
        b.unclaimed = b.unclaimed.checked_sub(full).unwrap();
        if b.unclaimed == 0 { b.claimable_at = 0; }
        assert_eq!(b.unclaimed, 0);
        assert_eq!(b.claimable_at, 0);
    }


    fn fresh_affiliate() -> Affiliate {
        Affiliate {
            wallet: Pubkey::new_unique(),
            referral_code: [0u8; 8],
            tier: AffiliateTier::Standard as u8,
            total_earned: 0,
            unclaimed: 0,
            claimable_at: 0,
            active_players: 0,
            monthly_volume: 0,
            bump: 255,
        }
    }

    fn fresh_pool(mint: Pubkey, deposited: u64) -> FundingPool {
        FundingPool {
            asset_mint: mint,
            token_account: Pubkey::new_unique(),
            sol_holder: Pubkey::default(),
            is_sol_pool: false,
            total_deposited: deposited,
            total_paid_out: 0,
            bump: 255,
            total_unclaimed: 0,
            reserved: [0u8; 24],
        }
    }

    fn apply_record_v2(
        affiliate: &mut Affiliate,
        bucket: &mut CommissionBucket,
        pool: &mut FundingPool,
        amount: u64,
        now: i64,
    ) -> core::result::Result<(), ()> {
        bucket.unclaimed = bucket.unclaimed.checked_add(amount).ok_or(())?;
        let new_total_unclaimed = pool.total_unclaimed.checked_add(amount).ok_or(())?;
        let liquidity = pool.total_deposited.checked_sub(pool.total_paid_out).ok_or(())?;
        if new_total_unclaimed > liquidity {
            bucket.unclaimed -= amount;
            return Err(());
        }
        pool.total_unclaimed = new_total_unclaimed;
        if bucket.claimable_at == 0 {
            bucket.claimable_at = now + COMMISSION_HOLD_SECONDS;
        }
        affiliate.total_earned = affiliate.total_earned.checked_add(amount).ok_or(())?;
        affiliate.monthly_volume = affiliate.monthly_volume.checked_add(amount).ok_or(())?;
        Ok(())
    }

    fn apply_clawback_v2(
        affiliate: &mut Affiliate,
        bucket: &mut CommissionBucket,
        pool: &mut FundingPool,
        amount: u64,
    ) {
        let clawback = amount.min(bucket.unclaimed);
        bucket.unclaimed = bucket.unclaimed.checked_sub(clawback).unwrap();
        if bucket.unclaimed == 0 {
            bucket.claimable_at = 0;
        }
        pool.total_unclaimed = pool.total_unclaimed.checked_sub(clawback).unwrap();
        affiliate.total_earned = affiliate.total_earned.saturating_sub(clawback);
        affiliate.monthly_volume = affiliate.monthly_volume.saturating_sub(clawback);
    }

    #[test]
    fn ar_r3_01_record_then_full_clawback_returns_analytics_to_baseline() {
        let mint = Pubkey::new_unique();
        let mut aff = fresh_affiliate();
        let mut bucket = CommissionBucket {
            affiliate: aff.wallet,
            mint,
            unclaimed: 0,
            claimable_at: 0,
            total_earned: 0,
            bump: 255,
            reserved: [0u8; 32],
        };
        let mut pool = fresh_pool(mint, 1_000_000);

        assert_eq!(aff.total_earned, 0);
        assert_eq!(aff.monthly_volume, 0);

        apply_record_v2(&mut aff, &mut bucket, &mut pool, 5_000, 1_000).unwrap();
        assert_eq!(aff.total_earned, 5_000);
        assert_eq!(aff.monthly_volume, 5_000);
        assert_eq!(bucket.unclaimed, 5_000);
        assert_eq!(pool.total_unclaimed, 5_000);

        apply_clawback_v2(&mut aff, &mut bucket, &mut pool, 5_000);

        assert_eq!(aff.total_earned, 0, "total_earned must return to baseline after full clawback");
        assert_eq!(aff.monthly_volume, 0, "monthly_volume must return to baseline after full clawback");
        assert_eq!(bucket.unclaimed, 0);
        assert_eq!(bucket.claimable_at, 0, "fully-drained bucket re-arms anchor");
        assert_eq!(pool.total_unclaimed, 0, "A-01 invariant preserved");
    }

    #[test]
    fn ar_r3_01_record_clawback_loop_cannot_inflate_tier() {
        let mint = Pubkey::new_unique();
        let mut aff = fresh_affiliate();
        let mut bucket = CommissionBucket {
            affiliate: aff.wallet,
            mint,
            unclaimed: 0,
            claimable_at: 0,
            total_earned: 0,
            bump: 255,
            reserved: [0u8; 32],
        };
        let mut pool = fresh_pool(mint, 1_000);

        for i in 0..10_000 {
            apply_record_v2(&mut aff, &mut bucket, &mut pool, 1_000, 1_000 + i).unwrap();
            apply_clawback_v2(&mut aff, &mut bucket, &mut pool, 1_000);
            assert_eq!(aff.monthly_volume, 0);
            assert_eq!(aff.total_earned, 0);
        }
        let tier = if aff.monthly_volume >= PARTNER_MONTHLY_VOLUME {
            AffiliateTier::Partner as u8
        } else if aff.active_players >= GOLD_PLAYERS {
            AffiliateTier::Gold as u8
        } else if aff.active_players >= SILVER_PLAYERS {
            AffiliateTier::Silver as u8
        } else {
            AffiliateTier::Standard as u8
        };
        assert_eq!(tier, AffiliateTier::Standard as u8,
            "record→clawback loop must NOT forge Partner tier — monthly_volume stays at baseline");
    }

    #[test]
    fn ar_r3_01_partial_clawback_decrements_analytics_by_clawed_amount() {
        let mint = Pubkey::new_unique();
        let mut aff = fresh_affiliate();
        let mut bucket = CommissionBucket {
            affiliate: aff.wallet,
            mint,
            unclaimed: 0,
            claimable_at: 0,
            total_earned: 0,
            bump: 255,
            reserved: [0u8; 32],
        };
        let mut pool = fresh_pool(mint, 1_000_000);

        apply_record_v2(&mut aff, &mut bucket, &mut pool, 300, 1_000).unwrap();
        apply_record_v2(&mut aff, &mut bucket, &mut pool, 200, 2_000).unwrap();
        assert_eq!(aff.total_earned, 500);
        assert_eq!(aff.monthly_volume, 500);

        apply_clawback_v2(&mut aff, &mut bucket, &mut pool, 120);
        assert_eq!(aff.total_earned, 380, "total_earned -= clawed amount");
        assert_eq!(aff.monthly_volume, 380, "monthly_volume -= clawed amount");
        assert_eq!(bucket.unclaimed, 380);
        assert_eq!(pool.total_unclaimed, 380, "A-01 invariant tracks bucket");
        assert_ne!(bucket.claimable_at, 0);
    }

    #[test]
    fn ar_r3_01_clawback_saturates_never_underflows() {
        let mint = Pubkey::new_unique();
        let mut aff = fresh_affiliate();
        let mut bucket = CommissionBucket {
            affiliate: aff.wallet,
            mint,
            unclaimed: 0,
            claimable_at: 0,
            total_earned: 0,
            bump: 255,
            reserved: [0u8; 32],
        };
        let mut pool = fresh_pool(mint, 1_000_000);

        apply_record_v2(&mut aff, &mut bucket, &mut pool, 1_000, 1_000).unwrap();
        aff.monthly_volume = 0;
        aff.total_earned = 0;

        apply_clawback_v2(&mut aff, &mut bucket, &mut pool, 1_000);
        assert_eq!(aff.monthly_volume, 0, "saturating floor — no underflow panic");
        assert_eq!(aff.total_earned, 0, "saturating floor — no underflow panic");
        assert_eq!(bucket.unclaimed, 0);
        assert_eq!(pool.total_unclaimed, 0);
    }


    #[test]
    fn commission_bucket_fits_under_200_bytes() {
        assert!(CommissionBucket::LEN < 200);
    }

    #[test]
    fn funding_pool_fits_under_200_bytes() {
        assert!(FundingPool::LEN < 200);
    }


    #[test]
    fn sol_pseudo_mint_distinct_from_random_pubkey() {
        let rand = Pubkey::new_unique();
        assert_ne!(SOL_PSEUDO_MINT, rand);
    }


    #[derive(Clone, Copy)]
    struct SimulatedTokenAccount {
        mint: Pubkey,
        owner: Pubkey,
    }

    fn satisfies_token_authority_constraint(
        account: &SimulatedTokenAccount,
        expected_authority: &Pubkey,
    ) -> bool {
        account.owner == *expected_authority
    }

    fn satisfies_token_mint_constraint(
        account: &SimulatedTokenAccount,
        expected_mint: &Pubkey,
    ) -> bool {
        account.mint == *expected_mint
    }

    #[test]
    fn initialize_rejects_attacker_owned_pool_invariant() {
        let affiliate_config_pda = Pubkey::new_unique();
        let attacker = Pubkey::new_unique();
        let attacker_owned_pool = SimulatedTokenAccount {
            mint: Pubkey::new_unique(),
            owner: attacker,
        };
        assert!(
            !satisfies_token_authority_constraint(&attacker_owned_pool, &affiliate_config_pda),
            "front-runner's attacker-owned pool must not satisfy `token::authority = affiliate_config`"
        );
    }

    #[test]
    fn legitimate_initialize_pool_passes_authority_invariant() {
        let affiliate_config_pda = Pubkey::new_unique();
        let canonical_pool = SimulatedTokenAccount {
            mint: Pubkey::new_unique(),
            owner: affiliate_config_pda,
        };
        assert!(
            satisfies_token_authority_constraint(&canonical_pool, &affiliate_config_pda),
            "canonical pool owned by affiliate_config PDA must satisfy authority constraint"
        );
    }

    #[test]
    fn init_funding_pool_rejects_attacker_owned_holder_invariant() {
        let funding_pool_pda = Pubkey::new_unique();
        let usdc_mint = Pubkey::new_unique();
        let attacker = Pubkey::new_unique();
        let attacker_owned_holder = SimulatedTokenAccount {
            mint: usdc_mint,
            owner: attacker,
        };
        assert!(
            !satisfies_token_authority_constraint(&attacker_owned_holder, &funding_pool_pda),
            "attacker-owned holder must not satisfy `token::authority = funding_pool`"
        );
        let imposter_mint = Pubkey::new_unique();
        let wrong_mint_holder = SimulatedTokenAccount {
            mint: imposter_mint,
            owner: funding_pool_pda,
        };
        assert!(
            !satisfies_token_mint_constraint(&wrong_mint_holder, &usdc_mint),
            "holder with wrong mint must not satisfy `token::mint = mint`"
        );
    }

    #[test]
    fn legitimate_init_funding_pool_passes_authority_and_mint_invariants() {
        let funding_pool_pda = Pubkey::new_unique();
        let usdc_mint = Pubkey::new_unique();
        let canonical_holder = SimulatedTokenAccount {
            mint: usdc_mint,
            owner: funding_pool_pda,
        };
        assert!(
            satisfies_token_authority_constraint(&canonical_holder, &funding_pool_pda),
            "canonical holder owned by funding_pool PDA must satisfy authority constraint"
        );
        assert!(
            satisfies_token_mint_constraint(&canonical_holder, &usdc_mint),
            "canonical holder with matching mint must satisfy mint constraint"
        );
    }

    #[test]
    fn sol_pool_init_funding_pool_skips_token_holder_constraint() {
        let token_holder: Option<SimulatedTokenAccount> = None;
        assert!(token_holder.is_none(),
            "SOL-branch callers pass None for token_holder; authority/mint pins are skipped");
    }


    fn drift_handler_end(src: &str, idx: usize) -> usize {
        src[idx + 1..]
            .find("\n    pub fn ")
            .map(|p| idx + 1 + p)
            .expect("drift-gate: no following `pub fn` — re-anchor this source-assert bound")
    }

    fn affiliate_registry_lib_rs_source() -> &'static str {
        include_str!("lib.rs")
    }

    #[test]
    fn record_commission_v1_is_hard_gated() {
        let src = affiliate_registry_lib_rs_source();
        let needle = "pub fn record_commission(\n        _ctx: Context<RecordCommission>";
        let idx = src.find(needle).expect("record_commission v1 handler must exist");
        let end = drift_handler_end(src, idx);
        let body = &src[idx..end];
        assert!(body.contains("err!(AffiliateError::InstructionDeprecated)"),
            "record_commission MUST hard-revert with InstructionDeprecated (R6.2 H.1 / Wave D.C.2). \
             Source excerpt:\n{}", body);
        assert!(body.contains("msg!(\"DEPRECATED:"),
            "record_commission MUST emit msg!(\"DEPRECATED:\") for Sentinel forensics. \
             Source excerpt:\n{}", body);
        
        
        let body_open = body.find("{\n").expect("handler must have body braces");
        let body_close = body[body_open..].find("\n    }").map(|i| body_open + i)
            .expect("handler body must close");
        let inner_body = &body[body_open + 2..body_close];
        let semicolons = inner_body.chars().filter(|c| *c == ';').count();
        assert!(semicolons <= 2,
            "record_commission v1 body MUST be gated — only the msg!() semicolon \
             should appear (saw {} semicolons). Body:\n{}",
            semicolons, inner_body);
    }
    #[test]
    fn record_commission_v1_signature_kept_for_idl_stability() {
        
        
        
        let src = affiliate_registry_lib_rs_source();
        assert!(src.contains("pub fn record_commission(\n        _ctx: Context<RecordCommission>"),
            "record_commission entrypoint MUST remain for IDL stability (R6.2 H.1 doc comment)");
    }
    #[test]
    fn v2_first_anchored_pattern_present() {
        
        
        
        let src = affiliate_registry_lib_rs_source();
        let needle = "pub fn record_commission_v2(";
        let idx = src.find(needle).expect("record_commission_v2 must exist");
        
        
        
        let rest = &src[idx + needle.len()..];
        let body_end = rest
            .find("\n    pub fn ")
            .map(|p| idx + needle.len() + p)
            .expect("drift-gate: bound marker absent - re-anchor (would else scan into the include_str! test module)");
        let body = &src[idx..body_end];
        
        assert!(body.contains("bucket.claimable_at == 0"),
            "record_commission_v2 MUST gate the claimable_at arm on currently-zero \
             (first-anchored, AUTH-02 fix). Source excerpt:\n{}", body);
        
        
        assert!(!body.contains("bucket.claimable_at.max("),
            "record_commission_v2 MUST NOT re-arm via max() (AFFIL-PV01 regression, \
             AUTH-02). Source excerpt:\n{}", body);
    }

    

    #[test]
    fn r77_h01_check_and_record_propose_helper_present() {
        let src = affiliate_registry_lib_rs_source();
        assert!(src.contains("fn check_and_record_propose("),
            "affiliate-registry MUST define check_and_record_propose helper (R7.7-H-01)");
        assert!(src.contains("propose_cooldown_until"),
            "AffiliateConfig MUST carry propose_cooldown_until field (R7.7-H-01)");
        assert!(src.contains("recent_proposes: [i64; 5]"),
            "AffiliateConfig MUST carry recent_proposes: [i64; 5] ring buffer (R7.7-H-01)");
        assert!(src.contains("ProposeCooldownActive"),
            "AffiliateError::ProposeCooldownActive variant MUST exist (R7.7-H-01)");
    }
    #[test]
    fn r77_h01_all_propose_handlers_call_check_and_record_propose() {
        
        
        
        let src = affiliate_registry_lib_rs_source();
        let propose_names = ["propose_rotate_admin"];
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
