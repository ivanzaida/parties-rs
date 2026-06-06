#![allow(dead_code)]

use std::{error::Error, fmt};

use bip39::Language;
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};

use crate::network::protocol::{AuthIdentity, DecodeResult, PROTOCOL_VERSION, PublicKey, SecretKey, Signature};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalIdentity {
  pub seed_phrase: Option<String>,
  pub secret_key: SecretKey,
  pub public_key: PublicKey,
}

#[derive(Debug)]
pub enum IdentityError {
  Random(getrandom::Error),
  InvalidSeedPhrase,
  InvalidPrivateKeyHex,
}

impl fmt::Display for IdentityError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Random(e) => write!(f, "random source failed: {e}"),
      Self::InvalidSeedPhrase => write!(f, "invalid seed phrase"),
      Self::InvalidPrivateKeyHex => write!(f, "invalid private key hex"),
    }
  }
}

impl Error for IdentityError {}

impl From<getrandom::Error> for IdentityError {
  fn from(value: getrandom::Error) -> Self {
    Self::Random(value)
  }
}

pub fn generate_identity() -> Result<LocalIdentity, IdentityError> {
  let seed_phrase = generate_seed_phrase()?;
  let (secret_key, public_key) = derive_keypair(&seed_phrase)?;

  Ok(LocalIdentity {
    seed_phrase: Some(seed_phrase),
    secret_key,
    public_key,
  })
}

pub fn restore_seed_phrase(input: &str) -> Result<LocalIdentity, IdentityError> {
  let seed_phrase = canonical_seed_phrase(input);
  validate_seed_phrase(&seed_phrase)?;
  let (secret_key, public_key) = derive_keypair(&seed_phrase)?;

  Ok(LocalIdentity {
    seed_phrase: Some(seed_phrase),
    secret_key,
    public_key,
  })
}

pub fn import_private_key_hex(input: &str) -> Result<LocalIdentity, IdentityError> {
  let secret_key = secret_key_from_hex(input)?;
  let public_key = derive_pubkey(&secret_key);

  Ok(LocalIdentity {
    seed_phrase: None,
    secret_key,
    public_key,
  })
}

pub fn auth_identity(
  identity: &LocalIdentity,
  display_name: &str,
  timestamp: u64,
  password: impl Into<String>,
) -> DecodeResult<AuthIdentity> {
  let payload = AuthIdentity::signed_payload(&identity.public_key, display_name, timestamp)?;
  Ok(AuthIdentity {
    protocol_version: PROTOCOL_VERSION,
    public_key: identity.public_key,
    display_name: display_name.to_owned(),
    timestamp,
    signature: sign(&identity.secret_key, &payload),
    password: password.into(),
  })
}

pub fn generate_seed_phrase() -> Result<String, IdentityError> {
  let words = Language::English.word_list();
  let mut phrase = Vec::with_capacity(12);

  for _ in 0..12 {
    let mut raw = [0u8; 2];
    getrandom::fill(&mut raw)?;
    let index = u16::from_le_bytes(raw) as usize % words.len();
    phrase.push(words[index]);
  }

  Ok(phrase.join(" "))
}

pub fn canonical_seed_phrase(input: &str) -> String {
  input
    .split_whitespace()
    .map(str::to_ascii_lowercase)
    .collect::<Vec<_>>()
    .join(" ")
}

pub fn validate_seed_phrase(input: &str) -> Result<(), IdentityError> {
  let words = Language::English.word_list();
  let mut count = 0;

  for word in input.split_whitespace() {
    if words.binary_search(&word).is_err() {
      return Err(IdentityError::InvalidSeedPhrase);
    }
    count += 1;
  }

  if count == 12 {
    Ok(())
  } else {
    Err(IdentityError::InvalidSeedPhrase)
  }
}

pub fn first_invalid_seed_word(input: &str) -> Option<(usize, String)> {
  let words = Language::English.word_list();

  for (index, word) in input.split_whitespace().enumerate() {
    let normalized = word.to_ascii_lowercase();
    if words.binary_search(&normalized.as_str()).is_err() {
      return Some((index + 1, word.to_owned()));
    }
  }

  None
}

pub fn derive_keypair(seed_phrase: &str) -> Result<(SecretKey, PublicKey), IdentityError> {
  validate_seed_phrase(seed_phrase)?;

  let secret_key = secret_key_from_seed_phrase(seed_phrase);
  let public_key = derive_pubkey(&secret_key);
  Ok((secret_key, public_key))
}

pub fn secret_key_from_seed_phrase(seed_phrase: &str) -> SecretKey {
  let digest = Sha256::digest(seed_phrase.as_bytes());
  let mut secret_key = [0u8; 32];
  secret_key.copy_from_slice(&digest);
  secret_key
}

