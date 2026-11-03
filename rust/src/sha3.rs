//! SHA3-256, ported from `src/tiny_sha3` (Markku-Juhani O. Saarinen, MIT
//! license; see `src/tiny_sha3/LICENSE.orig`), used for signed data URLs.

const KECCAKF_ROUNDS: usize = 24;

const RNDC: [u64; 24] = [
    0x0000000000000001, 0x0000000000008082, 0x800000000000808a, 0x8000000080008000,
    0x000000000000808b, 0x0000000080000001, 0x8000000080008081, 0x8000000000008009,
    0x000000000000008a, 0x0000000000000088, 0x0000000080008009, 0x000000008000000a,
    0x000000008000808b, 0x800000000000008b, 0x8000000000008089, 0x8000000000008003,
    0x8000000000008002, 0x8000000000000080, 0x000000000000800a, 0x800000008000000a,
    0x8000000080008081, 0x8000000000008080, 0x0000000080000001, 0x8000000080008008,
];
const ROTC: [u32; 24] = [
    1, 3, 6, 10, 15, 21, 28, 36, 45, 55, 2, 14, 27, 41, 56, 8, 25, 43, 62, 18, 39, 61, 20, 44,
];
const PILN: [usize; 24] = [
    10, 7, 11, 17, 18, 3, 5, 16, 8, 21, 24, 4, 15, 23, 19, 13, 12, 2, 20, 14, 22, 9, 6, 1,
];

fn keccakf(st: &mut [u64; 25]) {
    let mut bc = [0u64; 5];
    for round in RNDC.iter().take(KECCAKF_ROUNDS) {
        for i in 0..5 {
            bc[i] = st[i] ^ st[i + 5] ^ st[i + 10] ^ st[i + 15] ^ st[i + 20];
        }
        for i in 0..5 {
            let t = bc[(i + 4) % 5] ^ bc[(i + 1) % 5].rotate_left(1);
            for j in (0..25).step_by(5) {
                st[j + i] ^= t;
            }
        }
        let mut t = st[1];
        for i in 0..24 {
            let j = PILN[i];
            bc[0] = st[j];
            st[j] = t.rotate_left(ROTC[i]);
            t = bc[0];
        }
        for j in (0..25).step_by(5) {
            bc.copy_from_slice(&st[j..j + 5]);
            for i in 0..5 {
                st[j + i] ^= (!bc[(i + 1) % 5]) & bc[(i + 2) % 5];
            }
        }
        st[0] ^= round;
    }
}

/// SHA3-256 digest of `data`.
pub fn sha3_256(data: &[u8]) -> [u8; 32] {
    const MDLEN: usize = 32;
    const RSIZ: usize = 200 - 2 * MDLEN;
    let mut bytes = [0u8; 200];
    let mut st = [0u64; 25];

    let load = |bytes: &[u8; 200], st: &mut [u64; 25]| {
        for (i, w) in st.iter_mut().enumerate() {
            *w = u64::from_le_bytes(bytes[8 * i..8 * i + 8].try_into().unwrap());
        }
    };
    let store = |st: &[u64; 25], bytes: &mut [u8; 200]| {
        for (i, w) in st.iter().enumerate() {
            bytes[8 * i..8 * i + 8].copy_from_slice(&w.to_le_bytes());
        }
    };

    let mut pt = 0;
    for &b in data {
        bytes[pt] ^= b;
        pt += 1;
        if pt >= RSIZ {
            load(&bytes, &mut st);
            keccakf(&mut st);
            store(&st, &mut bytes);
            pt = 0;
        }
    }
    bytes[pt] ^= 0x06;
    bytes[RSIZ - 1] ^= 0x80;
    load(&bytes, &mut st);
    keccakf(&mut st);
    store(&st, &mut bytes);

    let mut md = [0u8; MDLEN];
    md.copy_from_slice(&bytes[..MDLEN]);
    md
}

#[cfg(test)]
mod tests {
    fn hex(d: &[u8]) -> String {
        d.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn known_vectors() {
        assert_eq!(
            hex(&super::sha3_256(b"")),
            "a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a"
        );
        assert_eq!(
            hex(&super::sha3_256(b"abc")),
            "3a985da74fe225b2045c172d6bd390bd855f086e3e9d525b46bfe24511431532"
        );
        // Longer than one block (136 bytes).
        let long = vec![b'a'; 200];
        assert_eq!(
            hex(&super::sha3_256(&long)),
            "cce34485baf2bf2aca99b94833892a4f52896d3d153f7b840cc4f9fe695f1387"
        );
    }
}
