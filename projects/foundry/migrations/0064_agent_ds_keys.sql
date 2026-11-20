create table agent_ds_keys (
    account_id uuid primary key references accounts(id) on delete cascade,
    public_key text not null,
    private_key_ciphertext bytea not null,
    fingerprint text not null unique,
    created_at timestamptz not null default now(),
    revoked_at timestamptz
);
