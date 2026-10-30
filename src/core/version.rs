//! The one place the product name and version are written.
//!
//! Both are shown in the window title and stamped into the executable's file
//! properties (by `build.rs`, from the same `Cargo.toml` version), so a build
//! can be identified from the binary alone without the directory it came from.

pub const APP_NAME: &str = "Roch StressTest";

/// The version, taken from `Cargo.toml` so there is exactly one place to bump.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Windows file-version resources want four numbers.
pub fn version_tuple() -> [u16; 4] {
    let mut parts = [0u16; 4];
    for (index, part) in VERSION.split('.').take(3).enumerate() {
        parts[index] = part.parse().unwrap_or(0);
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_has_four_numbers_ending_in_zero() {
        let tuple = version_tuple();
        assert_eq!(tuple[3], 0);
        assert_eq!(format!("{}.{}.{}", tuple[0], tuple[1], tuple[2]), VERSION);
    }
}
