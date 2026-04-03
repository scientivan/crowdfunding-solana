use solana_program::program_error::ProgramError;

#[derive(Debug)]
pub enum CrowdfundingError {
    InvalidInstruction,
    DeadlineInPast,
    DeadlineNotReached,
    GoalNotReached,
    GoalAlreadyReached,
    AlreadyClaimed,
    NotCreator,
    NoContribution,
    InvalidVaultAccount,
    InvalidContributionAccount,
}

impl From<CrowdfundingError> for ProgramError {
    fn from(e: CrowdfundingError) -> Self {
        ProgramError::Custom(e as u32)
    }
}
