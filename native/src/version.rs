//! Version comparison for Packmule's own releases.

/// "1.2.10" > "1.2.9"; non-numeric parts compare as 0.
pub fn newer_version(a: &str, b: &str) -> bool {
    let parts = |s: &str| -> Vec<u64> { s.split(['.', '-']).map(leading_int).collect() };
    let (pa, pb) = (parts(a), parts(b));
    for i in 0..pa.len().max(pb.len()) {
        let (x, y) = (pa.get(i).copied().unwrap_or(0), pb.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

/// Like JavaScript's parseInt: the leading digits, or 0.
fn leading_int(s: &str) -> u64 {
    let digits: String = s.trim_start().chars().take_while(char::is_ascii_digit).collect();
    digits.parse().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_numerically() {
        assert!(newer_version("1.2.10", "1.2.9"));
        assert!(!newer_version("1.2.9", "1.2.10"));
        assert!(!newer_version("0.1.0", "0.1.0"));
        assert!(newer_version("0.2.0", "0.1.9"));
        assert!(newer_version("1.0.1", "1.0"));
        assert!(!newer_version("1.0", "1.0.0"));
        assert!(newer_version("2.0.0-beta", "1.9.9"));
    }
}
