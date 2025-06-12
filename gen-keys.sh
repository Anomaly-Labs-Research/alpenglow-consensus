#!/bin/bash

mkdir keys

openssl req -x509 -newkey rsa:4096 -keyout ./keys/rsa-key.pem -out ./keys/rsa-cert.pem -days 365 -nodes -subj "/CN=localhost"

solana-keygen new --outfile ./keys/iden.json