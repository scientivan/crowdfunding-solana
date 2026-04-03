use borsh::BorshDeserialize;
use crowdfunding::processor::process_instruction;
use crowdfunding::state::{Campaign, Contribution, CAMPAIGN_SIZE, CONTRIBUTION_SIZE};
use solana_program::pubkey::Pubkey;
use solana_program_test::*;
use solana_sdk::{
    account::Account,
    instruction::{AccountMeta, Instruction},
    signature::{Keypair, Signer},
    system_program,
    transaction::Transaction,
};

fn program_id() -> Pubkey {
    Pubkey::new_unique()
}

fn build_create_campaign_ix(
    program_id: Pubkey,
    campaign: &Keypair,
    creator: &Keypair,
    goal: u64,
    deadline: i64,
) -> Instruction {
    let mut data = vec![0u8];
    data.extend_from_slice(&goal.to_le_bytes());
    data.extend_from_slice(&deadline.to_le_bytes());
    Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(campaign.pubkey(), true),
            AccountMeta::new(creator.pubkey(), true),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data,
    }
}

fn build_contribute_ix(
    program_id: Pubkey,
    campaign: Pubkey,
    contributor: &Keypair,
    amount: u64,
) -> Instruction {
    let (vault_pda, _) =
        Pubkey::find_program_address(&[b"vault", campaign.as_ref()], &program_id);
    let (contrib_pda, _) = Pubkey::find_program_address(
        &[b"contribution", campaign.as_ref(), contributor.pubkey().as_ref()],
        &program_id,
    );
    let mut data = vec![1u8];
    data.extend_from_slice(&amount.to_le_bytes());
    Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(campaign, false),
            AccountMeta::new(contributor.pubkey(), true),
            AccountMeta::new(vault_pda, false),
            AccountMeta::new(contrib_pda, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data,
    }
}

fn build_withdraw_ix(
    program_id: Pubkey,
    campaign: Pubkey,
    creator: &Keypair,
) -> Instruction {
    let (vault_pda, _) =
        Pubkey::find_program_address(&[b"vault", campaign.as_ref()], &program_id);
    Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(campaign, false),
            AccountMeta::new(creator.pubkey(), true),
            AccountMeta::new(vault_pda, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data: vec![2u8],
    }
}

fn build_refund_ix(
    program_id: Pubkey,
    campaign: Pubkey,
    contributor: &Keypair,
) -> Instruction {
    let (vault_pda, _) =
        Pubkey::find_program_address(&[b"vault", campaign.as_ref()], &program_id);
    let (contrib_pda, _) = Pubkey::find_program_address(
        &[b"contribution", campaign.as_ref(), contributor.pubkey().as_ref()],
        &program_id,
    );
    Instruction {
        program_id,
        accounts: vec![
            AccountMeta::new(campaign, false),
            AccountMeta::new(contributor.pubkey(), true),
            AccountMeta::new(vault_pda, false),
            AccountMeta::new(contrib_pda, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data: vec![3u8],
    }
}

async fn setup() -> (BanksClient, Keypair, solana_sdk::hash::Hash, Pubkey) {
    let pid = program_id();
    let mut program_test = ProgramTest::new(
        "crowdfunding",
        pid,
        processor!(process_instruction),
    );
    program_test.prefer_bpf(false);
    let (banks_client, payer, recent_blockhash) = program_test.start().await;
    (banks_client, payer, recent_blockhash, pid)
}

// ─────────────────────────────────────────────
// 1. CreateCampaign: sukses
// ─────────────────────────────────────────────
#[tokio::test]
async fn test_create_campaign_success() {
    let (mut banks_client, payer, recent_blockhash, pid) = setup().await;

    let campaign = Keypair::new();
    let deadline = solana_program::clock::Clock::default().unix_timestamp + 86400;

    let ix = build_create_campaign_ix(pid, &campaign, &payer, 1_000_000_000, deadline);
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[&payer, &campaign],
        recent_blockhash,
    );
    banks_client.process_transaction(tx).await.unwrap();

    let account = banks_client
        .get_account(campaign.pubkey())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(account.data.len(), CAMPAIGN_SIZE);

    let data = Campaign::deserialize(&mut &account.data[..]).unwrap();
    assert_eq!(data.creator, payer.pubkey());
    assert_eq!(data.goal, 1_000_000_000);
    assert_eq!(data.raised, 0);
    assert_eq!(data.deadline, deadline);
    assert!(!data.claimed);
}

// ─────────────────────────────────────────────
// 2. CreateCampaign: deadline di masa lalu → gagal
// ─────────────────────────────────────────────
#[tokio::test]
async fn test_create_campaign_deadline_in_past() {
    let (mut banks_client, payer, recent_blockhash, pid) = setup().await;

    let campaign = Keypair::new();
    let ix = build_create_campaign_ix(pid, &campaign, &payer, 1_000_000_000, 1);
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[&payer, &campaign],
        recent_blockhash,
    );
    assert!(banks_client.process_transaction(tx).await.is_err());
}

