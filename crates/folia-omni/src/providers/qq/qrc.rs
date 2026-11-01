// QRC lyric decryption (`qrcDecrypt`): QQ Music's own triple DES over the hex text, then zlib.

use std::io::Read;

use crate::OmniError;

const KEY: &[u8; 24] = b"!@#)(*$%123ZXC!@!@#)(NHL";

/// The S-boxes as the original writes them, its two irregular entries included.
const SBOX: [[u8; 64]; 8] = [
    [
        14, 4, 13, 1, 2, 15, 11, 8, 3, 10, 6, 12, 5, 9, 0, 7, 0, 15, 7, 4, 14, 2, 13, 1, 10, 6, 12, 11, 9, 5, 3, 8, 4,
        1, 14, 8, 13, 6, 2, 11, 15, 12, 9, 7, 3, 10, 5, 0, 15, 12, 8, 2, 4, 9, 1, 7, 5, 11, 3, 14, 10, 0, 6, 13,
    ],
    [
        15, 1, 8, 14, 6, 11, 3, 4, 9, 7, 2, 13, 12, 0, 5, 10, 3, 13, 4, 7, 15, 2, 8, 15, 12, 0, 1, 10, 6, 9, 11, 5, 0,
        14, 7, 11, 10, 4, 13, 1, 5, 8, 12, 6, 9, 3, 2, 15, 13, 8, 10, 1, 3, 15, 4, 2, 11, 6, 7, 12, 0, 5, 14, 9,
    ],
    [
        10, 0, 9, 14, 6, 3, 15, 5, 1, 13, 12, 7, 11, 4, 2, 8, 13, 7, 0, 9, 3, 4, 6, 10, 2, 8, 5, 14, 12, 11, 15, 1, 13,
        6, 4, 9, 8, 15, 3, 0, 11, 1, 2, 12, 5, 10, 14, 7, 1, 10, 13, 0, 6, 9, 8, 7, 4, 15, 14, 3, 11, 5, 2, 12,
    ],
    [
        7, 13, 14, 3, 0, 6, 9, 10, 1, 2, 8, 5, 11, 12, 4, 15, 13, 8, 11, 5, 6, 15, 0, 3, 4, 7, 2, 12, 1, 10, 14, 9, 10,
        6, 9, 0, 12, 11, 7, 13, 15, 1, 3, 14, 5, 2, 8, 4, 3, 15, 0, 6, 10, 10, 13, 8, 9, 4, 5, 11, 12, 7, 2, 14,
    ],
    [
        2, 12, 4, 1, 7, 10, 11, 6, 8, 5, 3, 15, 13, 0, 14, 9, 14, 11, 2, 12, 4, 7, 13, 1, 5, 0, 15, 10, 3, 9, 8, 6, 4,
        2, 1, 11, 10, 13, 7, 8, 15, 9, 12, 5, 6, 3, 0, 14, 11, 8, 12, 7, 1, 14, 2, 13, 6, 15, 0, 9, 10, 4, 5, 3,
    ],
    [
        12, 1, 10, 15, 9, 2, 6, 8, 0, 13, 3, 4, 14, 7, 5, 11, 10, 15, 4, 2, 7, 12, 9, 5, 6, 1, 13, 14, 0, 11, 3, 8, 9,
        14, 15, 5, 2, 8, 12, 3, 7, 0, 4, 10, 1, 13, 11, 6, 4, 3, 2, 12, 9, 5, 15, 10, 11, 14, 1, 7, 6, 0, 8, 13,
    ],
    [
        4, 11, 2, 14, 15, 0, 8, 13, 3, 12, 9, 7, 5, 10, 6, 1, 13, 0, 11, 7, 4, 9, 1, 10, 14, 3, 5, 12, 2, 15, 8, 6, 1,
        4, 11, 13, 12, 3, 7, 14, 10, 15, 6, 8, 0, 5, 9, 2, 6, 11, 13, 8, 1, 4, 10, 7, 9, 5, 0, 15, 14, 2, 3, 12,
    ],
    [
        13, 2, 8, 4, 6, 15, 11, 1, 10, 9, 3, 14, 5, 0, 12, 7, 1, 15, 13, 8, 10, 3, 7, 4, 12, 5, 6, 11, 0, 14, 9, 2, 7,
        11, 4, 1, 9, 12, 14, 2, 0, 6, 10, 13, 15, 3, 5, 8, 2, 1, 14, 7, 4, 10, 8, 13, 15, 12, 9, 0, 3, 5, 6, 11,
    ],
];

type Schedule = [[u8; 6]; 16];

