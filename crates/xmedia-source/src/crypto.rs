//! Native cryptographic & binary helpers exposed to Plugin v2 (`ctx.crypto`) and JS bridges.
//!
//! Supports MD5, SHA-1, SHA-256, HMAC-SHA1/256, AES-128/192/256 (CBC, ECB, GCM with PKCS#7,
//! zero, or no padding), and RSA encryption (PKCS#1 v1.5, OAEP-SHA256, and raw `m^e mod n`
//! modular exponentiation for protocols like NetEase Cloud Music `weapi`).

use aes::{Aes128, Aes192, Aes256};
use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes128Gcm, Aes256Gcm, KeyInit as AeadKeyInit, Nonce};
use base64::Engine;
use base64::engine::general_purpose::{
    STANDARD as BASE64_STD, STANDARD_NO_PAD as BASE64_NO_PAD, URL_SAFE as BASE64_URL,
    URL_SAFE_NO_PAD as BASE64_URL_NO_PAD,
};
use cipher::block_padding::{NoPadding, Pkcs7, ZeroPadding};
use cipher::{BlockModeDecrypt, BlockModeEncrypt, KeyInit, KeyIvInit};
use hmac::{Hmac, KeyInit as HmacKeyInit, Mac};
use md5::{Digest, Md5};
use rsa::pkcs1::DecodeRsaPublicKey;
use rsa::pkcs8::DecodePublicKey;
use rsa::{BoxedUint, PublicKey};
use serde::Deserialize;
use sha1::Sha1;
use sha2::Sha256;

const MAX_CRYPTO_INPUT_BYTES: usize = 8 * 1024 * 1024;
const MAX_RANDOM_BYTES: usize = 4096;
const MAX_RAW_RSA_MODULUS_BYTES: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteEncoding {
    Utf8,
    Hex,
    Base64,
    Base64Url,
}

impl ByteEncoding {
    pub fn parse(raw: Option<&str>, default: Self) -> Result<Self, String> {
        let Some(s) = raw.map(str::trim).filter(|v| !v.is_empty()) else {
            return Ok(default);
        };
        match s.to_ascii_lowercase().as_str() {
            "utf8" | "utf-8" | "text" | "string" => Ok(Self::Utf8),
            "hex" => Ok(Self::Hex),
            "base64" | "b64" => Ok(Self::Base64),
            "base64url" | "b64url" | "url_safe" => Ok(Self::Base64Url),
            other => Err(format!("不支持的字节编码格式: '{other}'")),
        }
    }
}

pub fn decode_bytes(raw: &str, enc: ByteEncoding) -> Result<Vec<u8>, String> {
    if raw.len() > MAX_CRYPTO_INPUT_BYTES * 2 {
        return Err("加密输入数据超过体积上限".into());
    }
    let out = match enc {
        ByteEncoding::Utf8 => raw.as_bytes().to_vec(),
        ByteEncoding::Hex => {
            let cleaned: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
            let padded = if cleaned.len() % 2 == 1 {
                format!("0{cleaned}")
            } else {
                cleaned
            };
            hex::decode(&padded).map_err(|e| format!("Hex 解码失败: {e}"))?
        }
        ByteEncoding::Base64 => {
            let cleaned: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
            BASE64_STD
                .decode(&cleaned)
                .or_else(|_| BASE64_NO_PAD.decode(cleaned.trim_end_matches('=')))
                .or_else(|_| BASE64_URL.decode(&cleaned))
                .or_else(|_| BASE64_URL_NO_PAD.decode(cleaned.trim_end_matches('=')))
                .map_err(|e| format!("Base64 解码失败: {e}"))?
        }
        ByteEncoding::Base64Url => {
            let cleaned: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
            BASE64_URL
                .decode(&cleaned)
                .or_else(|_| BASE64_URL_NO_PAD.decode(cleaned.trim_end_matches('=')))
                .or_else(|_| BASE64_STD.decode(&cleaned))
                .map_err(|e| format!("Base64Url 解码失败: {e}"))?
        }
    };
    if out.len() > MAX_CRYPTO_INPUT_BYTES {
        return Err("解码后的字节数据超过体积上限".into());
    }
    Ok(out)
}

