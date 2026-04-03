use solana_program::program_error::ProgramError;
use crate::error::CrowdfundingError;

pub enum CrowdfundingInstruction {
    CreateCampaign { goal: u64, deadline: i64 },
    Contribute { amount: u64 },
    Withdraw,
    Refund,
}

impl CrowdfundingInstruction {
    pub fn unpack(input: &[u8]) -> Result<Self, ProgramError> {
        let (tag, rest) = input.split_first().ok_or(CrowdfundingError::InvalidInstruction)?;
        match tag {
            0 => {
                if rest.len() < 16 {
                    return Err(CrowdfundingError::InvalidInstruction.into());
                }
                let goal = u64::from_le_bytes(rest[..8].try_into().unwrap());
                let deadline = i64::from_le_bytes(rest[8..16].try_into().unwrap());
                Ok(Self::CreateCampaign { goal, deadline })
            }
            1 => {
                if rest.len() < 8 {
                    return Err(CrowdfundingError::InvalidInstruction.into());
                }
                let amount = u64::from_le_bytes(rest[..8].try_into().unwrap());
                Ok(Self::Contribute { amount })
            }
            2 => Ok(Self::Withdraw),
            3 => Ok(Self::Refund),
            _ => Err(CrowdfundingError::InvalidInstruction.into()),
        }
    }
}