/// `qrcDecrypt`: hex text in, the plain QRC (or LRC) text out.
pub fn decrypt(hex: &str) -> Result<String, OmniError> {
    let hex = hex.trim();
    let bytes: Vec<u8> = (0..hex.len() / 2)
        .filter_map(|index| u8::from_str_radix(hex.get(index * 2..index * 2 + 2)?, 16).ok())
        .collect();
    if bytes.is_empty() {
        return Err(OmniError::Response("qq lyric is empty".to_owned()));
    }
    let schedules =
        [key_schedule(&KEY[16..24], false), key_schedule(&KEY[8..16], true), key_schedule(&KEY[0..8], false)];
    let mut plain = Vec::with_capacity(bytes.len());
    for chunk in bytes.chunks(8) {
        let mut block = [0u8; 8];
        block[..chunk.len()].copy_from_slice(chunk);
        for schedule in &schedules {
            block = crypt(&block, schedule);
        }
        plain.extend_from_slice(&block[..chunk.len()]);
    }
    let mut text = String::new();
    flate2::read::ZlibDecoder::new(plain.as_slice())
        .read_to_string(&mut text)
        .map_err(|err| OmniError::Response(format!("qq lyric does not inflate: {err}")))?;
    Ok(text)
}

fn bitnum(a: &[u8], b: usize, c: u32) -> u32 {
    let index = (b / 32) * 4 + 3 - (b % 32) / 8;
    let shift = 7 - (b % 8);
    (u32::from(a[index] >> shift) & 1) << c
}

fn bitnum_intr(a: u32, b: u32, c: u32) -> u32 {
    ((a >> (31 - b)) & 1) << c
}

fn bitnum_intl(a: u32, b: u32, c: u32) -> u32 {
    (a.wrapping_shl(b) & 0x8000_0000) >> c
}

fn sbox_bit(a: u8) -> usize {
    usize::from((a & 32) | ((a & 31) >> 1) | ((a & 1) << 4))
}

fn initial_permutation(input: &[u8; 8]) -> (u32, u32) {
    let pick = |order: [usize; 32]| {
        order.iter().enumerate().fold(0u32, |acc, (i, &bit)| acc | bitnum(input, bit, 31 - i as u32))
    };
    let s0 = pick([
        57, 49, 41, 33, 25, 17, 9, 1, 59, 51, 43, 35, 27, 19, 11, 3, 61, 53, 45, 37, 29, 21, 13, 5, 63, 55, 47, 39, 31,
        23, 15, 7,
    ]);
    let s1 = pick([
        56, 48, 40, 32, 24, 16, 8, 0, 58, 50, 42, 34, 26, 18, 10, 2, 60, 52, 44, 36, 28, 20, 12, 4, 62, 54, 46, 38, 30,
        22, 14, 6,
    ]);
    (s0, s1)
}

fn inverse_permutation(s0: u32, s1: u32) -> [u8; 8] {
    let byte = |base: u32| {
        (bitnum_intr(s1, base, 7)
            | bitnum_intr(s0, base, 6)
            | bitnum_intr(s1, base + 8, 5)
            | bitnum_intr(s0, base + 8, 4)
            | bitnum_intr(s1, base + 16, 3)
            | bitnum_intr(s0, base + 16, 2)
            | bitnum_intr(s1, base + 24, 1)
            | bitnum_intr(s0, base + 24, 0)) as u8
    };
    [byte(4), byte(5), byte(6), byte(7), byte(0), byte(1), byte(2), byte(3)]
}

