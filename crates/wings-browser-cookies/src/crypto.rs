//! Chromium's Mac cookie crypto: PBKDF2-HMAC-SHA1 for the key, AES-128-CBC with a fixed IV for the
//! values. Platform independent so it is unit tested everywhere.

use aes::Aes128;
use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use sha1_smol::Sha1;
use zeroize::Zeroizing;

pub(crate) type Key = Zeroizing<[u8; 16]>;

/// Chromium derives the key with this salt and count (components/os_crypt/async/browser/keychain_key_provider.mm:
/// kDefaultSalt "saltysalt", kIterations 1003, kDerivedKeySize 16).
const SALT: &[u8] = b"saltysalt";
const ITERATIONS: u32 = 1003;

/// The IV is sixteen spaces (components/os_crypt/async/common/encryptor.cc: kFixedIvForAes128Cbc).
const IV: [u8; 16] = [b' '; 16];

const SHA1_BLOCK: usize = 64;

/// HMAC-SHA1 with the two padded-key states precomputed, so each of PBKDF2's many calls only hashes the message.
struct Hmac {
    inner: Sha1,
    outer: Sha1,
}

impl Hmac {
    fn new(key: &[u8]) -> Self {
        let mut block = [0u8; SHA1_BLOCK];
        if key.len() > SHA1_BLOCK {
            block[..20].copy_from_slice(&Sha1::from(key).digest().bytes());
        } else {
            block[..key.len()].copy_from_slice(key);
        }
        let mut inner = Sha1::new();
        let mut outer = Sha1::new();
        inner.update(&block.map(|b| b ^ 0x36));
        outer.update(&block.map(|b| b ^ 0x5c));
        Self { inner, outer }
    }

    fn mac(&self, parts: &[&[u8]]) -> [u8; 20] {
        let mut inner = self.inner.clone();
        for part in parts {
            inner.update(part);
        }
        let mut outer = self.outer.clone();
        outer.update(&inner.digest().bytes());
        outer.digest().bytes()
    }
}

pub(crate) fn pbkdf2_sha1(password: &[u8], salt: &[u8], iterations: u32, out: &mut [u8]) {
    let hmac = Hmac::new(password);
    for (index, chunk) in out.chunks_mut(20).enumerate() {
        let mut u = hmac.mac(&[salt, &(index as u32 + 1).to_be_bytes()]);
        let mut t = u;
        for _ in 1..iterations {
            u = hmac.mac(&[&u]);
            for (t, u) in t.iter_mut().zip(u) {
                *t ^= u;
            }
        }
        chunk.copy_from_slice(&t[..chunk.len()]);
    }
}

/// The AES key for a browser's "<Name> Safe Storage" Keychain password.
pub(crate) fn derive_key(password: &[u8]) -> Key {
    let mut key = Zeroizing::new([0u8; 16]);
    pbkdf2_sha1(password, SALT, ITERATIONS, &mut *key);
    key
}

/// Decrypts the bytes after the `v10` prefix. `None` for a bad length or padding (a wrong key shows up as bad padding
/// most of the time, and the caller's hash check catches the rest).
pub(crate) fn decrypt(key: &Key, ciphertext: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    let mut buf = Zeroizing::new(ciphertext.to_vec());
    let len = cbc::Decryptor::<Aes128>::new(&(**key).into(), &IV.into())
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .ok()?
        .len();
    buf.truncate(len);
    Some(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cbc::cipher::BlockEncryptMut;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Chromium's own constant for the empty password (encryptor.cc, comment: PBKDF2-HMAC-SHA1, 1 iteration,
    /// key "", salt "saltysalt"), which pins both the algorithm and the salt.
    #[test]
    fn pbkdf2_matches_chromiums_empty_key_vector() {
        let mut out = [0u8; 16];
        pbkdf2_sha1(b"", b"saltysalt", 1, &mut out);
        assert_eq!(hex(&out), "d0d0ec9c7d77d43ac54187fa4818d17f");
    }

    /// RFC 6070 test vectors, including a key longer than one block and an output of more than one block.
    #[test]
    fn pbkdf2_matches_rfc_6070() {
        let mut out = [0u8; 20];
        pbkdf2_sha1(b"password", b"salt", 2, &mut out);
        assert_eq!(hex(&out), "ea6c014dc72d6f8ccd1ed92ace1d41f0d8de8957");
        let mut out = [0u8; 25];
        pbkdf2_sha1(
            b"passwordPASSWORDpassword",
            b"saltSALTsaltSALTsaltSALTsaltSALTsalt",
            4096,
            &mut out,
        );
        assert_eq!(hex(&out), "3d2eec4fe41c849b80c8d83662c0e44a8b291a964cf2f07038");
    }

    pub(crate) fn encrypt_for_test(key: &Key, plaintext: &[u8]) -> Vec<u8> {
        let mut buf = plaintext.to_vec();
        let len = buf.len();
        buf.resize(len + 16, 0);
        let out = cbc::Encryptor::<Aes128>::new(&(**key).into(), &IV.into())
            .encrypt_padded_mut::<Pkcs7>(&mut buf, len)
            .unwrap();
        out.to_vec()
    }

    #[test]
    fn decrypt_round_trips_and_rejects_bad_input() {
        let key = derive_key(b"a known password");
        let sealed = encrypt_for_test(&key, b"session-token");
        assert_eq!(&**decrypt(&key, &sealed).unwrap(), b"session-token");
        let wrong = decrypt(&derive_key(b"another"), &sealed);
        assert_ne!(wrong.as_deref().map(|v| v.as_slice()), Some(&b"session-token"[..]));
        assert!(decrypt(&key, &sealed[..sealed.len() - 1]).is_none());
        assert!(decrypt(&key, &[]).is_none());
    }
}