// ─────────────────────────────────────────────
// 3. Contribute: sukses
// ─────────────────────────────────────────────
#[tokio::test]
async fn test_contribute_success() {
    let (mut banks_client, payer, recent_blockhash, pid) = setup().await;

    let campaign = Keypair::new();
    let deadline = solana_program::clock::Clock::default().unix_timestamp + 86400;
    let goal = 2_000_000_000u64;
    let contribution_amount = 500_000_000u64;

    let create_ix = build_create_campaign_ix(pid, &campaign, &payer, goal, deadline);
    let tx = Transaction::new_signed_with_payer(
        &[create_ix],
        Some(&payer.pubkey()),
        &[&payer, &campaign],
        recent_blockhash,
    );
    banks_client.process_transaction(tx).await.unwrap();

    let (_, recent_blockhash, _) = banks_client.get_fees().await.unwrap();
    let contrib_ix = build_contribute_ix(pid, campaign.pubkey(), &payer, contribution_amount);
    let tx = Transaction::new_signed_with_payer(
        &[contrib_ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );
    banks_client.process_transaction(tx).await.unwrap();

    let (contrib_pda, _) = Pubkey::find_program_address(
        &[b"contribution", campaign.pubkey().as_ref(), payer.pubkey().as_ref()],
        &pid,
    );
    let contrib_account = banks_client
        .get_account(contrib_pda)
        .await
        .unwrap()
        .unwrap();
    let contribution = Contribution::deserialize(&mut &contrib_account.data[..]).unwrap();
    assert_eq!(contribution.amount, contribution_amount);

    let campaign_account = banks_client
        .get_account(campaign.pubkey())
        .await
        .unwrap()
        .unwrap();
    let camp_data = Campaign::deserialize(&mut &campaign_account.data[..]).unwrap();
    assert_eq!(camp_data.raised, contribution_amount);
}

// ─────────────────────────────────────────────
// 4. Contribute: akumulasi dari dua transaksi
// ─────────────────────────────────────────────
#[tokio::test]
async fn test_contribute_accumulates() {
    let (mut banks_client, payer, recent_blockhash, pid) = setup().await;

    let campaign = Keypair::new();
    let deadline = solana_program::clock::Clock::default().unix_timestamp + 86400;

    let create_ix = build_create_campaign_ix(pid, &campaign, &payer, 2_000_000_000, deadline);
    let tx = Transaction::new_signed_with_payer(
        &[create_ix],
        Some(&payer.pubkey()),
        &[&payer, &campaign],
        recent_blockhash,
    );
    banks_client.process_transaction(tx).await.unwrap();

    for _ in 0..2 {
        let (_, blockhash, _) = banks_client.get_fees().await.unwrap();
        let ix = build_contribute_ix(pid, campaign.pubkey(), &payer, 100_000_000);
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&payer.pubkey()),
            &[&payer],
            blockhash,
        );
        banks_client.process_transaction(tx).await.unwrap();
    }

    let (contrib_pda, _) = Pubkey::find_program_address(
        &[b"contribution", campaign.pubkey().as_ref(), payer.pubkey().as_ref()],
        &pid,
    );
    let contrib_account = banks_client
        .get_account(contrib_pda)
        .await
        .unwrap()
        .unwrap();
    let contribution = Contribution::deserialize(&mut &contrib_account.data[..]).unwrap();
    assert_eq!(contribution.amount, 200_000_000);
}

