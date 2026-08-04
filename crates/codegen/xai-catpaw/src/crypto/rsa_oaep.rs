use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use rsa::{
    Oaep, RsaPrivateKey, RsaPublicKey,
    pkcs8::{DecodePrivateKey, DecodePublicKey},
};
use sha1::Sha1;

use crate::crypto::{priv_key_pem, pub_key_pem};
use crate::{Error, Result};

pub fn load_rsa() -> Result<(RsaPublicKey, RsaPrivateKey)> {
    let public = RsaPublicKey::from_public_key_pem(pub_key_pem()?)
        .map_err(|error| Error::Rsa(format!("public key: {error}")))?;
    let private = RsaPrivateKey::from_pkcs8_pem(priv_key_pem()?)
        .map_err(|error| Error::Rsa(format!("private key: {error}")))?;
    Ok((public, private))
}

pub fn rsa_oaep_encrypt(public: &RsaPublicKey, plaintext: &[u8]) -> Result<Vec<u8>> {
    let mut rng = rsa::rand_core::OsRng;
    public
        .encrypt(&mut rng, Oaep::new::<Sha1>(), plaintext)
        .map_err(Into::into)
}

pub fn rsa_oaep_decrypt(private: &RsaPrivateKey, ciphertext: &[u8]) -> Result<Vec<u8>> {
    private
        .decrypt(Oaep::new::<Sha1>(), ciphertext)
        .map_err(Into::into)
}

pub fn rsa_oaep_encrypt_b64(public: &RsaPublicKey, plaintext: &[u8]) -> Result<String> {
    Ok(BASE64.encode(rsa_oaep_encrypt(public, plaintext)?))
}

pub fn rsa_oaep_decrypt_b64(private: &RsaPrivateKey, ciphertext: &str) -> Result<Vec<u8>> {
    rsa_oaep_decrypt(private, &BASE64.decode(ciphertext)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsa::traits::PublicKeyParts;

    #[test]
    fn bundled_keys_load_and_oaep_roundtrip() {
        let (_, private) = load_rsa().unwrap();
        assert_eq!(private.n().bits(), 2048);
        let public = RsaPublicKey::from(&private);
        let ciphertext = rsa_oaep_encrypt(&public, b"catpaw-key").unwrap();
        assert_eq!(
            rsa_oaep_decrypt(&private, &ciphertext).unwrap(),
            b"catpaw-key"
        );
    }
}
