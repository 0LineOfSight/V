
use ed25519_dalek::{SigningKey, VerifyingKey, Signature, Signer, Verifier, SECRET_KEY_LENGTH, PUBLIC_KEY_LENGTH};
use rand_core::OsRng;
use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PubKey(#[serde(with = "hex::serde")] pub [u8; PUBLIC_KEY_LENGTH]);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SecretKey(#[serde(with = "hex::serde")] pub [u8; SECRET_KEY_LENGTH]);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sig(#[serde(with = "hex::serde")] pub [u8; 64]);

impl PubKey {
    pub fn to_verkey(&self) -> VerifyingKey {
        VerifyingKey::from_bytes(&self.0).expect("valid pubkey")
    }
    pub fn hex(&self) -> String { hex::encode(self.0) }
}

impl SecretKey {
    pub fn to_signing_key(&self) -> SigningKey {
        SigningKey::from_bytes(&self.0)
    }
    pub fn hex(&self) -> String { hex::encode(self.0) }
}

pub fn generate() -> (SecretKey, PubKey) {
    let sk = SigningKey::generate(&mut OsRng);
    let pk = sk.verifying_key();
    (SecretKey(sk.to_bytes()), PubKey(pk.to_bytes()))
}

pub fn sign(sk: &SecretKey, msg: &[u8]) -> Sig {
    let sig: Signature = sk.to_signing_key().sign(msg);
    Sig(sig.to_bytes())
}

pub fn verify(pk: &PubKey, msg: &[u8], sig: &Sig) -> bool {
    let sig = Signature::from_bytes(&sig.0);
    pk.to_verkey().verify(msg, &sig).is_ok()
}