// ─────────────────────────────────────────────
// 5. Withdraw: sukses setelah deadline & goal tercapai
// ─────────────────────────────────────────────
#[tokio::test]
async fn test_withdraw_success() {
    let (mut banks_client, payer, recent_blockhash, pid) = setup().await;

    let campaign = Keypair::new();
    let deadline = 1i64; // di masa lalu agar clock > deadline setelah warp

    // Buat campaign dengan goal kecil
    let create_ix = build_create_campaign_ix(pid, &campaign, &payer, 100_000_000, deadline);
    let tx = Transaction::new_signed_with_payer(
        &[create_ix],
        Some(&payer.pubkey()),
        &[&payer, &campaign],
        recent_blockhash,
    );
    // Deadline di masa lalu akan ditolak oleh program (Clock check)
    // Jadi kita set deadline jauh di masa depan dulu, contribute, lalu warp time
    let _ = banks_client.process_transaction(tx).await;

    // Cara alternatif: set deadline valid, contribute hingga goal, lalu warp
    let (mut banks_client, payer, recent_blockhash, pid) = setup().await;
    let campaign = Keypair::new();
    let goal = 100_000_000u64;
    let future_deadline = solana_program::clock::Clock::default().unix_timestamp + 2;

    let create_ix = build_create_campaign_ix(pid, &campaign, &payer, goal, future_deadline);
    let tx = Transaction::new_signed_with_payer(
        &[create_ix],
        Some(&payer.pubkey()),
        &[&payer, &campaign],
        recent_blockhash,
    );
    banks_client.process_transaction(tx).await.unwrap();

    let (_, blockhash, _) = banks_client.get_fees().await.unwrap();
    let contrib_ix = build_contribute_ix(pid, campaign.pubkey(), &payer, goal);
    let tx = Transaction::new_signed_with_payer(
        &[contrib_ix],
        Some(&payer.pubkey()),
        &[&payer],
        blockhash,
    );
    banks_client.process_transaction(tx).await.unwrap();

    // Warp ke setelah deadline
    banks_client.warp_to_slot(500).await.unwrap();

    let (_, blockhash, _) = banks_client.get_fees().await.unwrap();
    let withdraw_ix = build_withdraw_ix(pid, campaign.pubkey(), &payer);
    let tx = Transaction::new_signed_with_payer(
        &[withdraw_ix],
        Some(&payer.pubkey()),
        &[&payer],
        blockhash,
    );
    banks_client.process_transaction(tx).await.unwrap();

    let campaign_account = banks_client
        .get_account(campaign.pubkey())
        .await
        .unwrap()
        .unwrap();
    let camp_data = Campaign::deserialize(&mut &campaign_account.data[..]).unwrap();
    assert!(camp_data.claimed);
}