pub fn derive_pubkey(secret_key: &SecretKey) -> PublicKey {
  SigningKey::from_bytes(secret_key).verifying_key().to_bytes()
}

pub fn sign(secret_key: &SecretKey, message: &[u8]) -> Signature {
  SigningKey::from_bytes(secret_key).sign(message).to_bytes()
}

pub fn secret_key_to_hex(secret_key: &SecretKey) -> String {
  let mut out = String::with_capacity(64);
  for byte in secret_key {
    out.push(hex_char(byte >> 4));
    out.push(hex_char(byte & 0x0f));
  }
  out
}

pub fn secret_key_from_hex(input: &str) -> Result<SecretKey, IdentityError> {
  let input = input.trim();
  if input.len() != 64 {
    return Err(IdentityError::InvalidPrivateKeyHex);
  }

  let mut secret_key = [0u8; 32];
  for (index, chunk) in input.as_bytes().chunks_exact(2).enumerate() {
    let high = hex_value(chunk[0]).ok_or(IdentityError::InvalidPrivateKeyHex)?;
    let low = hex_value(chunk[1]).ok_or(IdentityError::InvalidPrivateKeyHex)?;
    secret_key[index] = (high << 4) | low;
  }

  Ok(secret_key)
}

pub fn public_key_fingerprint(public_key: &PublicKey) -> String {
  let digest = Sha256::digest(public_key);
  let mut out = String::with_capacity(95);

  for (index, byte) in digest.iter().enumerate() {
    if index > 0 {
      out.push(':');
    }
    out.push(hex_char(byte >> 4));
    out.push(hex_char(byte & 0x0f));
  }

  out
}

fn hex_char(value: u8) -> char {
  match value {
    0..=9 => (b'0' + value) as char,
    10..=15 => (b'a' + value - 10) as char,
    _ => unreachable!("nibble out of range"),
  }
}

fn hex_value(value: u8) -> Option<u8> {
  match value {
    b'0'..=b'9' => Some(value - b'0'),
    b'a'..=b'f' => Some(value - b'a' + 10),
    b'A'..=b'F' => Some(value - b'A' + 10),
    _ => None,
  }
}

#[cfg(test)]
mod tests {
  use ed25519_dalek::{Signature as DalekSignature, Verifier, VerifyingKey};

  use super::*;
  use crate::network::protocol::AuthIdentity;

  const PHRASE: &str = "abandon ability able about above absent absorb abstract absurd abuse access accident";

  #[test]
  fn generated_seed_phrase_has_twelve_valid_words() {
    let phrase = generate_seed_phrase().unwrap();
    validate_seed_phrase(&phrase).unwrap();
    assert_eq!(phrase.split_whitespace().count(), 12);
  }

  #[test]
  fn canonical_seed_phrase_collapses_input_spacing_and_case() {
    assert_eq!(
      canonical_seed_phrase("  Abandon   ABILITY\nable  "),
      "abandon ability able"
    );
  }

  #[test]
  fn first_invalid_seed_word_reports_position_and_value() {
    let phrase = "abandon ability able about above absent rendr abstract absurd abuse access accident";
    assert_eq!(first_invalid_seed_word(phrase), Some((7, "rendr".to_owned())));
  }

  #[test]
  fn seed_phrase_derivation_is_deterministic() {
    let first = derive_keypair(PHRASE).unwrap();
    let second = derive_keypair(PHRASE).unwrap();
    assert_eq!(first, second);
  }

  #[test]
  fn private_key_hex_round_trips() {
    let (secret_key, _) = derive_keypair(PHRASE).unwrap();
    let hex = secret_key_to_hex(&secret_key);
    assert_eq!(secret_key_from_hex(&hex).unwrap(), secret_key);
  }

  #[test]
  fn auth_payload_signature_verifies() {
    let (secret_key, public_key) = derive_keypair(PHRASE).unwrap();
    let payload = AuthIdentity::signed_payload(&public_key, "alice", 42).unwrap();
    let signature = sign(&secret_key, &payload);

    VerifyingKey::from_bytes(&public_key)
      .unwrap()
      .verify(&payload, &DalekSignature::from_bytes(&signature))
      .unwrap();
  }

  #[test]
  fn auth_identity_uses_protocol_shape() {
    let identity = restore_seed_phrase(PHRASE).unwrap();
    let auth = auth_identity(&identity, "alice", 42, "password").unwrap();
    let payload = AuthIdentity::signed_payload(&auth.public_key, &auth.display_name, auth.timestamp).unwrap();

    assert_eq!(auth.protocol_version, PROTOCOL_VERSION);
    assert_eq!(auth.password, "password");
    VerifyingKey::from_bytes(&auth.public_key)
      .unwrap()
      .verify(&payload, &DalekSignature::from_bytes(&auth.signature))
      .unwrap();
  }
}
