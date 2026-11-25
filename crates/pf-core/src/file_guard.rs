//! Verrou et accès concurrent (US-17.3), sans changer le format du projet.
//!
//! - **Révision par empreinte** : SHA-256 des octets du fichier (`sha256:<hex>`),
//!   la même convention que la révision de `pf-mcp`. Le document mémorise la
//!   révision des octets lus ou écrits en dernier ; avant d'écraser, la
//!   révision du disque est relue et comparée (voir `Document::save_checked`).
//! - **Verrou** : fichier voisin `<projet>.lock` créé en exclusivité
//!   (`create_new`) et réécrit périodiquement par son propriétaire. Il contient
//!   un jeton, le numéro de processus et l'heure (secondes Unix), jamais un
//!   chemin ni un nom. Un verrou plus vieux que `LOCK_STALE_SECS` (processus
//!   arrêté brutalement) est repris. Le verrou est **consultatif** : il
//!   prévient, il n'interdit pas.

use crate::error::CoreError;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Suffixe du fichier de verrou, ajouté au nom du projet.
pub const LOCK_SUFFIX: &str = ".lock";
/// Âge (secondes) au-delà duquel un verrou non rafraîchi est considéré abandonné.
pub const LOCK_STALE_SECS: u64 = 45;
/// Période (secondes) de rafraîchissement conseillée à l'application.
pub const LOCK_REFRESH_SECS: u64 = 10;

/// Révision d'un contenu : `sha256:<64 chiffres hexadécimaux>`.
pub fn revision_of(bytes: &[u8]) -> String {
    format!("sha256:{}", sha256_hex(bytes))
}

/// Révision du fichier sur disque ; `None` s'il n'existe pas.
pub fn file_revision(path: &Path) -> Result<Option<String>, CoreError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(revision_of(&bytes))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(CoreError::Io(format!("{} : {e}", path.display()))),
    }
}

/// Emplacement du verrou d'un projet.
pub fn lock_path(project: &Path) -> PathBuf {
    let mut name = project.as_os_str().to_owned();
    name.push(LOCK_SUFFIX);
    PathBuf::from(name)
}

/// Qui tient le verrou (information affichable, sans donnée personnelle).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockHolder {
    pub pid: u32,
    /// Heure (secondes Unix) du dernier rafraîchissement.
    pub since: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockError {
    /// Le fichier est ouvert ailleurs (verrou récent d'un autre jeton).
    Held(LockHolder),
    /// Création impossible (dossier en lecture seule…) : pas de verrou.
    Io(String),
}

/// Verrou tenu par cette instance ; retiré à la destruction.
#[derive(Debug)]
pub struct FileLock {
    path: PathBuf,
    token: String,
    pid: u32,
}

fn parse_lock(text: &str) -> Option<(String, LockHolder)> {
    let mut parts = text.split_whitespace();
    let token = parts.next()?.to_string();
    let pid = parts.next()?.parse().ok()?;
    let since = parts.next()?.parse().ok()?;
    parts
        .next()
        .is_none()
        .then_some((token, LockHolder { pid, since }))
}

fn new_token(pid: u32) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!(
        "{pid}x{nanos:x}x{}",
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

impl FileLock {
    /// Prend le verrou du projet `project` à l'heure `now` (secondes Unix).
    pub fn acquire(project: &Path, now: u64) -> Result<FileLock, LockError> {
        let path = lock_path(project);
        let pid = std::process::id();
        let token = new_token(pid);
        let content = format!("{token} {pid} {now}");
        for _ in 0..2 {
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut f) => {
                    use std::io::Write;
                    f.write_all(content.as_bytes())
                        .map_err(|e| LockError::Io(e.to_string()))?;
                    return Ok(FileLock { path, token, pid });
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    let held = fs::read_to_string(&path)
                        .ok()
                        .and_then(|t| parse_lock(&t))
                        .filter(|(_, h)| now.saturating_sub(h.since) < LOCK_STALE_SECS);
                    if let Some((_, holder)) = held {
                        return Err(LockError::Held(holder));
                    }
                    // Verrou abandonné ou illisible : repris.
                    let _ = fs::remove_file(&path);
                }
                Err(e) => return Err(LockError::Io(e.to_string())),
            }
        }
        Err(LockError::Io("verrou repris par une autre instance".into()))
    }

    /// Rafraîchit l'heure du verrou ; sans effet si un autre jeton l'a repris.
    pub fn refresh(&self, now: u64) -> io::Result<()> {
        let ours = fs::read_to_string(&self.path)
            .ok()
            .and_then(|t| parse_lock(&t))
            .is_none_or(|(token, _)| token == self.token);
        if ours {
            fs::write(&self.path, format!("{} {} {now}", self.token, self.pid))?;
        }
        Ok(())
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let ours = fs::read_to_string(&self.path)
            .ok()
            .and_then(|t| parse_lock(&t))
            .is_some_and(|(token, _)| token == self.token);
        if ours {
            let _ = fs::remove_file(&self.path);
        }
    }
}
const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

pub(crate) fn sha256_hex(data: &[u8]) -> String {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, c) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [
                t1.wrapping_add(t2),
                v[0],
                v[1],
                v[2],
                v[3].wrapping_add(t1),
                v[4],
                v[5],
                v[6],
            ];
        }
        for (a, b) in h.iter_mut().zip(v) {
            *a = a.wrapping_add(b);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_vecteurs_connus() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(revision_of(b"abc").len(), "sha256:".len() + 64);
    }

    #[test]
    fn analyse_du_contenu_du_verrou() {
        assert_eq!(
            parse_lock("t 12 345"),
            Some((
                "t".into(),
                LockHolder {
                    pid: 12,
                    since: 345
                }
            ))
        );
        for bad in ["", "t", "t 1", "t x 3", "t 1 y", "t 1 2 3", "\0\u{1}"] {
            assert_eq!(parse_lock(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn nom_du_verrou() {
        assert_eq!(
            lock_path(Path::new("a/b.pforge.json")),
            PathBuf::from("a/b.pforge.json.lock")
        );
    }
}
