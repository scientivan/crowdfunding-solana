use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::pubkey::Pubkey;

pub const CAMPAIGN_SIZE: usize = 57;
pub const CONTRIBUTION_SIZE: usize = 8;

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct Campaign {
    pub creator: Pubkey,
    pub goal: u64,
    pub raised: u64,
    pub deadline: i64,
    pub claimed: bool,
}

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub struct Contribution {
    pub amount: u64,
}
