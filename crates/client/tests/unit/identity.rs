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