fn f(state: u32, key: &[u8; 6]) -> u32 {
    let t1 = bitnum_intl(state, 31, 0)
        | ((state & 0xf000_0000) >> 1)
        | bitnum_intl(state, 4, 5)
        | bitnum_intl(state, 3, 6)
        | ((state & 0x0f00_0000) >> 3)
        | bitnum_intl(state, 8, 11)
        | bitnum_intl(state, 7, 12)
        | ((state & 0x00f0_0000) >> 5)
        | bitnum_intl(state, 12, 17)
        | bitnum_intl(state, 11, 18)
        | ((state & 0x000f_0000) >> 7)
        | bitnum_intl(state, 16, 23);
    let t2 = bitnum_intl(state, 15, 0)
        | ((state & 0x0000_f000) << 15)
        | bitnum_intl(state, 20, 5)
        | bitnum_intl(state, 19, 6)
        | ((state & 0x0000_0f00) << 13)
        | bitnum_intl(state, 24, 11)
        | bitnum_intl(state, 23, 12)
        | ((state & 0x0000_00f0) << 11)
        | bitnum_intl(state, 28, 17)
        | bitnum_intl(state, 27, 18)
        | ((state & 0x0000_000f) << 9)
        | bitnum_intl(state, 0, 23);
    let large =
        [(t1 >> 24) as u8, (t1 >> 16) as u8, (t1 >> 8) as u8, (t2 >> 24) as u8, (t2 >> 16) as u8, (t2 >> 8) as u8];
    let x: Vec<u8> = large.iter().zip(key).map(|(a, b)| a ^ b).collect();
    let out = (u32::from(SBOX[0][sbox_bit(x[0] >> 2)]) << 28)
        | (u32::from(SBOX[1][sbox_bit(((x[0] & 0x03) << 4) | (x[1] >> 4))]) << 24)
        | (u32::from(SBOX[2][sbox_bit(((x[1] & 0x0f) << 2) | (x[2] >> 6))]) << 20)
        | (u32::from(SBOX[3][sbox_bit(x[2] & 0x3f)]) << 16)
        | (u32::from(SBOX[4][sbox_bit(x[3] >> 2)]) << 12)
        | (u32::from(SBOX[5][sbox_bit(((x[3] & 0x03) << 4) | (x[4] >> 4))]) << 8)
        | (u32::from(SBOX[6][sbox_bit(((x[4] & 0x0f) << 2) | (x[5] >> 6))]) << 4)
        | u32::from(SBOX[7][sbox_bit(x[5] & 0x3f)]);
    const P: [u32; 32] = [
        15, 6, 19, 20, 28, 11, 27, 16, 0, 14, 22, 25, 4, 17, 30, 9, 1, 7, 23, 13, 31, 26, 2, 8, 18, 12, 29, 5, 21, 10,
        3, 24,
    ];
    P.iter().enumerate().fold(0u32, |acc, (i, &bit)| acc | bitnum_intl(out, bit, i as u32))
}

fn crypt(input: &[u8; 8], key: &Schedule) -> [u8; 8] {
    let (mut s0, mut s1) = initial_permutation(input);
    for round in key.iter().take(15) {
        let previous = s1;
        s1 = f(s1, round) ^ s0;
        s0 = previous;
    }
    s0 ^= f(s1, &key[15]);
    inverse_permutation(s0, s1)
}

/// `key_schedule`; `encrypt` keeps the rounds in order, otherwise they are reversed.
fn key_schedule(key: &[u8], encrypt: bool) -> Schedule {
    const SHIFTS: [u32; 16] = [1, 1, 2, 2, 2, 2, 2, 2, 1, 2, 2, 2, 2, 2, 2, 1];
    const PERM_C: [usize; 28] =
        [56, 48, 40, 32, 24, 16, 8, 0, 57, 49, 41, 33, 25, 17, 9, 1, 58, 50, 42, 34, 26, 18, 10, 2, 59, 51, 43, 35];
    const PERM_D: [usize; 28] =
        [62, 54, 46, 38, 30, 22, 14, 6, 61, 53, 45, 37, 29, 21, 13, 5, 60, 52, 44, 36, 28, 20, 12, 4, 27, 19, 11, 3];
    const COMPRESSION: [u32; 48] = [
        13, 16, 10, 23, 0, 4, 2, 27, 14, 5, 20, 9, 22, 18, 11, 3, 25, 7, 15, 6, 26, 19, 12, 1, 40, 51, 30, 36, 46, 54,
        29, 39, 50, 44, 32, 47, 43, 48, 38, 55, 33, 52, 45, 41, 49, 35, 28, 31,
    ];
    let mut schedule = [[0u8; 6]; 16];
    let mut c = PERM_C.iter().enumerate().fold(0u32, |acc, (i, &bit)| acc | bitnum(key, bit, 31 - i as u32));
    let mut d = PERM_D.iter().enumerate().fold(0u32, |acc, (i, &bit)| acc | bitnum(key, bit, 31 - i as u32));
    for (i, shift) in SHIFTS.iter().enumerate() {
        c = (c.wrapping_shl(*shift) | (c >> (28 - shift))) & 0xffff_fff0;
        d = (d.wrapping_shl(*shift) | (d >> (28 - shift))) & 0xffff_fff0;
        let round = if encrypt { i } else { 15 - i };
        schedule[round] = [0; 6];
        for (j, &bit) in COMPRESSION.iter().enumerate() {
            let value = if j < 24 {
                bitnum_intr(c, bit, 7 - (j % 8) as u32)
            } else {
                bitnum_intr(d, bit - 27, 7 - (j % 8) as u32)
            };
            schedule[round][j / 8] |= value as u8;
        }
    }
    schedule
}
