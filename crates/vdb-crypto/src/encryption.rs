
pub struct EncryptionKey {
    key: chacha20poly1305::Key,
}
pub struct Nonce {
    nonce: chacha20poly1305::Nonce,
}
pub struct Ciphertext {
    cipher_text: Vec<u8>,
}

impl EncryptionKey {
    pub fn generate() -> Self;
    pub fn from_bytes(bytes: &[u8; 32]) -> Self;
    pub fn to_bytes(&self) -> [u8; 32];
}

impl Nonce {
    pub fn generate() -> Self;
    pub fn from_bytes(bytes: &[u8; 12]) -> Self;
    // Consider: from_u96 or from_counter for sequential nonces?
}

// The core operations:
pub fn encrypt(key: &EncryptionKey, nonce: &Nonce, plaintext: &[u8]) -> Ciphertext;
pub fn decrypt(
    key: &EncryptionKey,
    nonce: &Nonce,
    ciphertext: &Ciphertext,
) -> Result<Vec<u8>, CryptoError>;
