# AlpenGlow Consensus

## Setup

- ### Generate rsa creds

```sh
./gen-keys.sh
```

under the hood it does this

```sh
mkdir keys
```

```sh
openssl req -x509 -newkey rsa:4096 -keyout ./keys/rsa-key.pem -out ./keys/rsa-cert.pem -days 365 -nodes -subj "/CN=localhost"
```

- ### Generate ed25519 keyapir json

```sh
solana-keygen new --outfile ./keys/iden.json
```

## Run

```sh
cargo run
```
