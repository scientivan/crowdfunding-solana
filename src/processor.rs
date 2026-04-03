use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    clock::Clock,
    entrypoint::ProgramResult,
    msg,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    pubkey::Pubkey,
    rent::Rent,
    system_instruction,
    sysvar::Sysvar,
};

use crate::{
    error::CrowdfundingError,
    instruction::CrowdfundingInstruction,
    state::{Campaign, Contribution, CAMPAIGN_SIZE, CONTRIBUTION_SIZE},
};

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let instruction = CrowdfundingInstruction::unpack(instruction_data)?;
    match instruction {
        CrowdfundingInstruction::CreateCampaign { goal, deadline } => {
            process_create_campaign(program_id, accounts, goal, deadline)
        }
        CrowdfundingInstruction::Contribute { amount } => {
            process_contribute(program_id, accounts, amount)
        }
        CrowdfundingInstruction::Withdraw => process_withdraw(program_id, accounts),
        CrowdfundingInstruction::Refund => process_refund(program_id, accounts),
    }
}

fn process_create_campaign(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    goal: u64,
    deadline: i64,
) -> ProgramResult {
    let account_iter = &mut accounts.iter();
    let campaign_account = next_account_info(account_iter)?;
    let creator_account = next_account_info(account_iter)?;
    let system_program = next_account_info(account_iter)?;

    if !creator_account.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if !campaign_account.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let clock = Clock::get()?;
    if deadline <= clock.unix_timestamp {
        return Err(CrowdfundingError::DeadlineInPast.into());
    }

    let rent = Rent::get()?;
    let lamports = rent.minimum_balance(CAMPAIGN_SIZE);

    invoke(
        &system_instruction::create_account(
            creator_account.key,
            campaign_account.key,
            lamports,
            CAMPAIGN_SIZE as u64,
            program_id,
        ),
        &[
            creator_account.clone(),
            campaign_account.clone(),
            system_program.clone(),
        ],
    )?;

    let campaign = Campaign {
        creator: *creator_account.key,
        goal,
        raised: 0,
        deadline,
        claimed: false,
    };

    let mut data = campaign_account.try_borrow_mut_data()?;
    let mut writer = std::io::Cursor::new(&mut data[..]);
    campaign.serialize(&mut writer)?;

    msg!("Campaign created: goal={}, deadline={}", goal, deadline);
    Ok(())
}

fn process_contribute(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    amount: u64,
) -> ProgramResult {
    let account_iter = &mut accounts.iter();
    let campaign_account = next_account_info(account_iter)?;
    let contributor_account = next_account_info(account_iter)?;
    let vault_account = next_account_info(account_iter)?;
    let contribution_account = next_account_info(account_iter)?;
    let system_program = next_account_info(account_iter)?;

    if !contributor_account.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }

    let (vault_pda, _vault_bump) = Pubkey::find_program_address(
        &[b"vault", campaign_account.key.as_ref()],
        program_id,
    );
    if vault_pda != *vault_account.key {
        return Err(CrowdfundingError::InvalidVaultAccount.into());
    }

    let (contrib_pda, contrib_bump) = Pubkey::find_program_address(
        &[
            b"contribution",
            campaign_account.key.as_ref(),
            contributor_account.key.as_ref(),
        ],
        program_id,
    );
    if contrib_pda != *contribution_account.key {
        return Err(CrowdfundingError::InvalidContributionAccount.into());
    }

    invoke(
        &system_instruction::transfer(contributor_account.key, vault_account.key, amount),
        &[
            contributor_account.clone(),
            vault_account.clone(),
            system_program.clone(),
        ],
    )?;

    if contribution_account.data_is_empty() {
        let rent = Rent::get()?;
        let contrib_lamports = rent.minimum_balance(CONTRIBUTION_SIZE);

        invoke_signed(
            &system_instruction::create_account(
                contributor_account.key,
                contribution_account.key,
                contrib_lamports,
                CONTRIBUTION_SIZE as u64,
                program_id,
            ),
            &[
                contributor_account.clone(),
                contribution_account.clone(),
                system_program.clone(),
            ],
            &[&[
                b"contribution",
                campaign_account.key.as_ref(),
                contributor_account.key.as_ref(),
                &[contrib_bump],
            ]],
        )?;

        let contribution = Contribution { amount };
        let mut data = contribution_account.try_borrow_mut_data()?;
        let mut writer = std::io::Cursor::new(&mut data[..]);
        contribution.serialize(&mut writer)?;
    } else {
        let mut contribution = {
            let data = contribution_account.try_borrow_data()?;
            Contribution::deserialize(&mut &data[..])?
        };
        contribution.amount = contribution
            .amount
            .checked_add(amount)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        let mut data = contribution_account.try_borrow_mut_data()?;
        let mut writer = std::io::Cursor::new(&mut data[..]);
        contribution.serialize(&mut writer)?;
    }

    let mut campaign = {
        let data = campaign_account.try_borrow_data()?;
        Campaign::deserialize(&mut &data[..])?
    };
    campaign.raised = campaign
        .raised
        .checked_add(amount)
        .ok_or(ProgramError::ArithmeticOverflow)?;

    let raised = campaign.raised;
    let mut data = campaign_account.try_borrow_mut_data()?;
    let mut writer = std::io::Cursor::new(&mut data[..]);
    campaign.serialize(&mut writer)?;

    msg!("Contributed: {} lamports, total={}", amount, raised);
    Ok(())
}

