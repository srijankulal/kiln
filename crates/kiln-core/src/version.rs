//! Kiln's release version.
//!
//! Releases are tagged `0.xxx.xx` (e.g. `0.003.05`), which isn't valid semver, so the
//! version can't live in Cargo.toml. The release workflow sets `KILN_VERSION` to the tag
//! while building; local and CI builds without it are marked as development builds.

/// The version this binary was released as, or `"dev"` for a local build.
pub const VERSION: &str = match option_env!("KILN_VERSION") {
    Some(v) => v,
    None => "dev",
};

/// True for builds made outside the release workflow.
pub fn is_dev_build() -> bool {
    parse(VERSION).is_none()
}

/// Version for display: `0.003.05` stays as is; local builds read "dev".
pub fn display() -> String {
    if is_dev_build() {
        "dev".to_string()
    } else {
        VERSION.trim_start_matches('v').to_string()
    }
}

/// Numeric parts of a version or tag (`v0.003.05` -> `[0, 3, 5]`). `None` if it isn't one.
pub fn parse(v: &str) -> Option<Vec<u64>> {
    let v = v.trim().trim_start_matches('v');
    let core = v.split(['-', '+']).next()?;
    if core.is_empty() {
        return None;
    }
    core.split('.').map(|p| p.parse::<u64>().ok()).collect()
}

/// Whether `candidate` is a newer release than `current`. Compares numerically, so
/// `0.003.10` > `0.003.09` and `0.1.0` == `0.001.00`. A dev build is older than any release.
pub fn is_newer(current: &str, candidate: &str) -> bool {
    let Some(mut cand) = parse(candidate) else {
        return false;
    };
    let Some(mut cur) = parse(current) else {
        return true;
    };
    let n = cand.len().max(cur.len());
    cand.resize(n, 0);
    cur.resize(n, 0);
    cand > cur
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_numerically() {
        assert!(is_newer("0.003.05", "0.003.06"));
        assert!(is_newer("0.003.99", "0.004.00"));
        assert!(is_newer("0.003.09", "0.003.10"));
        assert!(is_newer("0.003.05", "v0.003.06"));
        assert!(!is_newer("0.003.06", "0.003.05"));
        assert!(!is_newer("0.003.05", "0.003.05"));
        assert!(!is_newer("0.1.0", "0.001.00"));
        assert!(!is_newer("0.003.05", "nightly"));
    }

    #[test]
    fn dev_builds_are_older_than_releases() {
        assert!(is_newer("dev", "0.000.01"));
        assert_eq!(parse("dev"), None);
        assert_eq!(parse("0.003.05"), Some(vec![0, 3, 5]));
    }
}