// ─────────────────────────────────────────────
// 6. Withdraw: double withdraw → gagal
// ─────────────────────────────────────────────
#[tokio::test]
async fn test_withdraw_already_claimed() {
    let (mut banks_client, payer, recent_blockhash, pid) = setup().await;

    let campaign = Keypair::new();
    let goal = 100_000_000u64;
    let future_deadline = solana_program::clock::Clock::default().unix_timestamp + 2;

    let create_ix = build_create_campaign_ix(pid, &campaign, &payer, goal, future_deadline);
    let tx = Transaction::new_signed_with_payer(
        &[create_ix],
        Some(&payer.pubkey()),
        &[&payer, &campaign],
        recent_blockhash,
    );
    banks_client.process_transaction(tx).await.unwrap();

    let (_, blockhash, _) = banks_client.get_fees().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[build_contribute_ix(pid, campaign.pubkey(), &payer, goal)],
        Some(&payer.pubkey()),
        &[&payer],
        blockhash,
    );
    banks_client.process_transaction(tx).await.unwrap();

    banks_client.warp_to_slot(500).await.unwrap();

    let (_, blockhash, _) = banks_client.get_fees().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[build_withdraw_ix(pid, campaign.pubkey(), &payer)],
        Some(&payer.pubkey()),
        &[&payer],
        blockhash,
    );
    banks_client.process_transaction(tx).await.unwrap();

    // Withdraw kedua harus gagal
    let (_, blockhash, _) = banks_client.get_fees().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[build_withdraw_ix(pid, campaign.pubkey(), &payer)],
        Some(&payer.pubkey()),
        &[&payer],
        blockhash,
    );
    assert!(banks_client.process_transaction(tx).await.is_err());
}

// ─────────────────────────────────────────────
// 7. Withdraw: goal tidak tercapai → gagal
// ─────────────────────────────────────────────
#[tokio::test]
async fn test_withdraw_goal_not_reached() {
    let (mut banks_client, payer, recent_blockhash, pid) = setup().await;

    let campaign = Keypair::new();
    let goal = 1_000_000_000u64;
    let future_deadline = solana_program::clock::Clock::default().unix_timestamp + 2;

    let create_ix = build_create_campaign_ix(pid, &campaign, &payer, goal, future_deadline);
    let tx = Transaction::new_signed_with_payer(
        &[create_ix],
        Some(&payer.pubkey()),
        &[&payer, &campaign],
        recent_blockhash,
    );
    banks_client.process_transaction(tx).await.unwrap();

    let (_, blockhash, _) = banks_client.get_fees().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[build_contribute_ix(pid, campaign.pubkey(), &payer, 100_000)],
        Some(&payer.pubkey()),
        &[&payer],
        blockhash,
    );
    banks_client.process_transaction(tx).await.unwrap();

    banks_client.warp_to_slot(500).await.unwrap();

    let (_, blockhash, _) = banks_client.get_fees().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[build_withdraw_ix(pid, campaign.pubkey(), &payer)],
        Some(&payer.pubkey()),
        &[&payer],
        blockhash,
    );
    assert!(banks_client.process_transaction(tx).await.is_err());
}

// ─────────────────────────────────────────────
// 8. Refund: sukses saat goal tidak tercapai
// ─────────────────────────────────────────────
#[tokio::test]
async fn test_refund_success() {
    let (mut banks_client, payer, recent_blockhash, pid) = setup().await;

    let campaign = Keypair::new();
    let goal = 1_000_000_000u64;
    let future_deadline = solana_program::clock::Clock::default().unix_timestamp + 2;
    let contrib_amount = 100_000_000u64;

    let create_ix = build_create_campaign_ix(pid, &campaign, &payer, goal, future_deadline);
    let tx = Transaction::new_signed_with_payer(
        &[create_ix],
        Some(&payer.pubkey()),
        &[&payer, &campaign],
        recent_blockhash,
    );
    banks_client.process_transaction(tx).await.unwrap();

    let (_, blockhash, _) = banks_client.get_fees().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[build_contribute_ix(pid, campaign.pubkey(), &payer, contrib_amount)],
        Some(&payer.pubkey()),
        &[&payer],
        blockhash,
    );
    banks_client.process_transaction(tx).await.unwrap();

    banks_client.warp_to_slot(500).await.unwrap();

    let balance_before = banks_client
        .get_balance(payer.pubkey())
        .await
        .unwrap();

    let (_, blockhash, _) = banks_client.get_fees().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[build_refund_ix(pid, campaign.pubkey(), &payer)],
        Some(&payer.pubkey()),
        &[&payer],
        blockhash,
    );
    banks_client.process_transaction(tx).await.unwrap();

    let balance_after = banks_client
        .get_balance(payer.pubkey())
        .await
        .unwrap();

    // Balance harus naik setelah refund (minus fee)
    assert!(balance_after > balance_before);

    // Contribution amount harus nol
    let (contrib_pda, _) = Pubkey::find_program_address(
        &[b"contribution", campaign.pubkey().as_ref(), payer.pubkey().as_ref()],
        &pid,
    );
    let contrib_account = banks_client
        .get_account(contrib_pda)
        .await
        .unwrap()
        .unwrap();
    let contribution = Contribution::deserialize(&mut &contrib_account.data[..]).unwrap();
    assert_eq!(contribution.amount, 0);
}

