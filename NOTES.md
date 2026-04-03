# Catatan Belajar: Platform Crowdfunding di Solana

---

## Daftar Isi
1. [Gambaran Umum Proyek](#1-gambaran-umum-proyek)
2. [Perbedaan Solana vs EVM (Ethereum/Base)](#2-perbedaan-solana-vs-evm)
3. [Smart Contract Solana](#3-smart-contract-solana)
4. [Apa itu PDA?](#4-apa-itu-pda-program-derived-address)
5. [Cara Frontend Kirim Transaksi](#5-cara-frontend-kirim-transaksi-solana)
6. [Cara Indexer Menangkap Events](#6-cara-indexer-menangkap-events)
7. [Alur Data Lengkap](#7-alur-data-lengkap)
8. [Cara Deploy dan Test](#8-cara-deploy-dan-test)
9. [Struktur File Proyek](#9-struktur-file-proyek)

---

## 1. Gambaran Umum Proyek

Platform crowdfunding ini mirip Kickstarter, tapi berjalan di blockchain Solana. Pengguna bisa:
- **Membuat kampanye** dengan target dana dan deadline
- **Berdonasi SOL** ke kampanye yang mereka pilih
- **Menarik dana** jika target tercapai setelah deadline
- **Refund** jika target tidak tercapai setelah deadline

Dana tidak langsung dikirim ke kreator — melainkan dikunci di **vault** (rekening milik program) sampai kondisi terpenuhi. Ini yang disebut **trustless**: donor tidak perlu percaya kepada kreator karena aturannya sudah terkunci di blockchain.

---

## 2. Perbedaan Solana vs EVM

| Aspek | EVM (Ethereum/Base) | Solana |
|---|---|---|
| Bahasa smart contract | Solidity | Rust |
| Model akun | Single account per contract | Setiap data = akun terpisah |
| Token native | ETH (wei) | SOL (lamports) |
| Unit terkecil | 1 ETH = 10^18 wei | 1 SOL = 1,000,000,000 lamports |
| Wallet library | wagmi, viem, RainbowKit | @solana/wallet-adapter |
| Event indexing | Ponder, TheGraph | Custom listener (connection.onLogs) |
| Biaya transaksi | Gas fee (bervariasi) | ~0.000005 SOL (sangat murah) |
| Kecepatan konfirmasi | ~12 detik | ~400ms |

**Penting:** Di Solana, sebuah program (smart contract) **tidak menyimpan data di dalam dirinya sendiri**. Data disimpan di **akun-akun terpisah** yang dimiliki oleh program. Contoh: setiap kampanye punya akunnya sendiri.

---

## 3. Smart Contract Solana

### Lokasi
```
Crowdfunding Solana/
├── Cargo.toml         (konfigurasi proyek Rust)
└── src/
    ├── lib.rs         (entry point program)
    ├── state.rs       (struktur data: Campaign, Contribution)
    ├── instruction.rs (definisi instruksi yang bisa dipanggil)
    ├── processor.rs   (logika setiap instruksi)
    └── error.rs       (kode error kustom)
```

### Struktur Data (state.rs)

```rust
pub struct Campaign {
    pub creator: Pubkey,  // alamat wallet kreator (32 bytes)
    pub goal: u64,        // target dana dalam lamports (8 bytes)
    pub raised: u64,      // dana terkumpul dalam lamports (8 bytes)
    pub deadline: i64,    // unix timestamp kapan kampanye berakhir (8 bytes)
    pub claimed: bool,    // apakah sudah ditarik? (1 byte)
}
// Total: 57 bytes

pub struct Contribution {
    pub amount: u64,  // total donasi dari satu donor ke satu kampanye (8 bytes)
}
```

### 4 Instruksi

#### 1. CreateCampaign
- **Siapa yang panggil:** Kreator kampanye
- **Input:** goal (lamports), deadline (unix timestamp)
- **Yang terjadi:**
  1. Validasi deadline harus di masa depan
  2. Buat akun baru untuk menyimpan data Campaign
  3. Tulis data Campaign ke akun tersebut
- **Akun yang dibutuhkan:** campaign (keypair baru), creator, system_program

#### 2. Contribute
- **Siapa yang panggil:** Donor
- **Input:** amount (lamports)
- **Yang terjadi:**
  1. Transfer SOL dari donor ke vault PDA
  2. Jika belum pernah donasi ke kampanye ini → buat akun Contribution PDA baru
  3. Update jumlah donasi di akun Contribution
  4. Update total raised di akun Campaign
- **Akun yang dibutuhkan:** campaign, contributor, vault_pda, contribution_pda, system_program

#### 3. Withdraw
- **Siapa yang panggil:** Kreator
- **Syarat:** raised >= goal DAN waktu sekarang >= deadline DAN belum pernah withdraw
- **Yang terjadi:**
  1. Cek semua syarat
  2. Transfer semua SOL dari vault ke kreator
  3. Tandai campaign.claimed = true
- **Akun yang dibutuhkan:** campaign, creator, vault_pda, system_program

#### 4. Refund
- **Siapa yang panggil:** Donor
- **Syarat:** raised < goal DAN waktu sekarang >= deadline
- **Yang terjadi:**
  1. Cek syarat
  2. Transfer SOL dari vault kembali ke donor (sebesar kontribusinya)
  3. Reset contribution.amount = 0
- **Akun yang dibutuhkan:** campaign, contributor, vault_pda, contribution_pda, system_program

---

## 4. Apa itu PDA (Program Derived Address)?

PDA adalah **alamat akun yang dihasilkan secara deterministik** dari kombinasi seeds + program ID. Tidak ada private key untuk PDA — hanya program yang bisa "menandatangani" transaksi untuk akun PDA tersebut.

### Kenapa dipakai untuk Vault?

Di Solana, untuk bisa transfer SOL **keluar** dari suatu akun, akun itu harus menandatangani transaksi. Tapi kita ingin vault dikendalikan oleh program (bukan kreator atau siapapun). Solusinya: gunakan PDA.

Program bisa "menandatangani" untuk PDA menggunakan `invoke_signed()` dengan seeds yang sama yang dipakai untuk men-derive PDA tersebut.

### PDA di proyek ini

```
Vault PDA:
  seeds = ["vault", campaign_pubkey]
  → Satu vault per kampanye, menyimpan SOL donasi

Contribution PDA:
  seeds = ["contribution", campaign_pubkey, contributor_pubkey]
  → Satu per (kampanye × donor), menyimpan jumlah donasi
```

### Cara Kerja invoke_signed

```rust
invoke_signed(
    &system_instruction::transfer(vault_pda, creator, amount),
    &[vault_account, creator_account, system_program],
    &[&[b"vault", campaign_key.as_ref(), &[vault_bump]]],
    // ↑ seeds yang membuktikan program "memiliki" PDA ini
)?;
```

---

## 5. Cara Frontend Kirim Transaksi Solana

### Setup (Web3Provider.tsx)

Alih-alih wagmi + RainbowKit (untuk EVM), sekarang pakai:
- **ConnectionProvider** — menghubungkan ke Solana RPC node
- **WalletProvider** — mengelola koneksi wallet (Phantom, Solflare, dll)
- **WalletModalProvider** — UI modal untuk pilih wallet

```tsx
<ConnectionProvider endpoint="https://api.devnet.solana.com">
  <WalletProvider wallets={[new PhantomWalletAdapter()]}>
    <WalletModalProvider>
      {children}
    </WalletModalProvider>
  </WalletProvider>
</ConnectionProvider>
```

### Membuat Instruksi (instructions.ts)

Setiap instruksi adalah objek `TransactionInstruction` berisi:
1. **keys** — daftar akun yang terlibat (writable/signer)
2. **programId** — alamat program kita
3. **data** — byte payload (tag instruksi + argumen)

Contoh instruksi Contribute:
```ts
const data = Buffer.alloc(9);
data.writeUInt8(1, 0);                   // tag = 1 (Contribute)
data.writeBigUInt64LE(amountLamports, 1); // amount dalam lamports

new TransactionInstruction({
  keys: [
    { pubkey: campaignPubkey, isSigner: false, isWritable: true },
    { pubkey: contributorPubkey, isSigner: true, isWritable: true },
    { pubkey: vaultPda, isSigner: false, isWritable: true },
    { pubkey: contributionPda, isSigner: false, isWritable: true },
    { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
  ],
  programId: PROGRAM_ID,
  data,
})
```

### Mengirim Transaksi (useDonate.ts)

```ts
const { publicKey, sendTransaction } = useWallet();
const { connection } = useConnection();

// Build transaksi
const { blockhash } = await connection.getLatestBlockhash();
const tx = new Transaction();
tx.recentBlockhash = blockhash;
tx.feePayer = publicKey;
tx.add(contributeInstruction);

// Kirim & tunggu konfirmasi
const signature = await sendTransaction(tx, connection);
await connection.confirmTransaction(signature, 'confirmed');
```

### CreateCampaign — Kasus Khusus

Campaign account adalah keypair baru yang dibuat di frontend. Keypair ini harus ikut menandatangani transaksi (karena program akan memanggil `create_account` untuk akun ini):

```ts
const campaignKeypair = Keypair.generate();
const tx = new Transaction();
tx.add(createCampaignInstruction(campaignKeypair.publicKey, ...));
tx.partialSign(campaignKeypair); // tanda tangan kampanye dulu
// Lalu wallet user menandatangani sisanya via sendTransaction
const signature = await sendTransaction(tx, connection);
```

**Pubkey kampanye** (`campaignKeypair.publicKey.toBase58()`) menjadi ID kampanye yang dipakai di seluruh sistem.

---

## 6. Cara Indexer Menangkap Events

### Problem

Ponder (indexer yang dipakai sebelumnya) hanya mendukung EVM. Untuk Solana, kita perlu mekanisme berbeda.

### Solusinya: `connection.onLogs()`

Solana RPC punya websocket subscription. Kita bisa subscribe ke semua log dari program kita:

```ts
connection.onLogs(PROGRAM_ID, (logInfo) => {
  // logInfo.logs = array string log
  // logInfo.signature = tx hash
  // logInfo.err = null jika sukses
});
```

### Parsing Logs

Program kita menggunakan `msg!()` untuk logging:
```rust
msg!("Campaign created: goal={}, deadline={}", goal, deadline);
msg!("Contributed: {} lamports, total={}", amount, raised);
msg!("Withdrawn: {} lamports", amount);
msg!("Refunded: {} lamports", refund_amount);
```

Indexer mem-parse baris log ini menggunakan regex untuk menentukan jenis event.

### Mendapatkan Akun yang Terlibat

Setelah tahu jenis event, indexer fetch transaksi lengkap untuk mendapat daftar akun:
```ts
const tx = await connection.getTransaction(signature, { maxSupportedTransactionVersion: 0 });
const accounts = tx.transaction.message.staticAccountKeys;
// accounts[0] = campaign, accounts[1] = creator/contributor, dst
```

### Push ke Backend

Sama seperti sebelumnya, indexer push data ke backend via HTTP POST ke endpoint `/api/sync/`:
```ts
POST http://localhost:3300/api/sync/campaign   → simpan kampanye baru
POST http://localhost:3300/api/sync/donation   → simpan donasi
POST http://localhost:3300/api/sync/withdrawal → simpan penarikan
```

---

## 7. Alur Data Lengkap

```
USER
  │
  ▼ (1) Klik "Create Campaign" di frontend
FRONTEND (Next.js)
  │   - useWallet() dari @solana/wallet-adapter
  │   - Build TransactionInstruction (tag=0x00, goal, deadline)
  │   - Generate campaign keypair baru
  │   - sendTransaction() → wallet user muncul untuk approve
  │
  ▼ (2) Transaksi dikirim ke Solana Devnet
SOLANA DEVNET
  │   - Program crowdfunding dieksekusi
  │   - Akun Campaign dibuat (57 bytes, diisi data)
  │   - msg!("Campaign created: goal=..., deadline=...")
  │
  ▼ (3) Log program terdeteksi
INDEXER (Node.js)
  │   - connection.onLogs() menerima notifikasi
  │   - Parse log → event type: CampaignCreated
  │   - Fetch tx lengkap → dapat campaign pubkey
  │   - POST /api/sync/campaign ke backend
  │
  ▼ (4) Backend menerima webhook
BACKEND (Express.js)
  │   - syncCampaign() menyimpan ke PostgreSQL
  │   - blockchain_campaigns table diupdate
  │
  ▼ (5) Frontend fetch data kampanye
FRONTEND
  │   - GET /crowdfunding/campaigns → backend baca dari PostgreSQL
  │   - Tampilkan daftar kampanye
  │
  ▼
USER melihat kampanye baru
```

---

## 8. Cara Deploy dan Test

### Prerequisites

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install Solana CLI
sh -c "$(curl -sSfL https://release.solana.com/v1.18.26/install)"

# Tambah target BPF untuk kompilasi
rustup target add bpfel-unknown-unknown
```

### Langkah 1: Kompilasi Smart Contract

```bash
cd "Crowdfunding Solana"
cargo build-sbf
# Output: target/deploy/crowdfunding.so
```

### Langkah 2: Setup Wallet & Devnet

```bash
# Generate keypair baru (kalau belum ada)
solana-keygen new --outfile ~/.config/solana/id.json

# Set ke devnet
solana config set --url devnet

# Minta SOL gratis (faucet devnet)
solana airdrop 2

# Cek saldo
solana balance
```

### Langkah 3: Deploy Program

```bash
solana program deploy target/deploy/crowdfunding.so
# Output: Program Id: Abcde1234...
# ← SIMPAN Program ID ini!
```

### Langkah 4: Isi Environment Variables

**be/.env:**
```env
SOLANA_PROGRAM_ID=<Program ID dari deploy>
SOLANA_RPC_URL=https://api.devnet.solana.com
SOLANA_PRIVATE_KEY=<base58 private key untuk signing>
PONDER_SYNC_API_KEY=your-secret-key
DATABASE_URL=postgresql://...
```

**indexer-2/.env:**
```env
SOLANA_PROGRAM_ID=<Program ID yang sama>
SOLANA_RPC_URL=https://api.devnet.solana.com
BACKEND_URL=http://localhost:3300
INDEXER_API_KEY=your-secret-key
```

**fe/.env.local:**
```env
NEXT_PUBLIC_SOLANA_PROGRAM_ID=<Program ID yang sama>
NEXT_PUBLIC_SOLANA_RPC_URL=https://api.devnet.solana.com
NEXT_PUBLIC_API_URL=http://localhost:3300
```

### Langkah 5: Jalankan Semua Service

```bash
# Terminal 1 - Backend
cd be && npm install && npm run start

# Terminal 2 - Indexer
cd indexer-2 && npm install && npm run dev

# Terminal 3 - Frontend
cd fe && npm install && npm run dev
```

### Langkah 6: Test Manual

1. Buka http://localhost:3000
2. Connect Phantom Wallet (pastikan set ke Devnet)
3. Klik "Create Campaign" → isi form → submit
4. Approve di Phantom → tunggu konfirmasi
5. Cek Solana Explorer: `https://explorer.solana.com/tx/<signature>?cluster=devnet`
6. Kampanye muncul di beranda setelah indexer push ke backend

### Testing Skenario Lengkap (dari task.txt)

```
1. Buat kampanye: goal = 10 SOL, deadline = besok
2. Donasi 6 SOL → raised = 6 SOL
3. Donasi 5 SOL lagi → raised = 11 SOL (>= goal)
4. Coba withdraw sebelum deadline → GAGAL (DeadlineNotReached)
5. Tunggu sampai deadline lewat
6. Withdraw → BERHASIL, semua SOL pindah ke kreator
7. Coba withdraw lagi → GAGAL (AlreadyClaimed)
```

---

## 9. Struktur File Proyek

```
Crowdfunding Solana/
│
├── src/                          ← Solana Smart Contract (Rust)
│   ├── lib.rs
│   ├── state.rs
│   ├── instruction.rs
│   ├── processor.rs
│   └── error.rs
├── Cargo.toml
│
├── fe/                           ← Frontend (Next.js)
│   ├── src/
│   │   ├── components/Contexts/
│   │   │   └── Web3Provider.tsx  ← Solana wallet adapter setup
│   │   ├── hooks/
│   │   │   ├── useCreateCampaign.ts  ← kirim tx CreateCampaign
│   │   │   ├── useDonate.ts          ← kirim tx Contribute
│   │   │   └── useWithdraw.ts        ← kirim tx Withdraw
│   │   ├── utils/solana/
│   │   │   ├── config.ts        ← PROGRAM_ID, RPC URL
│   │   │   ├── pda.ts           ← helper derivasi PDA
│   │   │   └── instructions.ts  ← builder instruksi Solana
│   │   └── modules/campaign/    ← halaman UI kampanye
│   └── package.json
│
├── indexer-2/                    ← Indexer (Node.js)
│   ├── src/
│   │   ├── index.ts             ← entry point
│   │   ├── listener.ts          ← subscribe onLogs
│   │   ├── parser.ts            ← parse log messages
│   │   ├── handlers.ts          ← handle events, push ke backend
│   │   └── utils/syncBackend.ts ← HTTP push ke backend
│   └── package.json
│
├── be/                           ← Backend (Express.js)
│   ├── config/solana.ts          ← Solana connection config (BARU)
│   ├── services/autoSync.ts      ← polling Solana RPC (diupdate)
│   └── routes/sync.ts            ← webhook endpoints dari indexer
│
└── NOTES.md                      ← file ini
```

---

## Konsep Kunci yang Perlu Diingat

| Konsep | Penjelasan Singkat |
|---|---|
| **Program** | Smart contract di Solana, ditulis dalam Rust, di-compile ke BPF bytecode |
| **Account** | Unit penyimpanan data di Solana, mirip "file" yang dimiliki oleh suatu program |
| **PDA** | Alamat akun tanpa private key, hanya bisa di-sign oleh program menggunakan seeds |
| **Lamports** | Unit terkecil SOL. 1 SOL = 1,000,000,000 lamports |
| **CPI** | Cross-Program Invocation — program memanggil program lain (misalnya system_program untuk transfer SOL) |
| **Rent** | Biaya menyewa ruang di blockchain. Akun harus punya cukup SOL untuk "rent-exempt" |
| **Blockhash** | "Waktu kadaluarsa" transaksi. Transaksi harus di-submit sebelum blockhash kedaluwarsa |
| **Commitment** | Tingkat finalitas: processed → confirmed → finalized |
