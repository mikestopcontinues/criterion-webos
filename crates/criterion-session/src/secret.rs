use serde::{Deserialize, Deserializer};
use zeroize::Zeroizing;

/// Secret text deliberately has no Clone or serializing implementation. Callers
/// expose a borrowed value only to the intended UI, HTTP or secure-store seam.
pub struct Secret(pub(crate) Zeroizing<String>);
impl Secret {
    pub fn expose(&self) -> &str {
        self.0.as_str()
    }
    pub(crate) fn valid(&self, maximum: usize) -> bool {
        !self.0.is_empty() && self.0.len() <= maximum && !self.0.chars().any(char::is_control)
    }
}
impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(|value| Self(Zeroizing::new(value)))
    }
}
impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[redacted]")
    }
}
impl std::fmt::Display for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[redacted]")
    }
}

pub struct SecretBody(Zeroizing<Vec<u8>>);
impl SecretBody {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(Zeroizing::new(bytes))
    }
    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}
impl AsRef<[u8]> for SecretBody {
    fn as_ref(&self) -> &[u8] {
        self.expose()
    }
}
impl std::fmt::Debug for SecretBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[redacted]")
    }
}
impl std::fmt::Display for SecretBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[redacted]")
    }
}