fn process_withdraw(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let account_iter = &mut accounts.iter();
    let campaign_account = next_account_info(account_iter)?;
    let creator_account = next_account_info(account_iter)?;
    let vault_account = next_account_info(account_iter)?;
    let system_program = next_account_info(account_iter)?;

    if !creator_account.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if campaign_account.owner != program_id {
        return Err(ProgramError::IllegalOwner);
    }

    let campaign = {
        let data = campaign_account.try_borrow_data()?;
        Campaign::deserialize(&mut &data[..])?
    };

    if *creator_account.key != campaign.creator {
        return Err(CrowdfundingError::NotCreator.into());
    }

    let clock = Clock::get()?;
    if clock.unix_timestamp < campaign.deadline {
        return Err(CrowdfundingError::DeadlineNotReached.into());
    }
    if campaign.raised < campaign.goal {
        return Err(CrowdfundingError::GoalNotReached.into());
    }
    if campaign.claimed {
        return Err(CrowdfundingError::AlreadyClaimed.into());
    }

    let (vault_pda, vault_bump) = Pubkey::find_program_address(
        &[b"vault", campaign_account.key.as_ref()],
        program_id,
    );
    if vault_pda != *vault_account.key {
        return Err(CrowdfundingError::InvalidVaultAccount.into());
    }

    let amount = vault_account.lamports();

    invoke_signed(
        &system_instruction::transfer(vault_account.key, creator_account.key, amount),
        &[
            vault_account.clone(),
            creator_account.clone(),
            system_program.clone(),
        ],
        &[&[b"vault", campaign_account.key.as_ref(), &[vault_bump]]],
    )?;

    let mut campaign = campaign;
    campaign.claimed = true;
    let mut data = campaign_account.try_borrow_mut_data()?;
    let mut writer = std::io::Cursor::new(&mut data[..]);
    campaign.serialize(&mut writer)?;

    msg!("Withdrawn: {} lamports", amount);
    Ok(())
}

fn process_refund(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let account_iter = &mut accounts.iter();
    let campaign_account = next_account_info(account_iter)?;
    let contributor_account = next_account_info(account_iter)?;
    let vault_account = next_account_info(account_iter)?;
    let contribution_account = next_account_info(account_iter)?;
    let system_program = next_account_info(account_iter)?;

    if !contributor_account.is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if campaign_account.owner != program_id {
        return Err(ProgramError::IllegalOwner);
    }

    let campaign = {
        let data = campaign_account.try_borrow_data()?;
        Campaign::deserialize(&mut &data[..])?
    };

    let clock = Clock::get()?;
    if clock.unix_timestamp < campaign.deadline {
        return Err(CrowdfundingError::DeadlineNotReached.into());
    }
    if campaign.raised >= campaign.goal {
        return Err(CrowdfundingError::GoalAlreadyReached.into());
    }

    let (vault_pda, vault_bump) = Pubkey::find_program_address(
        &[b"vault", campaign_account.key.as_ref()],
        program_id,
    );
    if vault_pda != *vault_account.key {
        return Err(CrowdfundingError::InvalidVaultAccount.into());
    }

    let (contrib_pda, _contrib_bump) = Pubkey::find_program_address(
        &[
            b"contribution",
            campaign_account.key.as_ref(),
            contributor_account.key.as_ref(),
        ],
        program_id,
    );
    if contrib_pda != *contribution_account.key {
        return Err(CrowdfundingError::InvalidContributionAccount.into());
    }

    if contribution_account.owner != program_id {
        return Err(ProgramError::IllegalOwner);
    }

    let mut contribution = {
        let data = contribution_account.try_borrow_data()?;
        Contribution::deserialize(&mut &data[..])?
    };

    if contribution.amount == 0 {
        return Err(CrowdfundingError::NoContribution.into());
    }

    let refund_amount = contribution.amount;

    invoke_signed(
        &system_instruction::transfer(vault_account.key, contributor_account.key, refund_amount),
        &[
            vault_account.clone(),
            contributor_account.clone(),
            system_program.clone(),
        ],
        &[&[b"vault", campaign_account.key.as_ref(), &[vault_bump]]],
    )?;

    contribution.amount = 0;
    let mut data = contribution_account.try_borrow_mut_data()?;
    let mut writer = std::io::Cursor::new(&mut data[..]);
    contribution.serialize(&mut writer)?;

    msg!("Refunded: {} lamports", refund_amount);
    Ok(())
}
