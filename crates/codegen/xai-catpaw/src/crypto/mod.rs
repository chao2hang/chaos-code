//! CatPaw's wire cryptography: XOR-obfuscated RSA keys, RSA-OAEP-SHA1,
//! and AES-128-ECB with PKCS#7 padding.

pub mod aes_ecb;
pub mod keys;
pub mod request;
pub mod rsa_oaep;

pub use aes_ecb::{aes_ecb_decrypt, aes_ecb_encrypt};
pub use keys::{XOR_KEY, priv_key_pem, pub_key_pem, xor_decipher};
pub use request::{
    EncryptedRequest, decrypt_response, decrypt_response_bytes, encrypt_request,
    encrypt_request_envelope,
};
pub use rsa_oaep::{
    load_rsa, rsa_oaep_decrypt, rsa_oaep_decrypt_b64, rsa_oaep_encrypt, rsa_oaep_encrypt_b64,
};
