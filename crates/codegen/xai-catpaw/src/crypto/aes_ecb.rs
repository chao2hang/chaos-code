use aes::{
    Aes128,
    cipher::{Array, BlockCipherDecrypt, BlockCipherEncrypt, KeyInit},
};

use crate::{Error, Result};

const BLOCK_LEN: usize = 16;

pub fn aes_ecb_encrypt(plaintext: &[u8], key: &[u8]) -> Result<Vec<u8>> {
    let key: &[u8; BLOCK_LEN] = key
        .try_into()
        .map_err(|_| Error::Aes(format!("AES-128 requires a 16-byte key, got {}", key.len())))?;
    let cipher = Aes128::new(&Array::from(*key));
    let padding = BLOCK_LEN - plaintext.len() % BLOCK_LEN;
    let mut output = Vec::with_capacity(plaintext.len() + padding);
    output.extend_from_slice(plaintext);
    output.extend(std::iter::repeat_n(padding as u8, padding));

    for chunk in output.chunks_exact_mut(BLOCK_LEN) {
        let block: &mut aes::Block = chunk.try_into().expect("exact AES block");
        cipher.encrypt_block(block);
    }
    Ok(output)
}

pub fn aes_ecb_decrypt(ciphertext: &[u8], key: &[u8]) -> Result<Vec<u8>> {
    let key: &[u8; BLOCK_LEN] = key
        .try_into()
        .map_err(|_| Error::Aes(format!("AES-128 requires a 16-byte key, got {}", key.len())))?;
    if ciphertext.is_empty() || !ciphertext.len().is_multiple_of(BLOCK_LEN) {
        return Err(Error::Pkcs7);
    }
    let cipher = Aes128::new(&Array::from(*key));
    let mut output = ciphertext.to_vec();
    for chunk in output.chunks_exact_mut(BLOCK_LEN) {
        let block: &mut aes::Block = chunk.try_into().expect("exact AES block");
        cipher.decrypt_block(block);
    }

    let padding = *output.last().ok_or(Error::Pkcs7)? as usize;
    if padding == 0
        || padding > BLOCK_LEN
        || padding > output.len()
        || !output[output.len() - padding..]
            .iter()
            .all(|byte| *byte as usize == padding)
    {
        return Err(Error::Pkcs7);
    }
    output.truncate(output.len() - padding);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crypto_roundtrip_including_full_padding_block() {
        let key = b"0123456789abcdef";
        for plaintext in [b"short".as_slice(), b"0123456789abcdef", b""] {
            let ciphertext = aes_ecb_encrypt(plaintext, key).unwrap();
            assert_eq!(ciphertext.len() % BLOCK_LEN, 0);
            assert_eq!(aes_ecb_decrypt(&ciphertext, key).unwrap(), plaintext);
        }
    }

    #[test]
    fn malformed_padding_is_rejected() {
        let key = b"0123456789abcdef";
        let mut ciphertext = aes_ecb_encrypt(b"secret", key).unwrap();
        *ciphertext.last_mut().unwrap() ^= 0x7f;
        assert!(aes_ecb_decrypt(&ciphertext, key).is_err());
    }
}