// ─────────────────────────────────────────────
// 9. Refund: goal sudah tercapai → gagal
// ─────────────────────────────────────────────
#[tokio::test]
async fn test_refund_goal_reached_fails() {
    let (mut banks_client, payer, recent_blockhash, pid) = setup().await;

    let campaign = Keypair::new();
    let goal = 100_000_000u64;
    let future_deadline = solana_program::clock::Clock::default().unix_timestamp + 2;

    let create_ix = build_create_campaign_ix(pid, &campaign, &payer, goal, future_deadline);
    let tx = Transaction::new_signed_with_payer(
        &[create_ix],
        Some(&payer.pubkey()),
        &[&payer, &campaign],
        recent_blockhash,
    );
    banks_client.process_transaction(tx).await.unwrap();

    let (_, blockhash, _) = banks_client.get_fees().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[build_contribute_ix(pid, campaign.pubkey(), &payer, goal)],
        Some(&payer.pubkey()),
        &[&payer],
        blockhash,
    );
    banks_client.process_transaction(tx).await.unwrap();

    banks_client.warp_to_slot(500).await.unwrap();

    let (_, blockhash, _) = banks_client.get_fees().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[build_refund_ix(pid, campaign.pubkey(), &payer)],
        Some(&payer.pubkey()),
        &[&payer],
        blockhash,
    );
    assert!(banks_client.process_transaction(tx).await.is_err());
}

// ─────────────────────────────────────────────
// 10. Withdraw: bukan creator → gagal
// ─────────────────────────────────────────────
#[tokio::test]
async fn test_withdraw_not_creator() {
    let (mut banks_client, payer, recent_blockhash, pid) = setup().await;

    let campaign = Keypair::new();
    let goal = 100_000_000u64;
    let future_deadline = solana_program::clock::Clock::default().unix_timestamp + 2;

    let create_ix = build_create_campaign_ix(pid, &campaign, &payer, goal, future_deadline);
    let tx = Transaction::new_signed_with_payer(
        &[create_ix],
        Some(&payer.pubkey()),
        &[&payer, &campaign],
        recent_blockhash,
    );
    banks_client.process_transaction(tx).await.unwrap();

    let (_, blockhash, _) = banks_client.get_fees().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[build_contribute_ix(pid, campaign.pubkey(), &payer, goal)],
        Some(&payer.pubkey()),
        &[&payer],
        blockhash,
    );
    banks_client.process_transaction(tx).await.unwrap();

    banks_client.warp_to_slot(500).await.unwrap();

    let impostor = Keypair::new();
    // Fund impostor
    let (_, blockhash, _) = banks_client.get_fees().await.unwrap();
    let fund_ix = solana_sdk::system_instruction::transfer(
        &payer.pubkey(),
        &impostor.pubkey(),
        1_000_000_000,
    );
    let tx = Transaction::new_signed_with_payer(&[fund_ix], Some(&payer.pubkey()), &[&payer], blockhash);
    banks_client.process_transaction(tx).await.unwrap();

    let (_, blockhash, _) = banks_client.get_fees().await.unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[build_withdraw_ix(pid, campaign.pubkey(), &impostor)],
        Some(&impostor.pubkey()),
        &[&impostor],
        blockhash,
    );
    assert!(banks_client.process_transaction(tx).await.is_err());
}
