# AlpenGlow Consensus

## Setup

- ### Generate rsa creds

```sh
openssl req -x509 -newkey rsa:4096 -keyout rsa-key.pem -out rsa-cert.pem -days 365 -nodes -subj "/CN=localhost"
```

- ### Generate ed25519 keyapir json

```sh
solana-keygen new --outfile iden.json
```

## Run

```sh
cargo run
```