pub fn encode_bytes(bytes: &[u8], enc: ByteEncoding) -> Result<String, String> {
    match enc {
        ByteEncoding::Utf8 => {
            String::from_utf8(bytes.to_vec()).map_err(|e| format!("解密结果不是有效 UTF-8: {e}"))
        }
        ByteEncoding::Hex => Ok(hex::encode(bytes)),
        ByteEncoding::Base64 => Ok(BASE64_STD.encode(bytes)),
        ByteEncoding::Base64Url => Ok(BASE64_URL_NO_PAD.encode(bytes)),
    }
}

pub fn hash_digest(
    alg: &str,
    data: &str,
    input_enc: Option<&str>,
    output_enc: Option<&str>,
) -> Result<String, String> {
    let in_enc = ByteEncoding::parse(input_enc, ByteEncoding::Utf8)?;
    let out_enc = ByteEncoding::parse(output_enc, ByteEncoding::Hex)?;
    let bytes = decode_bytes(data, in_enc)?;
    let digest: Vec<u8> = match alg.to_ascii_lowercase().as_str() {
        "md5" => {
            let mut h = Md5::new();
            h.update(&bytes);
            h.finalize().to_vec()
        }
        "sha1" | "sha-1" => {
            let mut h = Sha1::new();
            h.update(&bytes);
            h.finalize().to_vec()
        }
        "sha256" | "sha-256" => {
            let mut h = Sha256::new();
            h.update(&bytes);
            h.finalize().to_vec()
        }
        other => return Err(format!("不支持的哈希算法: '{other}'")),
    };
    encode_bytes(&digest, out_enc)
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct HmacOptions {
    pub key_encoding: Option<String>,
    pub input_encoding: Option<String>,
    pub output_encoding: Option<String>,
}

pub fn hmac_digest(alg: &str, key: &str, data: &str, opts: HmacOptions) -> Result<String, String> {
    let key_enc = ByteEncoding::parse(opts.key_encoding.as_deref(), ByteEncoding::Utf8)?;
    let in_enc = ByteEncoding::parse(opts.input_encoding.as_deref(), ByteEncoding::Utf8)?;
    let out_enc = ByteEncoding::parse(opts.output_encoding.as_deref(), ByteEncoding::Hex)?;
    let key_bytes = decode_bytes(key, key_enc)?;
    let data_bytes = decode_bytes(data, in_enc)?;

    let tag: Vec<u8> = match alg.to_ascii_lowercase().as_str() {
        "sha1" | "sha-1" | "hmac-sha1" => {
            let mut mac = <Hmac<Sha1> as HmacKeyInit>::new_from_slice(&key_bytes)
                .map_err(|e| format!("初始化 HMAC-SHA1 失败: {e}"))?;
            mac.update(&data_bytes);
            mac.finalize().into_bytes().to_vec()
        }
        "sha256" | "sha-256" | "hmac-sha256" => {
            let mut mac = <Hmac<Sha256> as HmacKeyInit>::new_from_slice(&key_bytes)
                .map_err(|e| format!("初始化 HMAC-SHA256 失败: {e}"))?;
            mac.update(&data_bytes);
            mac.finalize().into_bytes().to_vec()
        }
        other => return Err(format!("不支持的 HMAC 算法: '{other}'")),
    };
    encode_bytes(&tag, out_enc)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockPad {
    Pkcs7,
    Zero,
    None,
}

impl BlockPad {
    fn parse(raw: Option<&str>) -> Result<Self, String> {
        let Some(s) = raw.map(str::trim).filter(|v| !v.is_empty()) else {
            return Ok(Self::Pkcs7);
        };
        match s.to_ascii_lowercase().as_str() {
            "pkcs7" | "pkcs5" => Ok(Self::Pkcs7),
            "zero" | "zeropadding" => Ok(Self::Zero),
            "none" | "nopadding" => Ok(Self::None),
            other => Err(format!("不支持的块填充模式: '{other}'")),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AesRequest {
    pub mode: Option<String>,
    pub key: String,
    pub iv: Option<String>,
    pub aad: Option<String>,
    pub data: String,
    pub padding: Option<String>,
    pub key_encoding: Option<String>,
    pub iv_encoding: Option<String>,
    pub aad_encoding: Option<String>,
    pub input_encoding: Option<String>,
    pub output_encoding: Option<String>,
}

fn cbc_encrypt_with_cipher<C>(
    key: &[u8],
    iv: &[u8],
    plain: &[u8],
    pad: BlockPad,
) -> Result<Vec<u8>, String>
where
    C: cipher::BlockCipherEncrypt,
    cbc::Encryptor<C>: KeyIvInit + BlockModeEncrypt,
{
    let enc = cbc::Encryptor::<C>::new_from_slices(key, iv)
        .map_err(|_| "AES-CBC 密钥或 IV 长度无效 (IV 需为 16 字节)".to_string())?;
    Ok(match pad {
        BlockPad::Pkcs7 => enc.encrypt_padded_vec::<Pkcs7>(plain),
        BlockPad::Zero => enc.encrypt_padded_vec::<ZeroPadding>(plain),
        BlockPad::None => {
            if !plain.len().is_multiple_of(16) {
                return Err("AES-CBC NoPadding 要求输入长度为 16 字节整数倍".into());
            }
            enc.encrypt_padded_vec::<NoPadding>(plain)
        }
    })
}

fn cbc_decrypt_with_cipher<C>(
    key: &[u8],
    iv: &[u8],
    ciphertext: &[u8],
    pad: BlockPad,
) -> Result<Vec<u8>, String>
where
    C: cipher::BlockCipherDecrypt,
    cbc::Decryptor<C>: KeyIvInit + BlockModeDecrypt,
{
    let dec = cbc::Decryptor::<C>::new_from_slices(key, iv)
        .map_err(|_| "AES-CBC 密钥或 IV 长度无效 (IV 需为 16 字节)".to_string())?;
    match pad {
        BlockPad::Pkcs7 => dec
            .decrypt_padded_vec::<Pkcs7>(ciphertext)
            .map_err(|_| "AES-CBC PKCS7 解密填充校验失败".into()),
        BlockPad::Zero => dec
            .decrypt_padded_vec::<ZeroPadding>(ciphertext)
            .map_err(|_| "AES-CBC ZeroPadding 解密失败".into()),
        BlockPad::None => dec
            .decrypt_padded_vec::<NoPadding>(ciphertext)
            .map_err(|_| "AES-CBC NoPadding 解密失败（密文需为 16 字节整数倍）".into()),
    }
}

fn ecb_encrypt_with_cipher<C>(key: &[u8], plain: &[u8], pad: BlockPad) -> Result<Vec<u8>, String>
where
    C: cipher::BlockCipherEncrypt + KeyInit,
    ecb::Encryptor<C>: KeyInit + BlockModeEncrypt,
{
    let enc =
        ecb::Encryptor::<C>::new_from_slice(key).map_err(|_| "AES-ECB 密钥长度无效".to_string())?;
    Ok(match pad {
        BlockPad::Pkcs7 => enc.encrypt_padded_vec::<Pkcs7>(plain),
        BlockPad::Zero => enc.encrypt_padded_vec::<ZeroPadding>(plain),
        BlockPad::None => {
            if !plain.len().is_multiple_of(16) {
                return Err("AES-ECB NoPadding 要求输入长度为 16 字节整数倍".into());
            }
            enc.encrypt_padded_vec::<NoPadding>(plain)
        }
    })
}

fn ecb_decrypt_with_cipher<C>(
    key: &[u8],
    ciphertext: &[u8],
    pad: BlockPad,
) -> Result<Vec<u8>, String>
where
    C: cipher::BlockCipherDecrypt + KeyInit,
    ecb::Decryptor<C>: KeyInit + BlockModeDecrypt,
{
    let dec =
        ecb::Decryptor::<C>::new_from_slice(key).map_err(|_| "AES-ECB 密钥长度无效".to_string())?;
    match pad {
        BlockPad::Pkcs7 => dec
            .decrypt_padded_vec::<Pkcs7>(ciphertext)
            .map_err(|_| "AES-ECB PKCS7 解密填充校验失败".into()),
        BlockPad::Zero => dec
            .decrypt_padded_vec::<ZeroPadding>(ciphertext)
            .map_err(|_| "AES-ECB ZeroPadding 解密失败".into()),
        BlockPad::None => dec
            .decrypt_padded_vec::<NoPadding>(ciphertext)
            .map_err(|_| "AES-ECB NoPadding 解密失败（密文需为 16 字节整数倍）".into()),
    }
}

pub fn aes_encrypt(req: AesRequest) -> Result<String, String> {
    let mode = req
        .mode
        .as_deref()
        .unwrap_or("cbc")
        .trim()
        .to_ascii_lowercase();
    let key_enc = ByteEncoding::parse(req.key_encoding.as_deref(), ByteEncoding::Utf8)?;
    let in_enc = ByteEncoding::parse(req.input_encoding.as_deref(), ByteEncoding::Utf8)?;
    let out_enc = ByteEncoding::parse(req.output_encoding.as_deref(), ByteEncoding::Base64)?;
    let key = decode_bytes(&req.key, key_enc)?;
    let plain = decode_bytes(&req.data, in_enc)?;

    let out = match mode.as_str() {
        "cbc" | "aes-cbc" => {
            let iv_raw = req
                .iv
                .as_deref()
                .ok_or_else(|| "AES-CBC 需要提供 16 字节 iv".to_string())?;
            let iv_enc = ByteEncoding::parse(req.iv_encoding.as_deref(), ByteEncoding::Utf8)?;
            let iv = decode_bytes(iv_raw, iv_enc)?;
            let pad = BlockPad::parse(req.padding.as_deref())?;
            match key.len() {
                16 => cbc_encrypt_with_cipher::<Aes128>(&key, &iv, &plain, pad)?,
                24 => cbc_encrypt_with_cipher::<Aes192>(&key, &iv, &plain, pad)?,
                32 => cbc_encrypt_with_cipher::<Aes256>(&key, &iv, &plain, pad)?,
                n => return Err(format!("AES 密钥长度必须为 16/24/32 字节，当前为 {n} 字节")),
            }
        }
        "ecb" | "aes-ecb" => {
            let pad = BlockPad::parse(req.padding.as_deref())?;
            match key.len() {
                16 => ecb_encrypt_with_cipher::<Aes128>(&key, &plain, pad)?,
                24 => ecb_encrypt_with_cipher::<Aes192>(&key, &plain, pad)?,
                32 => ecb_encrypt_with_cipher::<Aes256>(&key, &plain, pad)?,
                n => return Err(format!("AES 密钥长度必须为 16/24/32 字节，当前为 {n} 字节")),
            }
        }
        "gcm" | "aes-gcm" => {
            let iv_raw = req
                .iv
                .as_deref()
                .ok_or_else(|| "AES-GCM 需要提供 12 字节 iv/nonce".to_string())?;
            let iv_enc = ByteEncoding::parse(req.iv_encoding.as_deref(), ByteEncoding::Utf8)?;
            let iv = decode_bytes(iv_raw, iv_enc)?;
            if iv.len() != 12 {
                return Err(format!(
                    "AES-GCM nonce 长度必须为 12 字节，当前为 {} 字节",
                    iv.len()
                ));
            }
            let aad = match req.aad {
                Some(ref raw_aad) => {
                    let aad_enc =
                        ByteEncoding::parse(req.aad_encoding.as_deref(), ByteEncoding::Utf8)?;
                    decode_bytes(raw_aad, aad_enc)?
                }
                None => Vec::new(),
            };
            let nonce = Nonce::try_from(iv.as_slice())
                .map_err(|_| "AES-GCM nonce 长度必须为 12 字节".to_string())?;
            let payload = Payload {
                msg: &plain,
                aad: &aad,
            };
            match key.len() {
                16 => Aes128Gcm::new_from_slice(&key)
                    .map_err(|_| "初始化 AES-128-GCM 失败".to_string())?
                    .encrypt(&nonce, payload)
                    .map_err(|_| "AES-128-GCM 加密失败".to_string())?,
                32 => Aes256Gcm::new_from_slice(&key)
                    .map_err(|_| "初始化 AES-256-GCM 失败".to_string())?
                    .encrypt(&nonce, payload)
                    .map_err(|_| "AES-256-GCM 加密失败".to_string())?,
                n => {
                    return Err(format!(
                        "AES-GCM 密钥长度必须为 16 或 32 字节，当前为 {n} 字节"
                    ));
                }
            }
        }
        other => return Err(format!("不支持的 AES 模式: '{other}'")),
    };

    encode_bytes(&out, out_enc)
}

pub fn aes_decrypt(req: AesRequest) -> Result<String, String> {
    let mode = req
        .mode
        .as_deref()
        .unwrap_or("cbc")
        .trim()
        .to_ascii_lowercase();
    let key_enc = ByteEncoding::parse(req.key_encoding.as_deref(), ByteEncoding::Utf8)?;
    let in_enc = ByteEncoding::parse(req.input_encoding.as_deref(), ByteEncoding::Base64)?;
    let out_enc = ByteEncoding::parse(req.output_encoding.as_deref(), ByteEncoding::Utf8)?;
    let key = decode_bytes(&req.key, key_enc)?;
    let ciphertext = decode_bytes(&req.data, in_enc)?;

    let out = match mode.as_str() {
        "cbc" | "aes-cbc" => {
            let iv_raw = req
                .iv
                .as_deref()
                .ok_or_else(|| "AES-CBC 需要提供 16 字节 iv".to_string())?;
            let iv_enc = ByteEncoding::parse(req.iv_encoding.as_deref(), ByteEncoding::Utf8)?;
            let iv = decode_bytes(iv_raw, iv_enc)?;
            let pad = BlockPad::parse(req.padding.as_deref())?;
            match key.len() {
                16 => cbc_decrypt_with_cipher::<Aes128>(&key, &iv, &ciphertext, pad)?,
                24 => cbc_decrypt_with_cipher::<Aes192>(&key, &iv, &ciphertext, pad)?,
                32 => cbc_decrypt_with_cipher::<Aes256>(&key, &iv, &ciphertext, pad)?,
                n => return Err(format!("AES 密钥长度必须为 16/24/32 字节，当前为 {n} 字节")),
            }
        }
        "ecb" | "aes-ecb" => {
            let pad = BlockPad::parse(req.padding.as_deref())?;
            match key.len() {
                16 => ecb_decrypt_with_cipher::<Aes128>(&key, &ciphertext, pad)?,
                24 => ecb_decrypt_with_cipher::<Aes192>(&key, &ciphertext, pad)?,
                32 => ecb_decrypt_with_cipher::<Aes256>(&key, &ciphertext, pad)?,
                n => return Err(format!("AES 密钥长度必须为 16/24/32 字节，当前为 {n} 字节")),
            }
        }
        "gcm" | "aes-gcm" => {
            let iv_raw = req
                .iv
                .as_deref()
                .ok_or_else(|| "AES-GCM 需要提供 12 字节 iv/nonce".to_string())?;
            let iv_enc = ByteEncoding::parse(req.iv_encoding.as_deref(), ByteEncoding::Utf8)?;
            let iv = decode_bytes(iv_raw, iv_enc)?;
            if iv.len() != 12 {
                return Err(format!(
                    "AES-GCM nonce 长度必须为 12 字节，当前为 {} 字节",
                    iv.len()
                ));
            }
            let aad = match req.aad {
                Some(ref raw_aad) => {
                    let aad_enc =
                        ByteEncoding::parse(req.aad_encoding.as_deref(), ByteEncoding::Utf8)?;
                    decode_bytes(raw_aad, aad_enc)?
                }
                None => Vec::new(),
            };
            let nonce = Nonce::try_from(iv.as_slice())
                .map_err(|_| "AES-GCM nonce 长度必须为 12 字节".to_string())?;
            let payload = Payload {
                msg: &ciphertext,
                aad: &aad,
            };
            match key.len() {
                16 => Aes128Gcm::new_from_slice(&key)
                    .map_err(|_| "初始化 AES-128-GCM 失败".to_string())?
                    .decrypt(&nonce, payload)
                    .map_err(|_| "AES-128-GCM 解密或认证标签校验失败".to_string())?,
                32 => Aes256Gcm::new_from_slice(&key)
                    .map_err(|_| "初始化 AES-256-GCM 失败".to_string())?
                    .decrypt(&nonce, payload)
                    .map_err(|_| "AES-256-GCM 解密或认证标签校验失败".to_string())?,
                n => {
                    return Err(format!(
                        "AES-GCM 密钥长度必须为 16 或 32 字节，当前为 {n} 字节"
                    ));
                }
            }
        }
        other => return Err(format!("不支持的 AES 模式: '{other}'")),
    };

    encode_bytes(&out, out_enc)
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RsaEncryptRequest {
    pub public_key_pem: Option<String>,
    pub modulus_hex: Option<String>,
    pub exponent_hex: Option<String>,
    pub data: String,
    pub padding: Option<String>,
    pub input_encoding: Option<String>,
    pub output_encoding: Option<String>,
}

/// Pure safe big-endian limb arithmetic (`u32` base `2^32`) for unpadded `base^exp mod modulus`
/// (supports arbitrary bit lengths including 1024-bit NetEase `weapi` keys).
mod big_modpow {
    fn from_be_bytes(bytes: &[u8]) -> Vec<u32> {
        let mut limbs = Vec::with_capacity(bytes.len().div_ceil(4));
        for chunk in bytes.rchunks(4) {
            let mut val = 0u32;
            for &b in chunk {
                val = (val << 8) | (b as u32);
            }
            limbs.push(val);
        }
        trim(&mut limbs);
        limbs
    }

    fn to_be_bytes_padded(limbs: &[u32], out_len: usize) -> Vec<u8> {
        let mut raw = Vec::with_capacity(limbs.len() * 4);
        for &limb in limbs.iter().rev() {
            raw.extend_from_slice(&limb.to_be_bytes());
        }
        let stripped = match raw.iter().position(|&b| b != 0) {
            Some(idx) => &raw[idx..],
            None => &[],
        };
        if stripped.len() >= out_len {
            stripped[stripped.len() - out_len..].to_vec()
        } else {
            let mut out = vec![0u8; out_len - stripped.len()];
            out.extend_from_slice(stripped);
            out
        }
    }

    fn trim(a: &mut Vec<u32>) {
        while a.last() == Some(&0) {
            a.pop();
        }
    }

    fn cmp(a: &[u32], b: &[u32]) -> std::cmp::Ordering {
        a.len().cmp(&b.len()).then_with(|| {
            for (x, y) in a.iter().rev().zip(b.iter().rev()) {
                match x.cmp(y) {
                    std::cmp::Ordering::Equal => continue,
                    non_eq => return non_eq,
                }
            }
            std::cmp::Ordering::Equal
        })
    }

    fn sub_assign(a: &mut Vec<u32>, b: &[u32]) {
        let mut borrow = 0i64;
        for (i, slot) in a.iter_mut().enumerate() {
            let bv = b.get(i).copied().unwrap_or(0) as i64;
            let diff = (*slot as i64) - bv - borrow;
            if diff < 0 {
                *slot = (diff + (1i64 << 32)) as u32;
                borrow = 1;
            } else {
                *slot = diff as u32;
                borrow = 0;
            }
        }
        trim(a);
    }

    fn shl1_add_bit(a: &mut Vec<u32>, bit: u32) {
        let mut carry = bit as u64;
        for slot in a.iter_mut() {
            let next = ((*slot as u64) << 1) | carry;
            *slot = next as u32;
            carry = next >> 32;
        }
        if carry != 0 {
            a.push(carry as u32);
        }
    }

    fn rem(a: &[u32], m: &[u32]) -> Vec<u32> {
        let mut r: Vec<u32> = Vec::with_capacity(m.len() + 1);
        for &limb in a.iter().rev() {
            for shift in (0..32).rev() {
                let bit = (limb >> shift) & 1;
                shl1_add_bit(&mut r, bit);
                if cmp(&r, m) != std::cmp::Ordering::Less {
                    sub_assign(&mut r, m);
                }
            }
        }
        r
    }

    fn mul_mod(a: &[u32], b: &[u32], m: &[u32]) -> Vec<u32> {
        if a.is_empty() || b.is_empty() {
            return Vec::new();
        }
        let mut prod = vec![0u32; a.len() + b.len()];
        for (i, &av) in a.iter().enumerate() {
            let mut carry = 0u64;
            for (j, &bv) in b.iter().enumerate() {
                let sum = (prod[i + j] as u64) + (av as u64) * (bv as u64) + carry;
                prod[i + j] = sum as u32;
                carry = sum >> 32;
            }
            prod[i + b.len()] = carry as u32;
        }
        trim(&mut prod);
        rem(&prod, m)
    }

    pub(super) fn mod_pow_be(base: &[u8], exp: &[u8], modulus: &[u8]) -> Result<Vec<u8>, String> {
        let mod_stripped = match modulus.iter().position(|&b| b != 0) {
            Some(idx) => &modulus[idx..],
            None => return Err("RSA 模数不能为 0".into()),
        };
        if mod_stripped.len() > super::MAX_RAW_RSA_MODULUS_BYTES {
            return Err("RSA 模数超过长度上限".into());
        }
        let out_len = mod_stripped.len();
        let m = from_be_bytes(mod_stripped);
        let mut result = vec![1u32];
        if cmp(&result, &m) != std::cmp::Ordering::Less {
            result = rem(&result, &m);
        }
        let mut b = rem(&from_be_bytes(base), &m);

        let exp_stripped = match exp.iter().position(|&x| x != 0) {
            Some(idx) => &exp[idx..],
            None => return Ok(to_be_bytes_padded(&result, out_len)),
        };

        for &byte in exp_stripped.iter().rev() {
            for bit_idx in 0..8 {
                if ((byte >> bit_idx) & 1) == 1 {
                    result = mul_mod(&result, &b, &m);
                }
                b = mul_mod(&b, &b, &m);
            }
        }
        Ok(to_be_bytes_padded(&result, out_len))
    }
}

fn parse_rsa_public_key(req: &RsaEncryptRequest) -> Result<PublicKey, String> {
    if let Some(pem) = req
        .public_key_pem
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return PublicKey::from_public_key_pem(pem)
            .or_else(|_| PublicKey::from_pkcs1_pem(pem))
            .map_err(|e| format!("解析 RSA PEM 公钥失败: {e}"));
    }
    let mod_hex = req
        .modulus_hex
        .as_deref()
        .ok_or_else(|| "RSA 加密需要提供 publicKeyPem 或 modulusHex".to_string())?;
    let exp_hex = req.exponent_hex.as_deref().unwrap_or("010001");
    let n_bytes = decode_bytes(mod_hex, ByteEncoding::Hex)?;
    let e_bytes = decode_bytes(exp_hex, ByteEncoding::Hex)?;
    let n_bits = (n_bytes.len() * 8).max(64) as u32;
    let e_bits = (e_bytes.len() * 8).max(64) as u32;
    let n = BoxedUint::from_be_slice(&n_bytes, n_bits)
        .map_err(|e| format!("解析 RSA 模数失败: {e}"))?;
    let e = BoxedUint::from_be_slice(&e_bytes, e_bits)
        .map_err(|e| format!("解析 RSA 指数失败: {e}"))?;
    PublicKey::new_with_size_limits(n, e, 512..=8192).map_err(|e| format!("构造 RSA 公钥失败: {e}"))
}

pub fn rsa_encrypt(req: RsaEncryptRequest) -> Result<String, String> {
    let padding = req
        .padding
        .as_deref()
        .unwrap_or("pkcs1v15")
        .trim()
        .to_ascii_lowercase();
    let in_enc = ByteEncoding::parse(req.input_encoding.as_deref(), ByteEncoding::Utf8)?;
    let default_out = if padding == "none" || padding == "raw" {
        ByteEncoding::Hex
    } else {
        ByteEncoding::Base64
    };
    let out_enc = ByteEncoding::parse(req.output_encoding.as_deref(), default_out)?;
    let plain = decode_bytes(&req.data, in_enc)?;

    let out = match padding.as_str() {
        "none" | "raw" | "nopadding" => {
            if let Some(mod_hex) = req
                .modulus_hex
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                let exp_hex = req.exponent_hex.as_deref().unwrap_or("010001");
                let n_bytes = decode_bytes(mod_hex, ByteEncoding::Hex)?;
                let e_bytes = decode_bytes(exp_hex, ByteEncoding::Hex)?;
                big_modpow::mod_pow_be(&plain, &e_bytes, &n_bytes)?
            } else {
                let pub_key = parse_rsa_public_key(&req)?;
                use rsa::traits::PublicKeyParts;
                let n_bytes = pub_key.n().to_be_bytes_trimmed_vartime();
                let e_bytes = pub_key.e().to_be_bytes_trimmed_vartime();
                big_modpow::mod_pow_be(&plain, &e_bytes, &n_bytes)?
            }
        }
        "pkcs1v15" | "pkcs1" => {
            let pub_key = parse_rsa_public_key(&req)?;
            #[allow(deprecated)]
            pub_key
                .pkcs1v15_encrypt(&plain)
                .map_err(|e| format!("RSA-PKCS1v15 加密失败: {e}"))?
        }
        "oaep" | "oaep-sha256" => {
            let pub_key = parse_rsa_public_key(&req)?;
            pub_key
                .oaep_encrypt::<Sha256>(&plain)
                .map_err(|e| format!("RSA-OAEP-SHA256 加密失败: {e}"))?
        }
        other => return Err(format!("不支持的 RSA 填充模式: '{other}'")),
    };

    encode_bytes(&out, out_enc)
}

pub fn random_bytes_encoded(length: usize, output_enc: Option<&str>) -> Result<String, String> {
    if length == 0 || length > MAX_RANDOM_BYTES {
        return Err(format!("随机字节长度需在 1..={MAX_RANDOM_BYTES} 之间"));
    }
    let out_enc = ByteEncoding::parse(output_enc, ByteEncoding::Hex)?;
    let mut buf = vec![0u8; length];
    getrandom::fill(&mut buf).map_err(|e| format!("生成随机字节失败: {e}"))?;
    encode_bytes(&buf, out_enc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_hmacs_aes_cbc_ecb_gcm_and_raw_rsa_roundtrip() {
        assert_eq!(
            hash_digest("sha256", "hello", None, None).unwrap(),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert_eq!(
            hmac_digest("sha256", "secret", "message", HmacOptions::default()).unwrap(),
            "8b5f48702995c1598c573db1e21866a9b825d4a794d169d7060a03605796360b"
        );

        // AES-128-CBC roundtrip (NetEase weapi parameters)
        let cbc_ct = aes_encrypt(AesRequest {
            mode: Some("cbc".into()),
            key: "0CoJUm6Qyw8W8jud".into(),
            iv: Some("0102030405060708".into()),
            data: r#"{"csrf_token":""}"#.into(),
            ..Default::default()
        })
        .unwrap();
        let cbc_pt = aes_decrypt(AesRequest {
            mode: Some("cbc".into()),
            key: "0CoJUm6Qyw8W8jud".into(),
            iv: Some("0102030405060708".into()),
            data: cbc_ct,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(cbc_pt, r#"{"csrf_token":""}"#);

        // AES-128-ECB roundtrip (NetEase eapi parameters)
        let ecb_hex = aes_encrypt(AesRequest {
            mode: Some("ecb".into()),
            key: "e82ckenh8dichen8".into(),
            data: "/api/song/enhance/player/url-36cd479b6b5-payload".into(),
            output_encoding: Some("hex".into()),
            ..Default::default()
        })
        .unwrap();
        let ecb_pt = aes_decrypt(AesRequest {
            mode: Some("ecb".into()),
            key: "e82ckenh8dichen8".into(),
            data: ecb_hex,
            input_encoding: Some("hex".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(ecb_pt, "/api/song/enhance/player/url-36cd479b6b5-payload");

        // AES-256-GCM roundtrip
        let gcm_ct = aes_encrypt(AesRequest {
            mode: Some("gcm".into()),
            key: "0123456789abcdef0123456789abcdef".into(),
            iv: Some("unique12byte".into()),
            aad: Some("plugin-v2".into()),
            data: "lossless-flac-token".into(),
            ..Default::default()
        })
        .unwrap();
        let gcm_pt = aes_decrypt(AesRequest {
            mode: Some("gcm".into()),
            key: "0123456789abcdef0123456789abcdef".into(),
            iv: Some("unique12byte".into()),
            aad: Some("plugin-v2".into()),
            data: gcm_ct,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(gcm_pt, "lossless-flac-token");

        // Raw RSA (padding = "none") modular exponentiation: 5^3 mod 33 = 125 mod 33 = 26 (0x1a)
        let raw_rsa = rsa_encrypt(RsaEncryptRequest {
            modulus_hex: Some("0021".into()),
            exponent_hex: Some("03".into()),
            data: "05".into(),
            padding: Some("none".into()),
            input_encoding: Some("hex".into()),
            output_encoding: Some("hex".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(raw_rsa, "1a");
    }
}
